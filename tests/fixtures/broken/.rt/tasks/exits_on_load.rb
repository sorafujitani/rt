desc "This task file exits at load time"
task "never_registered" do |ctx|
  ctx.say "unreachable"
end

exit 5
