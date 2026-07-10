#!/usr/bin/env bash
set -euo pipefail

binary="${1:-target/debug/rt}"
binary=$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/project/.rt/tasks" "$root/config"

cat >"$root/project/.rt/tasks/greet.rb" <<'RUBY'
desc "Greet someone by name"
option :name, type: :string, default: "world", description: "who to greet"
task "greet" do |ctx|
  ctx.say "Hello, #{ctx.option(:name)}!"
end
RUBY

run_json() {
  local output="$1"
  shift
  (
    cd "$root/project"
    RT_CONFIG_DIR="$root/config" "$binary" "$@"
  ) >"$output"
  ruby -rjson -e 'JSON.parse(File.read(ARGV.fetch(0)))' "$output"
}

run_json "$root/list.json" list --json
run_json "$root/help.json" help greet --json
run_json "$root/tools.json" tools --json greet
run_json "$root/run.json" run --json greet --name release

ruby -rjson -e '
  list = JSON.parse(File.read(ARGV.fetch(0)))
  help = JSON.parse(File.read(ARGV.fetch(1)))
  tools = JSON.parse(File.read(ARGV.fetch(2)))
  run = JSON.parse(File.read(ARGV.fetch(3)))
  abort "unexpected list schema" unless list["protocol_version"] == 3 && list["tasks"][0]["name"] == "greet"
  abort "unexpected help schema" unless help["protocol_version"] == 3 && help["task"]["name"] == "greet"
  abort "unexpected tools schema" unless tools["schema_version"] == 2 && tools["tools"][0]["task"] == "greet"
  abort "unexpected run result" unless run["schema_version"] == 2 && run["status"] == "success" && run["stdout"]["data"] == "Hello, release!\n"
' "$root/list.json" "$root/help.json" "$root/tools.json" "$root/run.json"

echo "release smoke passed: list/help/tools/run JSON"
