gem "rt_nonexistent_toplevel_gem"
require "rt_nonexistent_toplevel_gem"

task "bad_require" do |t|
  t.desc "Requires its declared gem at the top level, which is wrong"
  t.run do |output:|
    output.say "unreachable"
  end
end
