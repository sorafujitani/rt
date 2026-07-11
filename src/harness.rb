#!/usr/bin/env ruby
# frozen_string_literal: true

# rt Ruby harness. Embedded in the rt binary and executed as a child process.
# Modes:
#   --emit-metadata <root>  load tasks/**/*.rb and print metadata JSON
#   --run <root> <task>     read args JSON from stdin and execute one task
#
# The file-level DSL (`task` and `gem`) is extended onto the top-level object.
# Task-specific declarations live on the builder yielded by `task`.

require "json"
require "stringio"
require "fileutils"
require "pathname"
require "rbconfig"

HARNESS_PROTOCOL_VERSION = 4
CONTROL_FD_ENV = "RT_CONTROL_FD"

module RT
  OPTION_TYPES = %w[string integer boolean].freeze
  RESERVED_ARGUMENT_NAMES = %w[dry-run dry_run].freeze

  class Task
    attr_reader :name, :description, :params, :options, :requirements, :file, :block

    def initialize(name:, description:, params:, options:, requirements:, file:, block:)
      @name = name
      @description = description
      @params = params
      @options = options
      @requirements = requirements
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
        "gems" => RT.gems_for(@file),
        "requirements" => @requirements
      }
    end

    def declaration_errors
      errors = []
      param_names = @params.map(&:name)
      option_names = @options.map(&:name)

      duplicate_names(param_names).each do |name|
        errors << "duplicate param name #{name.inspect}"
      end
      duplicate_names(option_names).each do |name|
        errors << "duplicate option name #{name.inspect}"
      end
      (param_names & option_names).each do |name|
        errors << "name #{name.inspect} is used as both a param and an option"
      end
      ((param_names + option_names) & RESERVED_ARGUMENT_NAMES).each do |name|
        errors << "name #{name.inspect} is reserved by rt"
      end
      duplicate_names(@requirements).each do |name|
        errors << "duplicate requirement #{name.inspect}"
      end
      (@requirements - ["rails"]).each do |name|
        errors << "unknown requirement #{name.inspect}"
      end
      if @requirements.include?("rails") && !RT.gems_for(@file).empty?
        errors << "Rails tasks cannot declare inline gems"
      end

      @params.each { |param| errors.concat(param.declaration_errors) }
      @options.each { |option| errors.concat(option.declaration_errors) }
      errors
    end

    private

    def duplicate_names(names)
      names.group_by { |name| name }.select { |_name, values| values.length > 1 }.keys
    end
  end

  class Param
    attr_reader :name, :required, :default, :enum

    def initialize(name, required:, default:, enum:, description:)
      @name = name
      @required = required
      @default = default
      @enum = enum
      @description = description
    end

    def declaration_errors
      errors = []
      unless @required == true || @required == false
        errors << "param #{@name.inspect} required must be true or false"
      end
      if @required && !@default.nil?
        errors << "required param #{@name.inspect} cannot have a default"
      elsif !@default.nil? && !@default.is_a?(String)
        errors << "param #{@name.inspect} default must be a string"
      end
      if !@default.nil? && @enum && !@enum.include?(@default.to_s)
        errors << "param #{@name.inspect} default must be one of: #{@enum.join(', ')}"
      end
      errors
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
    attr_reader :name, :type, :default, :minimum, :maximum

    def initialize(name, type:, default:, range:, description:)
      @name = name
      @type = type
      @default = default
      @range = range
      @minimum = range.begin if range.is_a?(Range)
      @maximum = range.end if range.is_a?(Range)
      @description = description
    end

    def declaration_errors
      unless OPTION_TYPES.include?(@type)
        return ["option #{@name.inspect} has unknown option type #{@type.inspect}"]
      end
      errors = []
      if @range
        unless @type == "integer"
          errors << "option #{@name.inspect} range is only supported for integer options"
        end
        unless @range.is_a?(Range) && !@range.exclude_end? &&
               @minimum.is_a?(Integer) && @maximum.is_a?(Integer) && @minimum <= @maximum
          errors << "option #{@name.inspect} range must be an inclusive integer range"
        end
      end
      return errors if @default.nil?

      valid = case @type
              when "string" then @default.is_a?(String)
              when "integer" then @default.is_a?(Integer)
              when "boolean" then @default == true || @default == false
              end
      expected = @type == "integer" ? "an integer" : "a #{@type}"
      errors << "option #{@name.inspect} default must be #{expected}" unless valid
      valid_range = @type == "integer" && @range.is_a?(Range) &&
                    !@range.exclude_end? && @minimum.is_a?(Integer) &&
                    @maximum.is_a?(Integer) && @minimum <= @maximum
      if valid && valid_range && !@range.cover?(@default)
        errors << "option #{@name.inspect} default must be within #{@minimum}..#{@maximum}"
      end
      errors
    end

    def to_meta
      {
        "name" => @name,
        "type" => @type,
        "default" => @default,
        "minimum" => @minimum,
        "maximum" => @maximum,
        "description" => @description
      }
    end
  end

  class TaskBuilder
    attr_reader :description, :params, :options, :requirements, :block

    def initialize
      @description = nil
      @params = []
      @options = []
      @requirements = []
      @block = nil
    end

    def desc(text)
      @description = text
    end

    def param(name, required: false, default: nil, enum: nil, description: nil)
      @params << Param.new(
        name.to_s,
        required: required,
        default: default,
        enum: enum ? enum.map(&:to_s) : nil,
        description: description
      )
    end

    def option(name, type: :string, default: nil, range: nil, description: nil)
      @options << Option.new(
        name.to_s,
        type: type.to_s,
        default: default,
        range: range,
        description: description
      )
    end

    def requires(*requirements)
      @requirements.concat(requirements.map(&:to_s))
    end

    def run(&block)
      @block = block
    end

    def declaration_errors
      @block ? [] : ["run block is required"]
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
      declaration_errors = task.declaration_errors
      unless declaration_errors.empty?
        record_error(
          file: task.file,
          klass: "InvalidDeclaration",
          message: "task #{task.name.inspect}: #{declaration_errors.join('; ')}"
        )
        return
      end
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

    # Inline gem declarations apply to every task in their file, including a
    # declaration written after a task. Recheck at end-of-file so ordering
    # cannot bypass the Rails/isolated-gem boundary.
    def validate_file(file)
      invalid = @tasks.select do |task|
        task.file == file && task.requirements.include?("rails") && !RT.gems_for(file).empty?
      end
      invalid.each do |task|
        @tasks.delete(task)
        @by_name.delete(task.name)
        record_error(
          file: file,
          klass: "InvalidDeclaration",
          message: "task #{task.name.inspect}: Rails tasks cannot declare inline gems"
        )
      end
    end
  end

  class Context
    attr_reader :params, :options, :project_root

    def initialize(params:, options:, dry_run:, project_root:)
      @params = params
      @options = options
      @dry_run = dry_run
      @project_root = project_root.nil? ? nil : Pathname.new(project_root)
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

  module DSL
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

    def task(name)
      builder = TaskBuilder.new
      yield builder
      task = Task.new(
        name: name.to_s,
        description: builder.description,
        params: builder.params,
        options: builder.options,
        requirements: builder.requirements,
        file: RT.current_file,
        block: builder.block
      )
      errors = builder.declaration_errors
      if errors.empty?
        RT.registry.add(task)
      else
        RT.registry.record_error(
          file: RT.current_file,
          klass: "InvalidDeclaration",
          message: "task #{name.to_s.inspect}: #{errors.join('; ')}"
        )
      end
    end
  end

  class << self
    attr_accessor :current_file

    def registry
      @registry ||= Registry.new
    end

    def file_gems
      @file_gems ||= Hash.new { |h, k| h[k] = [] }
    end

    def gems_for(file)
      file_gems.fetch(file, [])
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
    ensure
      RT.registry.validate_file(rel)
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
    "harness_protocol_version" => HARNESS_PROTOCOL_VERSION,
    "tasks" => RT.registry.tasks.map(&:to_meta),
    "errors" => RT.registry.errors
  }
  puts JSON.generate(payload)
end

# Redirect gem installs into a per-ABI cache dir instead of the default gem
# environment, which on macOS system Ruby is unwritable without sudo. Built
# before requiring bundler/inline so bundler resolves against the isolated
# paths. Native extensions are ABI-specific, so the dir is keyed on engine +
# ruby_version. GEM_PATH is set explicitly (isolated home + Ruby's default dir):
# with only GEM_HOME set, rubygems' PathSupport re-adds the user gem dir and the
# ambient environment leaks back in. Returns the isolated home so the caller can
# lock it. Any failure here is an environment error (exit 74).
def isolate_gem_home
  configured = ENV["RT_GEM_HOME"]
  configured = nil if configured.nil? || configured.strip.empty?
  base = configured || File.join(cache_home, "rt", "gems")
  home = File.join(base, "#{RUBY_ENGINE}-#{RbConfig::CONFIG["ruby_version"]}")

  begin
    default_dir = Gem.default_dir
    FileUtils.mkdir_p(home)
  rescue SystemCallError => e
    warn "rt: cannot create gem cache dir #{home}: #{e.message}"
    exit 74
  end

  ENV["GEM_HOME"] = home
  ENV["GEM_PATH"] = [home, default_dir].join(File::PATH_SEPARATOR)
  Gem.clear_paths
  home
end

def cache_home
  xdg = ENV["XDG_CACHE_HOME"]
  xdg.nil? || xdg.strip.empty? ? File.join(Dir.home, ".cache") : xdg
end

# Resolve a task file's declared gems with bundler/inline before the block runs.
# Nothing may reach stdout (agents pipe task stdout to tools like jq), so any
# Bundler chatter is redirected to stderr. Install failure is an environment
# error (exit 74), matching rt's exit-code contract.
def install_gems(gems)
  return if gems.nil? || gems.empty?

  home = isolate_gem_home

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

  # rt runs agents in parallel against a shared gem home, so two processes can
  # try the first install of the same gem at once and collide in rubygems'
  # installer (transient exit 74, a half-written gem dir). Serialize installs per
  # ABI dir with an exclusive file lock; the fast path (gems already present)
  # still resolves under the lock but does no work.
  lock = File.open(File.join(home, ".install.lock"), File::CREAT | File::RDWR, 0o644)
  lock.flock(File::LOCK_EX)

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
    lock.flock(File::LOCK_UN)
    lock.close
  end
end

def write_failure(control, kind, error, root)
  payload = {
    "kind" => kind,
    "class" => error.class.name,
    "message" => error.message,
    "backtrace" => clean_backtrace(root, error.backtrace)
  }
  control.write(JSON.generate(payload))
  control.flush
end

def load_rails_environment(project_root, control, root)
  environment = File.join(project_root, "config", "environment.rb")
  begin
    unless File.file?(environment)
      raise LoadError, "Rails environment not found at #{environment}"
    end
    require environment
  rescue SystemExit => e
    error = RuntimeError.new("Rails environment exited with code #{e.status}")
    error.set_backtrace(e.backtrace)
    write_failure(control, "environment", error, root)
    exit 74
  rescue ScriptError, StandardError => e
    write_failure(control, "environment", e, root)
    exit 74
  end
end

def run_task(root, name, project_root)
  control_fd = ENV.delete(CONTROL_FD_ENV)
  abort "rt: missing task control fd" if control_fd.nil?
  control = IO.for_fd(Integer(control_fd), "w", autoclose: false)
  control.close_on_exec = true

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

  if task.requirements.include?("rails")
    if project_root.nil?
      error = RuntimeError.new("Rails task is missing its project root")
      write_failure(control, "environment", error, root)
      exit 74
    end
    load_rails_environment(project_root, control, root)
  end

  ctx = RT::Context.new(
    params: params,
    options: options,
    dry_run: dry_run,
    project_root: project_root
  )
  begin
    # Turn the block into a method so a bare `return` inside a task is a valid
    # early exit rather than a LocalJumpError.
    runner = Object.new
    runner.define_singleton_method(:__rt_task__, &task.block)
    runner.__rt_task__(ctx)
  rescue ScriptError, StandardError => e
    write_failure(control, "task_exception", e, root)
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
    project_root = argv[3]
    abort "rt: --run requires <root> <task>" if root.nil? || name.nil?
    run_task(root, name, project_root)
  else
    abort "rt: unknown harness mode #{mode.inspect}"
  end
end

main(ARGV)
