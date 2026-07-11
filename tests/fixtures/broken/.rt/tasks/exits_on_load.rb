task "never_registered" do |t|
  t.desc "This task file exits at load time"
  t.run do |output:|
    output.say "unreachable"
  end
end

exit 5
