desc "Greet someone by name"
option :name, type: :string, default: "world", description: "who to greet"
task "greet" do |ctx|
  ctx.say "Hello, #{ctx.option(:name)}!"
end

desc "Preview whether this is a dry run"
task "preview" do |ctx|
  if ctx.dry_run?
    ctx.say "would perform side effects (dry run)"
  else
    ctx.say "performing side effects"
  end
end

desc "Raise an exception"
task "boom" do |_ctx|
  raise "kaboom"
end

desc "Exit with a custom status code"
task "bail" do |_ctx|
  exit 3
end
