task "users:smoke" do |t|
  t.desc "Create and count a user through Rails"
  t.requires :rails
  t.run do |ctx|
    require "fileutils"
    FileUtils.mkdir_p(ctx.project_root.join("storage"))

    ActiveRecord::Schema.define do
      create_table :users, force: true do |table|
        table.string :name, null: false
      end
    end
    User.create!(name: "sora")
    ctx.say "rails=#{Rails.version} env=#{Rails.env} users=#{User.count}"
  end
end
