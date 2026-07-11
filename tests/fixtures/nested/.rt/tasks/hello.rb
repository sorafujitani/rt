task "hello" do |t|
  t.desc "A task used to verify root discovery from a subdirectory"
  t.run do |output:|
    output.say "hi"
  end
end
