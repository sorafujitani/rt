gem "rake"

desc "Use a gem declared inside the task file"
task "with_rake" do |ctx|
  require "rake"
  ctx.say "rake #{Rake::VERSION}"
end
