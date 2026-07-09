#!/usr/bin/env ruby
# frozen_string_literal: true

# rt Ruby harness. Embedded in the rt binary and executed as a child process.
# Modes:
#   --emit-metadata <root>  load tasks/**/*.rb and print metadata JSON
#   --run <root> <task>     read args JSON from stdin and execute one task
#
# The DSL (task/desc/param/option) is extended onto the top-level object so
# task files read naturally, while shared state lives on the RT module.

require "json"
require "stringio"

PROTOCOL_VERSION = 2
ERROR_SENTINEL = "\x1e__RT_ERROR__"

module RT
  OPTION_TYPES = %w[string integer boolean].freeze

  class Task
    attr_reader :name, :description, :params, :options, :file, :block

    def initialize(name:, description:, params:, options:, file:, block:)
      @name = name
      @description = description
      @params = params
      @options = options
      @file = file
      @block = block
    end

    def to_meta
      {
        "name" => @name,
        "description" => @description,
        "file" => @file,
        "params" => @params.map(&:to_meta),
        "options" => @options.map(&:to_meta),
        "gems" => RT.gems_for(@file)
      }
    end
  end

  class Param
    attr_reader :name

    def initialize(name, required:, default:, enum:, description:)
      @name = name
      @required = required
      @default = default
      @enum = enum
      @description = description
    end

    def to_meta
      {
        "name" => @name,
        "required" => @required,
        "default" => @default,
        "enum" => @enum,
        "description" => @description
      }
    end
  end

  class Option
    attr_reader :name

    def initialize(name, type:, default:, description:)
      @name = name
      @type = type
      @default = default
      @description = description
    end

    def to_meta
      {
        "name" => @name,
        "type" => @type,
        "default" => @default,
        "description" => @description
      }
    end
  end

  class Registry
    attr_reader :tasks, :errors

    def initialize
      @tasks = []
      @by_name = {}
      @errors = []
    end

    def add(task)
      if @by_name.key?(task.name)
        @errors << {
          "file" => task.file,
          "class" => "DuplicateTask",
          "message" => "task #{task.name.inspect} is already defined"
        }
        return
      end
      @by_name[task.name] = task
      @tasks << task
    end

    def find(name)
      @by_name[name]
    end

    def record_error(file:, klass:, message:)
      @errors << { "file" => file, "class" => klass, "message" => message }
    end
  end

  class Context
    attr_reader :params, :options

    def initialize(params:, options:, dry_run:)
      @params = params
      @options = options
      @dry_run = dry_run
    end

    def dry_run?
      @dry_run
    end

    def dry_run
      @dry_run
    end

    def param(name)
      @params[name.to_s]
    end

    def option(name)
      @options[name.to_s]
    end

    def say(message)
      puts message
    end
  end

  # DSL accumulates desc/param/option in a pending buffer, consumed when the
  # next `task` is declared (the Rake `desc` model).
  module DSL
    def desc(text)
      RT.pending[:description] = text
    end

    # Declared at the top level of a task file; scoped to that file. On the
    # top-level object this shadows Kernel#gem, but task blocks run on a fresh
    # object where Kernel#gem is visible again, so declarations are structurally
    # limited to file scope.
    def gem(name, *requirements)
      RT.file_gems[RT.current_file] << {
        "name" => name.to_s,
        "requirements" => requirements.map(&:to_s)
      }
    end

    def param(name, required: false, default: nil, enum: nil, description: nil)
      RT.pending[:params] << Param.new(
        name.to_s,
        required: required,
        default: default,
        enum: enum ? enum.map(&:to_s) : nil,
        description: description
      )
    end

    def option(name, type: :string, default: nil, description: nil)
      t = type.to_s
      t = "string" unless OPTION_TYPES.include?(t)
      RT.pending[:options] << Option.new(
        name.to_s,
        type: t,
        default: default,
        description: description
      )
    end

    def task(name, &block)
      pending = RT.consume_pending
      RT.registry.add(Task.new(
        name: name.to_s,
        description: pending[:description],
        params: pending[:params],
        options: pending[:options],
        file: RT.current_file,
        block: block
      ))
    end
  end

  class << self
    attr_accessor :current_file

    def registry
      @registry ||= Registry.new
    end

    def pending
      @pending ||= fresh_pending
    end

    def file_gems
      @file_gems ||= Hash.new { |h, k| h[k] = [] }
    end

    def gems_for(file)
      file_gems.fetch(file, [])
    end

    def consume_pending
      current = pending
      @pending = fresh_pending
      current
    end

    def fresh_pending
      { description: nil, params: [], options: [] }
    end
  end
end

def relative_path(root, path)
  abs = File.expand_path(path)
  base = File.expand_path(root)
  if abs.start_with?(base + File::SEPARATOR)
    abs[(base.length + 1)..-1]
  else
    path
  end
end

def load_tasks(root)
  self.extend(RT::DSL)
  pattern = File.join(root, "tasks", "**", "*.rb")
  Dir.glob(pattern).sort.each do |file|
    rel = relative_path(root, file)
    RT.current_file = rel
    RT.consume_pending # drop any dangling desc/param from a prior file
    begin
      load file
    rescue SystemExit => e
      RT.registry.record_error(
        file: rel, klass: "SystemExit",
        message: "task file called exit(#{e.status}) while loading"
      )
    rescue ScriptError, StandardError => e
      RT.registry.record_error(file: rel, klass: e.class.name, message: e.message)
    end
  end
  RT.current_file = nil
end

def with_silenced_stdout
  original = $stdout
  $stdout = StringIO.new
  begin
    yield
  ensure
    $stdout = original
  end
end

def clean_backtrace(root, backtrace)
  return [] unless backtrace
  harness = File.expand_path(__FILE__)
  backtrace
    .reject do |line|
      path = line.split(":", 2).first
      path && File.expand_path(path) == harness
    end
    .map { |line| relative_path(root, line) }
    .first(5)
end

def emit_metadata(root)
  with_silenced_stdout { load_tasks(root) }
  payload = {
    "protocol_version" => PROTOCOL_VERSION,
    "tasks" => RT.registry.tasks.map(&:to_meta),
    "errors" => RT.registry.errors
  }
  puts JSON.generate(payload)
end

# Resolve a task file's declared gems with bundler/inline before the block
# runs. Nothing may reach stdout (agents pipe task stdout to tools like jq), so
# any Bundler chatter is redirected to stderr. Install failure is an
# environment error (exit 74), matching rt's exit-code contract.
def install_gems(gems)
  return if gems.nil? || gems.empty?

  require "bundler/inline"
  summary = gems.map { |g| [g["name"], *g["requirements"]].join(" ").strip }.join(", ")
  warn "rt: resolving gems (#{summary})"

  original = $stdout
  $stdout = $stderr
  begin
    gemfile(true, quiet: true) do
      source(ENV["RT_GEM_SOURCE"] || "https://rubygems.org")
      gems.each { |g| gem(g["name"], *g["requirements"]) }
    end
  ensure
    $stdout = original
  end
rescue Gem::Exception, Bundler::BundlerError, StandardError => e
  warn "rt: failed to resolve gems (#{e.class}): #{e.message}"
  exit 74
end

def run_task(root, name)
  input = $stdin.read
  args = input.empty? ? {} : JSON.parse(input)
  params = args["params"] || {}
  options = args["options"] || {}
  dry_run = args["dry_run"] ? true : false

  with_silenced_stdout { load_tasks(root) }

  task = RT.registry.find(name)
  if task.nil?
    warn "rt: task #{name.inspect} not found while running"
    exit 70
  end

  # Installed even under --dry-run: the task block still `require`s these gems,
  # so they must be resolvable before the block runs.
  install_gems(RT.gems_for(task.file))

  ctx = RT::Context.new(params: params, options: options, dry_run: dry_run)
  begin
    # Turn the block into a method so a bare `return` inside a task is a valid
    # early exit rather than a LocalJumpError.
    runner = Object.new
    runner.define_singleton_method(:__rt_task__, &task.block)
    runner.__rt_task__(ctx)
  rescue ScriptError, StandardError => e
    payload = {
      "class" => e.class.name,
      "message" => e.message,
      "backtrace" => clean_backtrace(root, e.backtrace)
    }
    $stderr.puts("#{ERROR_SENTINEL} #{JSON.generate(payload)}")
    exit 1
  end
end

def main(argv)
  mode = argv[0]
  case mode
  when "--emit-metadata"
    root = argv[1]
    abort "rt: --emit-metadata requires a root path" if root.nil?
    emit_metadata(root)
  when "--run"
    root = argv[1]
    name = argv[2]
    abort "rt: --run requires <root> <task>" if root.nil? || name.nil?
    run_task(root, name)
  else
    abort "rt: unknown harness mode #{mode.inspect}"
  end
end

main(ARGV)
