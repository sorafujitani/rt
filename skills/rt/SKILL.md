---
name: rt
description: Discover, inspect, run, and author rt tasks. rt turns Ruby scripts in a .rt/tasks/ directory into a CLI with validation, vendor-neutral tool schemas, structured execution results, and dry-run. Use when a repo contains a .rt/ directory, when asked to discover or run its tasks, or when automating repository workflows with rt.
---

# rt

rt turns ordinary Ruby scripts into a discoverable command-line tool for humans and agents. Tasks are described in a small Ruby DSL with names, descriptions, typed params and options, and rt provides help, validation, machine-readable metadata, and a dry-run mode. It works in any repository (Go, TypeScript, anything) as long as a Ruby interpreter is available.

Detect rt in a repo by the presence of a `.rt/` directory. rt finds the project by walking up from the current directory; `RT_ROOT` (the directory containing `.rt/`) overrides discovery.

## Discover and run tasks

For agent-facing discovery, start with the tool catalog:

```bash
rt tools --json             # all tasks as vendor-neutral object input schemas
rt tools --json <task>      # one task, with the same top-level catalog shape
rt help <task> --json       # ordered params and named options for CLI invocation
rt run --json <task> [args...] # structured execution result
```

Treat each catalog entry's `task` as an exact identifier; do not normalize it.
Check the catalog's `errors` before selecting a tool. To invoke an input object,
use `help --json` to map params to declaration-order positionals, options to
`--name value` flags, and `dry_run: true` to `--dry-run`. Omit false boolean
options and values that should use their defaults.

For example, map `{"environment":"production","workers":4,"force":true}` to:

```bash
rt run --json deploy production --workers 4 --force
```

Use `rt list --json` when raw task files, gem requirements, or full declaration
metadata are needed rather than a tool schema.

Prefer `rt run --json` when an agent needs to interpret the result. It emits one
JSON object on stdout and nothing on stderr, including on task, usage, and
environment failures. The process exit code is still meaningful. Captured
stdout/stderr use `encoding: "utf-8"` for text and `encoding: "base64"` for
non-UTF-8 bytes. Each stream captures at most the first 1,048,576 bytes and
reports whether more output was drained. If a task declares its own `--json`
option, pass task arguments after the separator: `rt run --json my-task -- --json`.

Human-readable variants are `rt list` and `rt help <task>`. Every task accepts `--dry-run`, which sets `ctx.dry_run?` to true inside the task. Use it to preview a task with side effects before running it for real.

The exact JSON shapes, the full environment-variable table, and project layout details are in [reference.md](reference.md). Read it when you need a JSON schema or an env var beyond `RT_ROOT`.

## Author tasks

Put task files in `.rt/tasks/` (loaded from `.rt/tasks/**/*.rb`). Each `task` yields a builder that owns its description, inputs, requirements, and run block. Copy this template:

```ruby
# .rt/tasks/gh-release.rb
gem "octokit", "~> 8.0"

task "gh:release" do |t|
  t.desc "Create a GitHub release"
  t.param :tag, required: true, description: "tag to release"
  t.option :draft, type: :boolean, default: false, description: "create as draft"
  t.option :retries, type: :integer, default: 3, range: 1..10,
                     description: "API retry count"
  t.run do |ctx|
    require "octokit"   # require INSIDE the run block, never at the top level
    ctx.say "releasing #{ctx.param(:tag)} (draft: #{ctx.option(:draft)})"
    return if ctx.dry_run?
    # real work here
  end
end
```

Rules:

- `param name, required:, default:, enum:, description:` is a positional argument. Command-line values always arrive as `String`, so a non-null default must be a string. A required param cannot have a default. `enum` restricts accepted values.
- `option name, type:, default:, range:, description:` is a `--flag`. `type` is one of `:string`, `:integer`, `:boolean`, and the default must match that type. An integer option may declare an inclusive integer `range`; rt validates both its default and CLI values. Booleans are set by presence (`--force`) or explicitly (`--force=false`).
- Param and option names must be unique and cannot overlap. `dry_run` and `dry-run` are reserved by rt. Invalid declarations become `InvalidDeclaration` load errors and are not registered.
- The context API is `ctx.param(:name)`, `ctx.option(:name)`, `ctx.dry_run?`, `ctx.project_root`, and `ctx.say(message)`. `ctx.project_root` is a `Pathname` for project tasks and `nil` for global tasks. A bare `return` is a valid early exit.
- The task name is exactly what you declare. There is no automatic namespacing from file paths. Declaring the same name twice is an error.
- Tasks cannot read interactive input from stdin. Pass everything as params and options.

## Rails applications

Declare `t.requires :rails` inside a task that uses the Rails application:

```ruby
task "users:count" do |t|
  t.desc "Count users"
  t.requires :rails
  t.run do |ctx|
    ctx.say User.count
  end
end
```

`list`, `help`, and `tools` expose the `rails` requirement without inspecting
the project-root `Gemfile` or booting the application. `run` requires the
project-root `Gemfile` and a complete Bundler environment, changes to the
project root, and loads `config/environment.rb` before the block. Rails tasks
cannot be global and cannot be declared in a file with top-level inline `gem`
declarations. Pass the environment normally, for example
`RAILS_ENV=test rt run --json users:count`.

When replacing Rake tasks, use the full name such as `users:count` directly and
move prerequisites other than the Rails environment into normal Ruby classes
or modules. rt does not load Rakefiles or implement Rake task dependencies.

## Gems

Declare gems a task needs with top-level `gem` lines. rt resolves them with `bundler/inline` just before the task runs, so no project `Gemfile` is required.

- Declare at the top level, `require` inside the task block. A top-level `require` of a declared gem fails discovery and is reported as a load error.
- Gems are scoped to the file that declares them and apply to every task in it.
- Gem tasks are self-contained. They run under plain Ruby in a scrubbed environment (`BUNDLE_GEMFILE`, `RUBYOPT`, `RUBYLIB`, `GEM_HOME`, `GEM_PATH`, and all `BUNDLE_*` stripped) and do not see gems from a project `Gemfile`. Declare everything the task needs.
- Gems install into an isolated per-Ruby cache dir (`RT_GEM_HOME`, else `$XDG_CACHE_HOME/rt/gems`, else `~/.cache/rt/gems`), never the default gem environment. Deleting it is safe. `RT_GEM_SOURCE` overrides the gem source.
- Installation runs even under `--dry-run`, because the block still requires the gems.
- If gem resolution fails, rt exits `74` and the task does not run.

## Global tasks

rt also loads machine-wide tasks from `<config_dir>/tasks/`, where the config dir is `RT_CONFIG_DIR`, else `$XDG_CONFIG_HOME/rt`, else `~/.config/rt`. Inside a project, `rt list` shows project and global sections; in JSON every task carries a `source` field of `project` or `global`. On a name collision the project task wins and the shadowed global task is reported as a `ShadowedTask` warning.

Trust caveat: the top level of every task file executes during discovery on every rt invocation (`list`, `help`, `tools`, `run`), from any directory. Treat write access to `<config_dir>/tasks/` as code-execution access. Never write untrusted content there, and keep top-level code in task files to declarations only.

## Exit codes

| code | meaning |
|------|---------|
| 0    | success |
| 1    | the task raised an exception |
| 2    | usage error (unknown task, failed validation) |
| 70   | internal error (harness failure, unparseable metadata) |
| 74   | environment error (Ruby missing or failed to start) |
| n    | the task called `exit n` |

## Ruby resolution

rt picks a Ruby in this order:

1. `RT_RUBY`, if set. A path to a single Ruby executable, not a shell command line.
2. `bundle exec ruby` when the task home contains a `Gemfile` and `bundle` is on `PATH`.
3. `ruby` on `PATH`.

The project-root `Gemfile` is resolved only when a project task runs, never for
`list`, `help`, or `tools`.

If Bundler is missing or `bundle exec` fails, rt warns and falls back to plain
`ruby`. Tasks that declare gems always run under plain Ruby regardless of a
`Gemfile`. rt strips `RUBYOPT` and `RUBYLIB` from every Ruby it launches, and
plain discovery removes activation inherited from an outer `bundle exec`.

Rails tasks never use the plain-Ruby fallback. A missing project `Gemfile`,
missing Bundler, incomplete bundle, or Rails boot failure exits 74. Rails task
execution uses the application's Bundler runtime rather than `RT_RUBY` and
does not reuse activation state from an outer `bundle exec`.
