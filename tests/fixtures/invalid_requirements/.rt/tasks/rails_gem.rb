task "rails_with_inline_gem" do |t|
  t.requires :rails
  t.run { }
end

gem "rake"
