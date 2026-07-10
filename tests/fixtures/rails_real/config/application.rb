require_relative "boot"
require "rails/all"

module RtRailsFixture
  class Application < Rails::Application
    config.load_defaults 8.1
    config.eager_load = false
    config.secret_key_base = "rt-rails-fixture"
  end
end
