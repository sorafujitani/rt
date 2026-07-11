task "rails_with_inline_gem" do |t|
  t.requires :rails
  t.run { |_ctx| }
end

gem "rake"
