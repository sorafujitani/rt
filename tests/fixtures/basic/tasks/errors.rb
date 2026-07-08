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
