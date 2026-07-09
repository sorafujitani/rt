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
require "fileutils"
require "rbconfig"

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
      message = e.message
      # A gem required at the top level fails discovery (gems are only
      # installed at run time). Point the author at the fix.
      if e.is_a?(LoadError) && !RT.file_gems.fetch(rel, []).empty?
        message += " (declared gems must be required inside the task block, not at the top level)"
      end
      RT.registry.record_error(file: rel, klass: e.class.name, message: message)
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
# Redirect gem installs into a per-ABI cache dir instead of the default gem
# environment, which on macOS system Ruby is unwritable without sudo. Built
# before requiring bundler/inline so bundler resolves against the isolated
# paths. Native extensions are ABI-specific, so the dir is keyed on engine +
# ruby_version. GEM_PATH is set explicitly (isolated home + Ruby's default dir):
# with only GEM_HOME set, rubygems' PathSupport re-adds the user gem dir and the
# ambient environment leaks back in. default_dir supplies Ruby's bundled/default
# gems (bundler, rake) and must be read before ENV is mutated.
def isolate_gem_home
  configured = ENV["RT_GEM_HOME"]
  configured = nil if configured.nil? || configured.strip.empty?
  base = configured || File.join(cache_home, "rt", "gems")
  home = File.join(base, "#{RUBY_ENGINE}-#{RbConfig::CONFIG["ruby_version"]}")

  default_dir = Gem.default_dir
  begin
    FileUtils.mkdir_p(home)
  rescue SystemCallError => e
    warn "rt: cannot create gem cache dir #{home}: #{e.message}"
    exit 74
  end

  ENV["GEM_HOME"] = home
  ENV["GEM_PATH"] = [home, default_dir].join(File::PATH_SEPARATOR)
  Gem.clear_paths
end

def cache_home
  xdg = ENV["XDG_CACHE_HOME"]
  xdg.nil? || xdg.strip.empty? ? File.join(Dir.home, ".cache") : xdg
end

def install_gems(gems)
  return if gems.nil? || gems.empty?

  isolate_gem_home

  # Require bundler/inline before the main rescue so its absence (a broken Ruby)
  # is reported as exit 74 rather than raising a NameError when the rescue tries
  # to name Bundler::BundlerError on a Ruby without Bundler loaded.
  begin
    require "bundler/inline"
  rescue LoadError => e
    warn "rt: cannot load bundler/inline to resolve gems: #{e.message}"
    exit 74
  end

  summary = gems.map { |g| [g["name"], *g["requirements"]].join(" ").strip }.join(", ")
  warn "rt: resolving gems (#{summary})"

  # Nothing may reach stdout (agents pipe task stdout to tools like jq). Redirect
  # fd 1 to stderr at the OS level, not just $stdout, so a native-extension build
  # subprocess writing straight to fd 1 is captured too. Restored afterward.
  saved = $stdout.dup
  $stdout.reopen($stderr)
  begin
    # An empty RT_GEM_SOURCE is truthy in Ruby, so treat blank as unset and fall
    # back to the default source rather than an empty source URL.
    configured = ENV["RT_GEM_SOURCE"]
    configured = nil if configured.nil? || configured.strip.empty?
    gemfile(true, quiet: true) do
      source(configured || "https://rubygems.org")
      gems.each { |g| gem(g["name"], *g["requirements"]) }
    end
  # Resolution failure is an environment problem, not a task bug, so any failure
  # maps to exit 74: missing gems, unreachable/invalid sources, network and OS
  # errors, and malformed version requirements all surface as StandardError.
  rescue StandardError => e
    warn "rt: failed to resolve gems (#{e.class}): #{e.message}"
    exit 74
  ensure
    $stdout.reopen(saved)
    saved.close
  end
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
