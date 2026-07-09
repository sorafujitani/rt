# rt

rt turns ordinary Ruby scripts into a discoverable command-line tool that both
humans and agents can use. You describe tasks in a small Ruby DSL — name,
description, typed params and options — and rt gives them automatic help,
validation, machine-readable metadata, and a dry-run mode.

rt is not tied to Ruby projects. As long as a Ruby interpreter is on the
machine, rt works in Go, TypeScript, or any other repository — or in no
repository at all. Task definitions live in a `tasks/` directory and are plain
Ruby using only the standard library.

## Requirements

- A Ruby interpreter (`ruby`) on `PATH`, or `RT_RUBY` pointing at one.
- macOS or Linux. Windows is not supported.

## Install

Via Homebrew (the repository doubles as a tap; no releases yet, so install from HEAD):

```
brew tap sorafujitani/rt https://github.com/sorafujitani/rt
brew install --HEAD sorafujitani/rt/rt
```

Or with cargo, from a checkout:

```
cargo install --path .
```

## Getting started

Create a `tasks/` directory in your project and add a task file:

```ruby
# tasks/greet.rb
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
`tasks/` directory (or an `rt.yml` file). Set `RT_ROOT` to override discovery.

## Writing tasks

Task files are loaded from `tasks/**/*.rb`. The DSL uses a pending-buffer model
like Rake: `desc`, `param`, and `option` describe the next `task`.

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
  command line always reaches the task as a `String`; prefer a string `default`
  too, so the type is consistent whether or not the argument was given.
- `option name, type:, default:, description:` — a `--flag`. `type` is one of
  `:string`, `:integer`, `:boolean`. Boolean options are set by presence
  (`--force`) or explicitly (`--force=false`). Only options carry a `type` and
  are coerced accordingly (integers become integers, booleans become booleans).
- The block receives a context: `ctx.param(:name)`, `ctx.option(:name)`,
  `ctx.dry_run?`, and `ctx.say(message)` for output. A bare `return` inside a
  task body is a valid early exit.

The task name is exactly what you declare; there is no automatic namespacing
from file paths. Declaring the same name twice is reported as an error.

`--dry-run` is available for every task and sets `ctx.dry_run?` to true.

### Declaring gems

A task file may declare gems it needs with a top-level `gem` line. rt resolves
them with `bundler/inline` just before the task runs, so a task can depend on a
gem without the project having a `Gemfile`.

```ruby
# tasks/gh-release.rb
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

## Commands

- `rt list` — list tasks with descriptions.
- `rt help <task>` — show usage for one task.
- `rt run <task> [args...]` — run a task.

### Machine-readable output

`rt list --json` and `rt help <task> --json` print JSON on stdout and nothing
else. The schema keeps full type information for params and options so it can
be converted to a JSON Schema or an MCP tool definition. Load errors are
reported in the JSON `errors` array rather than on stderr.

## Global tasks

Besides a project's `tasks/`, rt also loads machine-wide tasks from a config
directory, so you can carry personal tasks across every repository — or use rt
with no project at all. Put task files in `<config_dir>/tasks/`, where
`config_dir` is resolved in this order:

1. `RT_CONFIG_DIR`, if set.
2. `$XDG_CONFIG_HOME/rt`, if `XDG_CONFIG_HOME` is set.
3. `~/.config/rt`.

Global tasks work the same as project tasks, and get their own cache and
harness under `<config_dir>/.rt/`.

A word on trust: the top level of every task file runs during discovery, on
*every* rt invocation (`list`, `help`, and `run`) from any directory. Because
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
   and `bundle` is on `PATH`.
3. `ruby` on `PATH`.

If a `Gemfile` is present but `bundle` is not installed, rt warns and falls
back to plain `ruby`. If `bundle exec` is installed but fails (for example when
the bundle's gems are not installed), rt warns and retries discovery once with
plain `ruby`. The plain-Ruby path is the primary one; Bundler is only an
optimization for projects that already use it.

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

Discovered metadata is cached in `.rt/cache.json`, keyed on each task file's
size and modification time (seconds and nanoseconds) plus the resolved Ruby
command. Size is part of the key because some filesystems only report
one-second mtime resolution, where a same-second edit could otherwise be
missed. rt regenerates the cache when a task file changes, the file set
changes, or the Ruby command changes. The `.rt/` directory is git-ignored
automatically. Deleting it is always safe.

## Limitations

- Task files are loaded into a shared Ruby environment, so a helper defined in
  one file is visible to others. Keep helpers task-local if you need isolation.
- Tasks cannot read interactive input from stdin (`gets`); stdin is reserved
  for passing arguments to the harness.
- Windows is not supported.
