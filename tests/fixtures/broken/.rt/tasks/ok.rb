task "healthy" do |t|
  t.desc "A healthy task alongside a broken one"
  t.run do |output:|
    output.say "ok"
  end
end
