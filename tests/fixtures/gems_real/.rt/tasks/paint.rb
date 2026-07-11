gem "paint", "~> 2.3"

task "paint_demo" do |t|
  t.desc "Colorize a word using the paint gem installed on demand"
  t.run do |output:|
    require "paint"
    output.say Paint["colored", :red]
  end
end
