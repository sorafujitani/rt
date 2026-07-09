# rt reference

## JSON schemas

`rt list --json`:

```json
{
  "protocol_version": 2,
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
        { "name": "workers", "type": "integer", "default": 2, "description": null },
        { "name": "force", "type": "boolean", "default": false, "description": null }
      ],
      "gems": [
        { "name": "octokit", "requirements": ["~> 8.0"] }
      ]
    }
  ],
  "errors": [
    { "file": "tasks/broken.rb", "class": "SyntaxError", "message": "...", "source": "project" }
  ]
}
```

`rt help <task> --json` returns `{ "protocol_version": 2, "task": { ...same task shape... } }`.

Option `type` is one of `string`, `integer`, `boolean`. Param values arrive in the task as strings regardless of the default's type.

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

task "gh:release" do |ctx|
  require "octokit"   # requires belong inside the task block
  # ...
end
```

- Gems install into `RT_GEM_HOME`'s per-Ruby-ABI subdirectory on first run. No sudo is needed and the user gem environment is untouched. Concurrent installs are serialized with a file lock.
- The gem environment for such tasks is closed: the declared gems plus Ruby's bundled/default gems are visible, a project Gemfile is not.
- Resolution failures (unknown gem, unreachable source, bad requirement) exit 74 deterministically.
- If the project has a `Gemfile`, tasks *without* gem declarations run under `bundle exec` and see the project's gems instead.

## Project layout

```
.rt/
  tasks/          # task files, discovered recursively (versioned)
  cache.json      # metadata cache (auto-generated, gitignored)
  harness-*.rb    # Ruby harness (auto-generated, gitignored)
```

Root discovery walks upward from the working directory until it finds a `.rt/` directory. The global config dir (`RT_CONFIG_DIR`, default `~/.config/rt`) has the same shape as `.rt/`: `tasks/`, `cache.json`, and the harness live directly under it. Without a project, global tasks still work.
