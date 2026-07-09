class Rt < Formula
  desc "Run Ruby-defined tasks from a discoverable CLI"
  homepage "https://github.com/sorafujitani/rt"
  url "https://github.com/sorafujitani/rt/archive/refs/tags/v0.0.1.tar.gz"
  sha256 "2e72a49f2cbe5945fb3d5bac690e525783f3a1215ffc9cfdd287a2622a091f30"
  license "MIT"
  head "https://github.com/sorafujitani/rt.git", branch: "main"

  depends_on "rust" => :build

  uses_from_macos "ruby"

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    (testpath/"tasks/greet.rb").write <<~RUBY
      desc "Greet someone by name"
      option :name, type: :string, default: "world", description: "who to greet"
      task "greet" do |ctx|
        ctx.say "Hello, \#{ctx.option(:name)}!"
      end
    RUBY

    assert_match "Hello, brew!", shell_output("#{bin}/rt run greet --name brew")
    assert_match "greet", shell_output("#{bin}/rt list")
  end
end
