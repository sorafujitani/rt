gem "rt_nonexistent_toplevel_gem"
require "rt_nonexistent_toplevel_gem"

desc "Requires its declared gem at the top level, which is wrong"
task "bad_require" do |ctx|
  ctx.say "unreachable"
end
