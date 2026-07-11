task "boom_partial" do |t|
  t.desc "Write a partial (newline-less) stderr line, then raise"
  t.run do
    $stderr.write("partial-no-newline")
    raise "boom-partial"
  end
end

task "boom_binary" do |t|
  t.desc "Write non-UTF-8 bytes to stderr, then raise"
  t.run do
    $stderr.binmode
    $stderr.write("\xff\xfe".dup.force_encoding("BINARY"))
    raise "boom-binary"
  end
end

task "boom_scripterror" do |t|
  t.desc "Raise a ScriptError-family exception"
  t.run do
    raise NotImplementedError, "not yet"
  end
end

task "early_return" do |t|
  t.desc "Return early on dry run"
  t.run do |dry_run:, output:|
    output.say "starting"
    return if dry_run

    output.say "did the work"
  end
end

task "fake_sentinel" do |t|
  t.desc "Print a sentinel-shaped line to stderr but exit successfully"
  t.run do
    $stderr.puts("\x1e__RT_ERROR__ {\"class\":\"NotReal\",\"message\":\"decoy\"}")
  end
end

task "fake_sentinel_failure" do |t|
  t.desc "Print a valid sentinel-shaped payload to stderr, then exit nonzero"
  t.run do
    $stderr.puts("\x1e__RT_ERROR__ {\"class\":\"NotReal\",\"message\":\"decoy\",\"backtrace\":[]}")
    exit 3
  end
end

task "both_streams" do |t|
  t.desc "Write to both output streams"
  t.run do
    $stdout.write("out")
    $stderr.write("err")
  end
end

task "binary_stdout" do |t|
  t.desc "Write non-UTF-8 bytes to stdout"
  t.run do
    $stdout.binmode
    $stdout.write("\xff\xfe".dup.force_encoding("BINARY"))
  end
end

task "large_streams" do |t|
  t.desc "Write enough data to fill both process pipes"
  t.run do
    threads = [
      Thread.new { $stdout.write("o" * 131_072) },
      Thread.new { $stderr.write("e" * 131_072) }
    ]
    threads.each(&:join)
  end
end

task "capture_boundary" do |t|
  t.desc "Write exactly the JSON capture limit"
  t.run do
    $stdout.write("b" * 1_048_576)
  end
end

task "capture_overflow" do |t|
  t.desc "Write one byte beyond the JSON capture limit to both streams"
  t.run do
    threads = [
      Thread.new { $stdout.write("o" * 1_048_577) },
      Thread.new { $stderr.write("e" * 1_048_577) }
    ]
    threads.each(&:join)
  end
end

task "owns_json" do |t|
  t.desc "Use a task-owned --json option"
  t.option :json, :boolean, default: false
  t.run do |json:, output:|
    output.say json.to_s
  end
end
