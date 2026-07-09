gem "rake", "this is not a version"

desc "Declares a malformed version requirement"
task "bad_version" do |ctx|
  require "rake"
  ctx.say "unreachable"
end
