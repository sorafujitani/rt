desc "Deploy the application to an environment"
param :environment, required: true, enum: %w[staging production],
                    description: "target environment"
option :workers, type: :integer, default: 2, description: "worker count"
option :force, type: :boolean, default: false, description: "skip safety checks"
task "deploy" do |ctx|
  ctx.say "deploying to #{ctx.param(:environment)} with #{ctx.option(:workers)} workers"
  ctx.say "force=#{ctx.option(:force)}"
end
