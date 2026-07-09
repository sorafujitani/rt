desc "Greet someone by name"
option :name, type: :string, default: "world", description: "who to greet"
task "greet" do |ctx|
  ctx.say "Hello, #{ctx.option(:name)}!"
end
