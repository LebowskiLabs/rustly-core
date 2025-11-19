# frozen_string_literal: true

require "rustly/core"
require "rspec/json_matcher"

RSpec.configure do |config|
  config.example_status_persistence_file_path = ".rspec_status"

  config.mock_with :rspec do |mocks|
    mocks.verify_partial_doubles = true
  end

  config.disable_monkey_patching!
  config.expose_dsl_globally = true

  config.expect_with :rspec do |c|
    c.syntax = :expect
  end

  config.include(RSpec::JsonMatcher)
end
