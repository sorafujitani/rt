gem "rt_dummy"

desc "Use a locally-built dummy gem installed on demand"
task "use_dummy" do |ctx|
  require "rt_dummy"
  ctx.say "dummy #{RtDummy::VERSION}"
end
