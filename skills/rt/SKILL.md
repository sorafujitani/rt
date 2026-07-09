---
name: rt
description: Discover, run, and author rt tasks. rt turns Ruby scripts in a tasks/ directory into a CLI with help, validation, JSON metadata, and dry-run. Use when a repo contains a tasks/ directory or rt.yml, when asked to list or run rt tasks, or when asked to automate something with rt.
license: MIT
---

# rt

rt turns ordinary Ruby scripts into a discoverable command-line tool for humans and agents. Tasks are described in a small Ruby DSL with names, descriptions, typed params and options, and rt provides help, validation, machine-readable metadata, and a dry-run mode. It works in any repository (Go, TypeScript, anything) as long as a Ruby interpreter is available.

Detect rt in a repo by the presence of a `tasks/` directory or an `rt.yml` file. rt finds the project by walking up from the current directory; `RT_ROOT` overrides discovery.

## Discover and run tasks

Prefer the JSON commands. They print JSON on stdout and nothing else, with full type information for params and options. Load errors appear in the JSON `errors` array, not on stderr.

```bash
rt list --json          # all tasks: name, description, file, params, options, gems, source
rt help <task> --json   # usage metadata for one task
rt run <task> [args...] # run a task
```

Params are passed as positional arguments and options as flags:

```bash
rt run deploy production --workers 4 --force
```

Human-readable variants are `rt list` and `rt help <task>`. Every task accepts `--dry-run`, which sets `ctx.dry_run?` to true inside the task. Use it to preview a task with side effects before running it for real.

The exact JSON shapes, the full environment-variable table, and project layout details are in [reference.md](reference.md). Read it when you need the schema of `rt list --json` or an env var beyond `RT_ROOT`.

## Author tasks

Put task files in `tasks/` (loaded from `tasks/**/*.rb`). The DSL uses a pending-buffer model like Rake. `desc`, `param`, and `option` describe the next `task` declaration. Copy this template:

```ruby
# tasks/gh-release.rb
gem "octokit", "~> 8.0"

desc "Create a GitHub release"
param :tag, required: true, description: "tag to release"
option :draft, type: :boolean, default: false, description: "create as draft"
option :retries, type: :integer, default: 3, description: "API retry count"
task "gh:release" do |ctx|
  require "octokit"   # require INSIDE the block, never at the top level
  ctx.say "releasing #{ctx.param(:tag)} (draft: #{ctx.option(:draft)})"
  return if ctx.dry_run?
  # real work here
end
```

Rules:

- `param name, required:, default:, enum:, description:` is a positional argument. Command-line values always arrive as `String`; use a string default so the type is consistent. `enum` restricts accepted values.
- `option name, type:, default:, description:` is a `--flag`. `type` is one of `:string`, `:integer`, `:boolean`, and only options are coerced to that type. Booleans are set by presence (`--force`) or explicitly (`--force=false`).
- The context API is `ctx.param(:name)`, `ctx.option(:name)`, `ctx.dry_run?`, and `ctx.say(message)`. A bare `return` is a valid early exit.
- The task name is exactly what you declare. There is no automatic namespacing from file paths. Declaring the same name twice is an error.
- Tasks cannot read interactive input from stdin. Pass everything as params and options.

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

Trust caveat: the top level of every task file executes during discovery on every rt invocation (`list`, `help`, `run`), from any directory. Treat write access to `<config_dir>/tasks/` as code-execution access. Never write untrusted content there, and keep top-level code in task files to declarations only.

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
2. `bundle exec ruby` when a `Gemfile` is present and `bundle` is on `PATH`.
3. `ruby` on `PATH`.

If Bundler is missing or `bundle exec` fails, rt warns and falls back to plain `ruby`. Tasks that declare gems always run under plain Ruby regardless of a `Gemfile`. rt strips `RUBYOPT` and `RUBYLIB` from every Ruby it launches.
