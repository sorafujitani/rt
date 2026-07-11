task "no_gems" do |t|
  t.desc "A task with no declared gems"
  t.run do |output:|
    output.say "plain"
  end
end
