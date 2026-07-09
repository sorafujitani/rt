desc "Write a partial (newline-less) stderr line, then raise"
task "boom_partial" do |_ctx|
  $stderr.write("partial-no-newline")
  raise "boom-partial"
end

desc "Write non-UTF-8 bytes to stderr, then raise"
task "boom_binary" do |_ctx|
  $stderr.binmode
  $stderr.write("\xff\xfe".dup.force_encoding("BINARY"))
  raise "boom-binary"
end

desc "Raise a ScriptError-family exception"
task "boom_scripterror" do |_ctx|
  raise NotImplementedError, "not yet"
end

desc "Return early on dry run"
task "early_return" do |ctx|
  ctx.say "starting"
  return if ctx.dry_run?

  ctx.say "did the work"
end

desc "Print a sentinel-shaped line to stderr but exit successfully"
task "fake_sentinel" do |_ctx|
  $stderr.puts("\x1e__RT_ERROR__ {\"class\":\"NotReal\",\"message\":\"decoy\"}")
end

desc "Write to both output streams"
task "both_streams" do |_ctx|
  $stdout.write("out")
  $stderr.write("err")
end

desc "Write non-UTF-8 bytes to stdout"
task "binary_stdout" do |_ctx|
  $stdout.binmode
  $stdout.write("\xff\xfe".dup.force_encoding("BINARY"))
end

desc "Write enough data to fill both process pipes"
task "large_streams" do |_ctx|
  threads = [
    Thread.new { $stdout.write("o" * 131_072) },
    Thread.new { $stderr.write("e" * 131_072) }
  ]
  threads.each(&:join)
end

desc "Use a task-owned --json option"
option :json, type: :boolean, default: false
task "owns_json" do |ctx|
  ctx.say ctx.option(:json).to_s
end
