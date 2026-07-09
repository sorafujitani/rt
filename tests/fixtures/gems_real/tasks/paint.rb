gem "paint", "~> 2.3"

desc "Colorize a word using the paint gem installed on demand"
task "paint_demo" do |ctx|
  require "paint"
  ctx.say Paint["colored", :red]
end
