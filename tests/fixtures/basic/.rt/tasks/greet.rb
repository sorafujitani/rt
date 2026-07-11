task "greet" do |t|
  t.desc "Greet someone by name"
  t.option :name, String, default: "world", description: "who to greet"
  t.run do |name:, output:|
    output.say "Hello, #{name}!"
  end
end

task "preview" do |t|
  t.desc "Preview whether this is a dry run"
  t.run do |dry_run:, output:|
    if dry_run
      output.say "would perform side effects (dry run)"
      next
    end

    output.say "performing side effects"
  end
end

task "boom" do |t|
  t.desc "Raise an exception"
  t.run do
    raise "kaboom"
  end
end

task "bail" do |t|
  t.desc "Exit with a custom status code"
  t.run do
    exit 3
  end
end
