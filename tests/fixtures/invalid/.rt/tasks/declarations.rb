task "duplicate_param" do |t|
  t.param :name
  t.param :name
  t.run { |_ctx| }
end

task "duplicate_option" do |t|
  t.option :force, type: :boolean
  t.option :force, type: :boolean
  t.run { |_ctx| }
end

task "param_option_collision" do |t|
  t.param :target
  t.option :target
  t.run { |_ctx| }
end

task "reserved_name" do |t|
  t.option :dry_run, type: :boolean
  t.run { |_ctx| }
end

task "unknown_option_type" do |t|
  t.option :count, type: :float
  t.run { |_ctx| }
end

task "invalid_option_default" do |t|
  t.option :count, type: :integer, default: "three"
  t.run { |_ctx| }
end

task "invalid_boolean_default" do |t|
  t.option :force, type: :boolean, default: "yes"
  t.run { |_ctx| }
end

task "invalid_param_default" do |t|
  t.param :count, default: 3
  t.run { |_ctx| }
end

task "invalid_enum_default" do |t|
  t.param :environment, enum: %w[staging production], default: "development"
  t.run { |_ctx| }
end

task "invalid_required_type" do |t|
  t.param :environment, required: "yes"
  t.run { |_ctx| }
end

task "required_param_with_default" do |t|
  t.param :environment, required: true, default: "production"
  t.run { |_ctx| }
end

task "invalid_range_type" do |t|
  t.option :name, type: :string, range: 1..3
  t.run { |_ctx| }
end

task "invalid_range_bounds" do |t|
  t.option :count, type: :integer, range: 3...5
  t.run { |_ctx| }
end

task "default_outside_range" do |t|
  t.option :count, type: :integer, default: 5, range: 1..3
  t.run { |_ctx| }
end

task "missing_run" do |t|
  t.desc "Has no run block"
end

task "healthy" do |t|
  t.desc "A valid task alongside invalid declarations"
  t.run do |ctx|
    ctx.say "ok"
  end
end
