task "rails:probe" do |t|
  t.desc "Exercise a Rails application task"
  t.requires :rails
  t.option :write, :boolean, default: false
  t.option :limit, Integer, default: 5, in: 1..100
  t.run do |write:, limit:, dry_run:, output:, project_root:|
    raise "unexpected limit" unless limit == 5

    output.say "env=#{Rails.env}"
    output.say "users=#{User.count}"
    output.say "root=#{project_root}"
    output.say "cwd=#{Dir.pwd}"

    return unless write
    return if dry_run

    File.write(project_root.join("mutation.txt"), "written")
  end
end

task "root:show" do |t|
  t.desc "Expose the project root without booting Rails"
  t.run do |output:, project_root:|
    output.say project_root.to_s
  end
end
