task "deploy" do |t|
  t.desc "Deploy the application to an environment"
  t.param :environment, required: true, enum: %w[staging production],
                        description: "target environment"
  t.option :workers, type: :integer, default: 2, range: 1..16,
                     description: "worker count"
  t.option :force, type: :boolean, default: false, description: "skip safety checks"
  t.run do |ctx|
    ctx.say "deploying to #{ctx.param(:environment)} with #{ctx.option(:workers)} workers"
    ctx.say "force=#{ctx.option(:force)}"
  end
end
