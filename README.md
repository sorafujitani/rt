# rt

rt turns ordinary Ruby scripts into a discoverable command-line tool that both
humans and agents can use. You describe tasks in a small Ruby DSL — name,
description, typed params and options — and rt gives them automatic help,
validation, machine-readable metadata, and a dry-run mode.

rt is not tied to Ruby projects. As long as a Ruby interpreter is on the
machine, rt works in Go, TypeScript, or any other repository — or in no
repository at all. Task definitions live in a `.rt/tasks/` directory and are
plain Ruby using only the standard library.

## Requirements

- A Ruby interpreter (`ruby`) on `PATH`, or `RT_RUBY` pointing at one.
- macOS or Linux. Windows is not supported.

## Install

Via Homebrew:

```
brew install sorafujitani/tap/rt
```

Add `--HEAD` to build the latest main instead of the released version.

Or with cargo, from a checkout:

```
cargo install --path .
```

### Agent skill

An [Agent Skill](https://skills.sh) that teaches coding agents to discover,
run, and author rt tasks ships in `skills/rt/`. It targets no specific agent;
install it into Claude Code, Codex, Cursor, or any other supported agent with:

```
npx skills add sorafujitani/rt
```

## Getting started

Create a `.rt/tasks/` directory in your project and add a task file:

```ruby
# .rt/tasks/greet.rb
desc "Greet someone by name"
option :name, type: :string, default: "world", description: "who to greet"
task "greet" do |ctx|
  ctx.say "Hello, #{ctx.option(:name)}!"
end
```

Then:

```
$ rt list
  greet    Greet someone by name

$ rt help greet
Usage: rt run greet [options]

Greet someone by name

Options:
  --name <string> (default: "world") - who to greet
  --dry-run  Preview without side effects

$ rt run greet --name sora
Hello, sora!
```

rt finds your project by walking up from the current directory looking for a
`.rt/` directory. Set `RT_ROOT` to the directory containing `.rt/` to override
discovery.

## Writing tasks

Task files are loaded from `.rt/tasks/**/*.rb`. The DSL uses a pending-buffer
model: `desc`, `param`, `option`, and `requires` describe the next `task`.

```ruby
desc "Deploy the application to an environment"
param :environment, required: true, enum: %w[staging production],
                    description: "target environment"
option :workers, type: :integer, default: 2, description: "worker count"
option :force, type: :boolean, default: false, description: "skip safety checks"
task "deploy" do |ctx|
  ctx.say "deploying to #{ctx.param(:environment)} with #{ctx.option(:workers)} workers"
  return if ctx.dry_run?
  # ... real work ...
end
```

- `param name, required:, default:, enum:, description:` — a positional
  argument. `enum` restricts the accepted values. A value supplied on the
  command line always reaches the task as a `String`, so a non-null `default`
  must also be a string. A required param cannot have a default.
- `option name, type:, default:, description:` — a `--flag`. `type` is one of
  `:string`, `:integer`, `:boolean`. Boolean options are set by presence
  (`--force`) or explicitly (`--force=false`). Only options carry a `type` and
  are coerced accordingly (integers become integers, booleans become booleans).
- Param and option names must be unique within a task and cannot overlap.
  `dry_run` and `dry-run` are reserved by rt. Option defaults must match their
  declared type. Invalid declarations are reported as `InvalidDeclaration`
  load errors and the invalid task is not registered.
- `requires :rails` marks a project task that boots the Rails application
  immediately before its block runs. Requirements are task-scoped.
- The block receives a context: `ctx.param(:name)`, `ctx.option(:name)`,
  `ctx.dry_run?`, `ctx.project_root`, and `ctx.say(message)` for output.
  `ctx.project_root` is a `Pathname` for project tasks and `nil` for global
  tasks. A bare `return` inside a task body is a valid early exit.

The task name is exactly what you declare; there is no automatic namespacing
from file paths. Declaring the same name twice is reported as an error.

`--dry-run` is available for every task and sets `ctx.dry_run?` to true.

### Declaring gems

A task file may declare gems it needs with a top-level `gem` line. rt resolves
them with `bundler/inline` just before the task runs, so a task can depend on a
gem without the project having a `Gemfile`.

```ruby
# .rt/tasks/gh-release.rb
gem "octokit", "~> 8.0"

desc "Create a GitHub release"
param :tag, required: true
task "gh:release" do |ctx|
  require "octokit"       # require INSIDE the task block, not at the top level
  # ...
end
```

Rules and behavior:

- **Declare at the top level, require inside the block.** `gem` lines go at the
  top of the file; the matching `require` must live inside the `task` block.
  Requiring a declared gem at the top level fails discovery (the gem is not
  installed yet) and is reported as a load error with a hint.
- **Gems are scoped to the file** that declares them and apply to every task in
  that file. `rt help` shows a `Gems:` line and `rt list --json` includes a
  `gems` array on each task.
- **Gem tasks are self-contained.** A task that declares gems runs under plain
  Ruby in a scrubbed environment: `BUNDLE_GEMFILE`, `RUBYOPT`, `RUBYLIB`,
  `GEM_HOME`, `GEM_PATH`, and every `BUNDLE_*` variable are stripped, so a task
  behaves the same whether or not `rt` itself was launched under `bundle exec`.
  It does *not* see the gems from a project `Gemfile`; declare everything the
  task needs.
- **Installation runs even under `--dry-run`,** because the task block still
  `require`s the gems and they must be resolvable first.
- **Gems install into an isolated, per-Ruby cache dir,** not the default gem
  environment, so installs never need `sudo` (relevant to the macOS system
  Ruby) and never disturb your other gems. The location is chosen in this order:
    1. `RT_GEM_HOME`, if set.
    2. `$XDG_CACHE_HOME/rt/gems`, if `XDG_CACHE_HOME` is set.
    3. `~/.cache/rt/gems`.

  Under it, gems live in a `<engine>-<ruby_version>` subdirectory (native
  extensions are ABI-specific). The directory is a cache: deleting it is safe,
  and gems are reinstalled on the next run. Set `RT_GEM_SOURCE` to use a gem
  source other than `https://rubygems.org`.
- If resolution fails (missing gem, unreachable source), rt exits `74`
  (environment error) and the task does not run.

The isolated gem environment resolves against your Ruby's built-in
(bundled/default) gems too, so a gem preinstalled there — for instance one you
added by hand under a version manager like rbenv — may be visible to a task
without being declared. Declare every gem a task needs so it does not depend on
that.

### Rails application tasks

Use `requires :rails` when a task needs application models, configuration, or
database connections:

```ruby
desc "Delete inactive users"
requires :rails
option :days, type: :integer, default: 90, description: "inactive period"
task "users:cleanup" do |ctx|
  users = User.where(last_active_at: ...ctx.option(:days).days.ago)
  ctx.say "#{users.count} users will be deleted"
  return if ctx.dry_run?

  users.delete_all
end
```

Discovery commands (`list`, `help`, and `tools`) record the requirement but do
not load `config/environment.rb` or run Rails initializers. `run` requires the
project-root `Gemfile`, verifies the bundle, changes the child working directory
to the project root, and then loads `config/environment.rb` before the task
block. `RAILS_ENV` is inherited normally:

```sh
RAILS_ENV=production rt run users:cleanup --dry-run
```

Rails tasks cannot be global tasks and cannot share a task file with top-level
inline `gem` declarations. Application dependencies belong in the Rails
project's `Gemfile`. Missing Bundler dependencies and Rails boot failures are
environment errors (exit 74); JSON results preserve the exception class,
message, and backtrace.

When migrating from Rake, replace `task name: :environment` with
`requires :rails`, write the fully-qualified task name directly (for example
`"users:cleanup"`), and move prerequisite behavior into ordinary Ruby classes
or modules. rt does not load Rakefiles or implement Rake task graphs.

The dedicated Rails integration CI job verifies Rails 8.1, Ruby 3.4, Bundler
2.6, Active Record, and SQLite. Other application bundles may work, but are not
part of the verified matrix yet.

## Commands

- `rt list` — list tasks with descriptions.
- `rt help <task>` — show usage for one task.
- `rt tools --json [task]` — emit vendor-neutral tool definitions.
- `rt run <task> [args...]` — run a task.
- `rt run --json <task> [args...]` — run a task and capture its result as JSON.

### Machine-readable output

`rt list --json`, `rt help <task> --json`, `rt tools --json [task]`, and
`rt run --json <task>` print JSON on stdout and nothing on stderr when they
succeed. Load errors are reported in the JSON rather than on stderr.

#### Agent tool catalog

`rt tools --json` converts every discovered task into a vendor-neutral tool
definition with an object input schema. Pass a task name to return the same
top-level catalog shape with one tool: `rt tools --json greet`.

```json
{
  "schema_version": 2,
  "tools": [
    {
      "task": "greet",
      "description": "Greet someone by name",
      "source": "project",
      "requirements": [],
      "input_schema": {
        "type": "object",
        "properties": {
          "dry_run": {
            "type": "boolean",
            "description": "Set the task's dry-run flag",
            "default": false
          },
          "name": {
            "type": "string",
            "description": "who to greet",
            "default": "world"
          }
        },
        "required": [],
        "additionalProperties": false
      }
    }
  ],
  "errors": []
}
```

The schema uses task params, options, enums, defaults, descriptions, and the
universal `dry_run` input. Task names are preserved exactly. To invoke a tool,
use `rt help <task> --json` to distinguish ordered params from named options,
then call `rt run --json <task> [args...]`.

The catalog is not an MCP server and does not emit provider-specific OpenAI,
Anthropic, or MCP definitions. It does not normalize task names or execute an
input object. Provider adapters own naming constraints, transport, and the
input-object-to-CLI mapping.

`rt run --json` captures the task's stdout and stderr, completion status, exit
code, and structured Ruby exception details in one result. Each stream retains
at most the first 1,048,576 bytes while continuing to drain and count all
output. UTF-8 output is returned as text; non-UTF-8 output is base64-encoded.
The process still exits with the ordinary rt/task exit code, so callers can use
both the status code and the JSON body.

```json
{
  "schema_version": 2,
  "task": "greet",
  "status": "success",
  "exit_code": 0,
  "stdout": {
    "encoding": "utf-8",
    "data": "Hello, sora!\n",
    "total_bytes": 13,
    "captured_bytes": 13,
    "truncated": false
  },
  "stderr": {
    "encoding": "utf-8",
    "data": "",
    "total_bytes": 0,
    "captured_bytes": 0,
    "truncated": false
  },
  "error": null,
  "load_errors": []
}
```

`--json` is an rt option. If a task itself declares an option named `json`, put
its arguments after `--`: `rt run --json my-task -- --json`.

## Global tasks

Besides a project's `.rt/tasks/`, rt also loads machine-wide tasks from a config
directory, so you can carry personal tasks across every repository — or use rt
with no project at all. Put task files in `<config_dir>/tasks/`, where
`config_dir` is resolved in this order:

1. `RT_CONFIG_DIR`, if set.
2. `$XDG_CONFIG_HOME/rt`, if `XDG_CONFIG_HOME` is set.
3. `~/.config/rt`.

Global tasks work the same as project tasks, and get their own cache and
harness directly under `<config_dir>/`. The config dir has the same shape as a
project's `.rt/` directory.

A word on trust: the top level of every task file runs during discovery, on
*every* rt invocation (`list`, `help`, `tools`, and `run`) from any directory. Because
global task files load regardless of where you are, write access to
`<config_dir>/tasks/` is equivalent to code-execution access whenever you run
rt. Keep that directory as trusted as any startup script.

- Outside any project, rt runs purely from global tasks.
- Inside a project, `rt list` shows two sections, `Project tasks:` and
  `Global tasks:`.
- On a name collision the **project task wins**; the shadowed global task is
  dropped from the task list and reported as a `ShadowedTask` warning. This
  keeps task names unique, including in `--json`, where every task carries a
  `source` field of `project` or `global`.

## Exit codes

| code | meaning |
|------|---------|
| 0    | success |
| 1    | the task raised an exception |
| 2    | usage error (unknown task, failed validation) |
| 70   | internal error (harness failure, unparseable metadata) |
| 74   | environment error (Ruby missing or failed to start) |
| n    | the task called `exit n`; rt exits with the same code |

## Ruby resolution and Bundler

rt resolves Ruby in this order:

1. `RT_RUBY`, if set. This must be the path to a single Ruby executable (for
   example `/usr/bin/ruby` or a `ruby-install` shim). It is not a shell command
   line — compound values like `"bundle exec ruby"` are not supported.
2. `bundle exec ruby` (with `BUNDLE_GEMFILE` set) when a `Gemfile` is present
   and `bundle` is on `PATH`. The `Gemfile` is looked up first inside the task
   home (`.rt/Gemfile`, or `<config_dir>/Gemfile` for global tasks) and then at
   the project root.
3. `ruby` on `PATH`.

If a `Gemfile` is present but `bundle` is not installed, rt warns and falls
back to plain `ruby`. If `bundle exec` is installed but fails (for example when
the bundle's gems are not installed), rt warns and retries discovery once with
plain `ruby`. The plain-Ruby path is the primary one; Bundler is only an
optimization for projects that already use it.

Rails tasks are the strict exception: they require the project-root `Gemfile`
and a complete bundle, never use the plain-Ruby fallback, and ignore an
`.rt/Gemfile` and `RT_RUBY` in favor of the Rails application's Bundler runtime.
Inherited activation from an outer `bundle exec` is removed before entering the
application bundle, so its Gemfile and lockfile remain isolated.

On every path, rt strips `RUBYOPT` and `RUBYLIB` from the Ruby it launches, so a
value inherited from the surrounding shell (common under `bundle exec`) cannot
inject a require or a load path that breaks the harness. A deliberate `RUBYOPT`
of your own (say `--yjit`) is dropped too.

A task that [declares gems](#declaring-gems) is the one exception to Bundler
resolution: it always runs under plain Ruby (honoring `RT_RUBY`) in a fully
scrubbed environment (`BUNDLE_GEMFILE`, `GEM_HOME`, `GEM_PATH`, and every
`BUNDLE_*` variable removed on top of the `RUBYOPT`/`RUBYLIB` scrub above), so
`bundler/inline` resolves the declared gems into rt's isolated gem home without
fighting an active `bundle exec`.

## Caching

Discovered metadata is cached in `cache.json` next to the tasks
(`.rt/cache.json` in a project, `<config_dir>/cache.json` for global tasks),
keyed on each task file's size and modification time (seconds and nanoseconds)
plus the resolved Ruby command. Size is part of the key because some
filesystems only report one-second mtime resolution, where a same-second edit
could otherwise be missed. rt regenerates the cache when a task file changes,
the file set changes, or the Ruby command changes. rt writes a `.gitignore`
into the home — `.rt/` in a project, the config dir itself for global tasks —
with patterns anchored to cover only its generated files (cache and harness),
so `tasks/` stays versioned. Deleting the generated files is always safe.

## Migrating from the old layout

rt 0.0.2 and earlier read project tasks from a top-level `tasks/` directory,
with `rt.yml` as an optional root marker. To migrate a project:

1. Move the tasks: `mkdir -p .rt && git mv tasks .rt/tasks`. Delete `rt.yml`
   if present.
2. The old auto-generated `.rt/.gitignore` contained `*`; rt rewrites it with
   the new anchored patterns on the next run, so `.rt/tasks/` becomes visible
   to git.
3. In repositories that no longer use rt, delete any leftover `.rt/` directory
   (old cache and harness): its presence alone now marks the directory above
   it as an rt project.

Global tasks under `<config_dir>/tasks/` need no migration.

## Limitations

- Task files are loaded into a shared Ruby environment, so a helper defined in
  one file is visible to others. Keep helpers task-local if you need isolation.
- Tasks cannot read interactive input from stdin (`gets`); stdin is reserved
  for passing arguments to the harness.
- Windows is not supported.
