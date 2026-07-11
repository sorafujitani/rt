task "unknown_requirement" do |t|
  t.requires :unknown
  t.run { |_ctx| }
end

task "duplicate_requirement" do |t|
  t.requires :rails, :rails
  t.run { |_ctx| }
end
