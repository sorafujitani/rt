task "ggreet" do |t|
  t.desc "A task available from anywhere on this machine"
  t.run do |output:|
    output.say "hello from global"
  end
end

task "greet" do |t|
  t.desc "A global greet that a project task of the same name should shadow"
  t.run do |output:|
    output.say "GLOBAL GREET"
  end
end
