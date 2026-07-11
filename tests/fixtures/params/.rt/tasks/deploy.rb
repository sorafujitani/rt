task "deploy" do |t|
  t.desc "Deploy the application to an environment"
  t.param :environment, required: true, enum: %w[staging production],
                        description: "target environment"
  t.option :workers, Integer, default: 2, in: 1..16,
                     description: "worker count"
  t.option :force, :boolean, default: false, description: "skip safety checks"
  t.run do |environment:, workers:, force:, output:|
    output.say "deploying to #{environment} with #{workers} workers"
    output.say "force=#{force}"
  end
end
