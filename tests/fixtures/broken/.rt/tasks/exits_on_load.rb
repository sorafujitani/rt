task "never_registered" do |t|
  t.desc "This task file exits at load time"
  t.run do |ctx|
    ctx.say "unreachable"
  end
end

exit 5
