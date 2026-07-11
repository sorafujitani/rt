task "healthy" do |t|
  t.desc "A healthy task alongside a broken one"
  t.run do |ctx|
    ctx.say "ok"
  end
end
