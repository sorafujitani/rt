gem "rt_definitely_nonexistent_gem_xyz"

desc "Declares a gem that cannot be resolved"
task "needs_missing" do |ctx|
  require "rt_definitely_nonexistent_gem_xyz"
  ctx.say "unreachable"
end
