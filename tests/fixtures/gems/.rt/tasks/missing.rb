gem "rt_definitely_nonexistent_gem_xyz"

task "needs_missing" do |t|
  t.desc "Declares a gem that cannot be resolved"
  t.run do |ctx|
    require "rt_definitely_nonexistent_gem_xyz"
    ctx.say "unreachable"
  end
end
