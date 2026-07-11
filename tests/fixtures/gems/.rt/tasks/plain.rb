task "no_gems" do |t|
  t.desc "A task with no declared gems"
  t.run do |ctx|
    ctx.say "plain"
  end
end
