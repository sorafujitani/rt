desc "A task available from anywhere on this machine"
task "ggreet" do |ctx|
  ctx.say "hello from global"
end

desc "A global greet that a project task of the same name should shadow"
task "greet" do |ctx|
  ctx.say "GLOBAL GREET"
end
