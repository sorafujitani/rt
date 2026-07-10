# frozen_string_literal: true

# Lightweight harness-contract fixture. The real Rails integration fixture is
# tests/fixtures/rails_real.

module Rails
  def self.env
    ENV.fetch("RAILS_ENV", "development")
  end
end

class User
  def self.count
    2
  end
end

if (marker = ENV["RAILS_BOOT_MARKER"])
  File.open(marker, "a") { |file| file.puts("boot") }
end
