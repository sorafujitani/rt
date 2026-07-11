# rt reference

## JSON schemas

`rt list --json`:

```json
{
  "protocol_version": 4,
  "tasks": [
    {
      "name": "deploy",
      "description": "Deploy the app",
      "file": "tasks/deploy.rb",
      "source": "project",
      "params": [
        { "name": "environment", "required": true, "default": null,
          "enum": ["staging", "production"], "description": null }
      ],
      "options": [
        { "name": "workers", "type": "integer", "default": 2,
          "minimum": 1, "maximum": 16, "description": null },
        { "name": "force", "type": "boolean", "default": false, "description": null }
      ],
      "gems": [
        { "name": "octokit", "requirements": ["~> 8.0"] }
      ],
      "requirements": []
    }
  ],
  "errors": [
    { "file": "tasks/broken.rb", "class": "SyntaxError", "message": "...", "source": "project" }
  ]
}
```

`rt help <task> --json` returns `{ "protocol_version": 4, "task": { ...same task shape... } }`.

Option `type` is one of `string`, `integer`, `boolean`. Param values arrive in the task as strings regardless of the default's type.
`protocol_version` versions this public metadata schema; it is independent of
rt's private Ruby harness protocol and on-disk cache format.

`rt tools --json [task]`:

```json
{
  "schema_version": 3,
  "tools": [
    {
      "task": "deploy",
      "description": "Deploy the application to an environment",
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
          "environment": {
            "type": "string",
            "description": "target environment",
            "enum": ["staging", "production"]
          },
          "force": {
            "type": "boolean",
            "description": "skip safety checks",
            "default": false
          },
          "workers": {
            "type": "integer",
            "description": "worker count",
            "default": 2,
            "minimum": 1,
            "maximum": 16
          }
        },
        "required": ["environment"],
        "additionalProperties": false
      }
    }
  ],
  "errors": []
}
```

`schema_version` versions the tool catalog independently from metadata and run
results. `task`, `source`, and `requirements` preserve the merged metadata values. Properties
combine params, options, and `dry_run` into one object namespace. Params are
strings; option types remain `string`, `integer`, or `boolean`. Required params
appear in `required`. Null defaults are omitted, while a boolean with no
declared default has the effective default `false`. Integer ranges become
JSON Schema `minimum` and `maximum`. Every input schema sets
`additionalProperties` to `false`.

Without `[task]`, `tools` contains every merged project/global task. With a
task filter, the top-level shape and load `errors` remain unchanged. An unknown
task is a usage error with exit code 2. The command requires `--json`. Catalog
`errors` use the same load-error shape shown for `list --json`, including
`ShadowedTask` entries from project/global name collisions.

`rt run --json <task> [args...]`:

```json
{
  "schema_version": 2,
  "task": "deploy",
  "status": "error",
  "exit_code": 1,
  "stdout": {
    "encoding": "utf-8",
    "data": "starting\n",
    "total_bytes": 9,
    "captured_bytes": 9,
    "truncated": false
  },
  "stderr": {
    "encoding": "utf-8",
    "data": "",
    "total_bytes": 0,
    "captured_bytes": 0,
    "truncated": false
  },
  "error": {
    "kind": "task_exception",
    "class": "RuntimeError",
    "message": "deployment failed",
    "backtrace": ["tasks/deploy.rb:12:in `block in <top (required)>'"]
  },
  "load_errors": []
}
```

`status` is `success` or `error`. `error.kind` is one of `usage`,
`task_exception`, `task_exit`, `environment`, or `internal`. A successful result
has `error: null`. Each output object retains at most the first 1,048,576 raw
bytes. `total_bytes` counts the full drained stream, `captured_bytes` counts the
retained raw bytes, and `truncated` is true when they differ. Output objects use
`encoding: "utf-8"` when the captured bytes are valid UTF-8 and
`encoding: "base64"` otherwise. JSON mode writes the result
only to stdout and preserves the normal process exit code.

The rt-level `--json` flag may appear before or after the task name. To pass a
task-owned option named `--json`, separate task arguments with `--`, for example
`rt run --json deploy -- --json`.

## Exit codes

| exit | meaning |
|---|---|
| 0 | success |
| 1 | task raised an exception (formatted class/message/backtrace on stderr) |
| 2 | usage error: unknown task, missing required param, enum violation, bad option value |
| 70 | rt internal error (harness failure, metadata corruption) |
| 74 | environment error: Ruby not found, gem resolution/installation failed |
| n | the task itself called `exit n` (passed through) |

## Environment variables

| variable | effect |
|---|---|
| `RT_ROOT` | skip upward project discovery, use this directory (which must contain `.rt/`) as the project root |
| `RT_RUBY` | path to a single Ruby executable (no shell strings like `"bundle exec ruby"`) |
| `RT_CONFIG_DIR` | global tasks location (default `$XDG_CONFIG_HOME/rt`, then `~/.config/rt`) |
| `RT_GEM_HOME` | base dir for the isolated inline-gem cache (default `$XDG_CACHE_HOME/rt/gems`, then `~/.cache/rt/gems`) |
| `RT_GEM_SOURCE` | gem source URL for inline-gem resolution (default `https://rubygems.org`) |

## Inline gems

A task file can declare gems at the top level:

```ruby
gem "octokit", "~> 8.0"

task "gh:release" do |t|
  t.run do |ctx|
    require "octokit"   # requires belong inside the run block
    # ...
  end
end
```

- Gems install into `RT_GEM_HOME`'s per-Ruby-ABI subdirectory on first run. No sudo is needed and the user gem environment is untouched. Concurrent installs are serialized with a file lock.
- The gem environment for such tasks is closed: the declared gems plus Ruby's bundled/default gems are visible, a project Gemfile is not.
- Resolution failures (unknown gem, unreachable source, bad requirement) exit 74 deterministically.
- If the project has a `Gemfile`, tasks *without* gem declarations run under `bundle exec` and see the project's gems instead.

## Rails applications

- `t.requires :rails` is task-scoped and appears as `"requirements": ["rails"]` in metadata and tool definitions.
- Discovery never inspects the project-root `Gemfile` or loads the Rails application. Execution loads the project-root `config/environment.rb` immediately before the task block.
- A Rails task requires the project-root `Gemfile`, Bundler, and a complete bundle. It never falls back to plain Ruby, uses the application's Bundler runtime instead of `RT_RUBY`, and removes activation state inherited from an outer `bundle exec`.
- Rails tasks run with the project root as the working directory and receive it as a `Pathname` through `ctx.project_root`.
- Rails tasks cannot be global or share a file with inline `gem` declarations.
- Rails boot failures use exit 74 and JSON `error.kind: "environment"`, preserving the exception class, message, and backtrace.

## Project layout

```
.rt/
  tasks/          # task files, discovered recursively (versioned)
  cache.json      # metadata cache (auto-generated, gitignored)
  harness-*.rb    # Ruby harness (auto-generated, gitignored)
```

Root discovery walks upward from the working directory until it finds a `.rt/` directory. The global config dir (`RT_CONFIG_DIR`, default `~/.config/rt`) has the same shape as `.rt/`: `tasks/`, `cache.json`, and the harness live directly under it. Without a project, global tasks still work.
