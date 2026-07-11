task "rails:probe" do |t|
  t.desc "Exercise a Rails application task"
  t.requires :rails
  t.option :write, type: :boolean, default: false
  t.run do |ctx|
    ctx.say "env=#{Rails.env}"
    ctx.say "users=#{User.count}"
    ctx.say "root=#{ctx.project_root}"
    ctx.say "cwd=#{Dir.pwd}"

    return unless ctx.option(:write)
    return if ctx.dry_run?

    File.write(ctx.project_root.join("mutation.txt"), "written")
  end
end

task "root:show" do |t|
  t.desc "Expose the project root without booting Rails"
  t.run do |ctx|
    ctx.say ctx.project_root.to_s
  end
end
