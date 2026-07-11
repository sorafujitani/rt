gem "rake", "this is not a version"

task "bad_version" do |t|
  t.desc "Declares a malformed version requirement"
  t.run do |ctx|
    require "rake"
    ctx.say "unreachable"
  end
end
