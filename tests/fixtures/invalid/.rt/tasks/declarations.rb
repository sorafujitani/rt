param :name
param :name
task "duplicate_param" do |_ctx|
end

option :force, type: :boolean
option :force, type: :boolean
task "duplicate_option" do |_ctx|
end

param :target
option :target
task "param_option_collision" do |_ctx|
end

option :dry_run, type: :boolean
task "reserved_name" do |_ctx|
end

option :count, type: :float
task "unknown_option_type" do |_ctx|
end

option :count, type: :integer, default: "three"
task "invalid_option_default" do |_ctx|
end

option :force, type: :boolean, default: "yes"
task "invalid_boolean_default" do |_ctx|
end

param :count, default: 3
task "invalid_param_default" do |_ctx|
end

param :environment, enum: %w[staging production], default: "development"
task "invalid_enum_default" do |_ctx|
end

param :environment, required: "yes"
task "invalid_required_type" do |_ctx|
end

param :environment, required: true, default: "production"
task "required_param_with_default" do |_ctx|
end

desc "A valid task alongside invalid declarations"
task "healthy" do |ctx|
  ctx.say "ok"
end
