desc "Exercise a Rails application task"
requires :rails
option :write, type: :boolean, default: false
task "rails:probe" do |ctx|
  ctx.say "env=#{Rails.env}"
  ctx.say "users=#{User.count}"
  ctx.say "root=#{ctx.project_root}"
  ctx.say "cwd=#{Dir.pwd}"

  return unless ctx.option(:write)
  return if ctx.dry_run?

  File.write(ctx.project_root.join("mutation.txt"), "written")
end

desc "Expose the project root without booting Rails"
task "root:show" do |ctx|
  ctx.say ctx.project_root.to_s
end
