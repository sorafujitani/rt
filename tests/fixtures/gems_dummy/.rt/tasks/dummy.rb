gem "rt_dummy"

task "use_dummy" do |t|
  t.desc "Use a locally-built dummy gem installed on demand"
  t.run do |output:|
    require "rt_dummy"
    output.say "dummy #{RtDummy::VERSION}"
  end
end
