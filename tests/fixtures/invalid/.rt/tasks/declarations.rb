task "duplicate_param" do |t|
  t.param :name
  t.param :name
  t.run { }
end

task "duplicate_option" do |t|
  t.option :force, :boolean
  t.option :force, :boolean
  t.run { }
end

task "param_option_collision" do |t|
  t.param :target
  t.option :target
  t.run { }
end

task "reserved_name" do |t|
  t.option :dry_run, :boolean
  t.run { }
end

task "reserved_output_name" do |t|
  t.option :output, String
  t.run { }
end

task "invalid_keyword_name" do |t|
  t.option :"bad-name", String
  t.run { }
end

task "unknown_option_type" do |t|
  t.option :count, :float
  t.run { }
end

task "invalid_option_default" do |t|
  t.option :count, Integer, default: "three"
  t.run { }
end

task "invalid_boolean_default" do |t|
  t.option :force, :boolean, default: "yes"
  t.run { }
end

task "invalid_param_default" do |t|
  t.param :count, default: 3
  t.run { }
end

task "invalid_enum_default" do |t|
  t.param :environment, enum: %w[staging production], default: "development"
  t.run { }
end

task "invalid_required_type" do |t|
  t.param :environment, required: "yes"
  t.run { }
end

task "required_param_with_default" do |t|
  t.param :environment, required: true, default: "production"
  t.run { }
end

task "invalid_range_type" do |t|
  t.option :name, String, in: 1..3
  t.run { }
end

task "invalid_range_bounds" do |t|
  t.option :count, Integer, in: 3...5
  t.run { }
end

task "default_outside_range" do |t|
  t.option :count, Integer, default: 5, in: 1..3
  t.run { }
end

task "missing_run" do |t|
  t.desc "Has no run block"
end

task "positional_run_argument" do |t|
  t.run { |_value| }
end

task "unknown_run_keyword" do |t|
  t.run { |missing:| }
end

task "healthy" do |t|
  t.desc "A valid task alongside invalid declarations"
  t.run do |output:|
    output.say "ok"
  end
end
