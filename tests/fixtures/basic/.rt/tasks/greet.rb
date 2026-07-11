task "greet" do |t|
  t.desc "Greet someone by name"
  t.option :name, type: :string, default: "world", description: "who to greet"
  t.run do |ctx|
    ctx.say "Hello, #{ctx.option(:name)}!"
  end
end

task "preview" do |t|
  t.desc "Preview whether this is a dry run"
  t.run do |ctx|
    if ctx.dry_run?
      ctx.say "would perform side effects (dry run)"
    else
      ctx.say "performing side effects"
    end
  end
end

task "boom" do |t|
  t.desc "Raise an exception"
  t.run do |_ctx|
    raise "kaboom"
  end
end

task "bail" do |t|
  t.desc "Exit with a custom status code"
  t.run do |_ctx|
    exit 3
  end
end
