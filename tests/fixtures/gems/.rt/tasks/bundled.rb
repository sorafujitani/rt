gem "rake"

task "with_rake" do |t|
  t.desc "Use a gem declared inside the task file"
  t.run do |ctx|
    require "rake"
    ctx.say "rake #{Rake::VERSION}"
  end
end
