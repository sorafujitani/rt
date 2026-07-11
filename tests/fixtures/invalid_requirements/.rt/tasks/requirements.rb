task "unknown_requirement" do |t|
  t.requires :unknown
  t.run { }
end

task "duplicate_requirement" do |t|
  t.requires :rails, :rails
  t.run { }
end
