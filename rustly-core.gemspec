# frozen_string_literal: true

require_relative "lib/rustly/core/version"

Gem::Specification.new do |spec|
  spec.name = "rustly-core"
  spec.version = Rustly::Core::VERSION
  spec.authors = ["dude"]
  spec.email = ["skuf@justregulardude.ru"]

  spec.summary = "Rust-backed validation core for Rustly models."
  spec.description = "Rust runtime compiling Rustly schemas with coercion and validation sans GVL."
  spec.homepage = "https://github.com/rustly/rustly-core"
  spec.license = "MIT"
  spec.required_ruby_version = Gem::Requirement.new(">= 3.2", "< 3.5")

  spec.metadata["homepage_uri"] = spec.homepage
  spec.metadata["source_code_uri"] = spec.homepage
  spec.metadata["changelog_uri"] = "https://github.com/rustly/rustly-core/blob/main/CHANGELOG.md"
  spec.metadata["rubygems_mfa_required"] = "true"
  spec.metadata["cargo_crate_name"] = "rustly-core"

  # Specify which files should be added to the gem when it is released.
  # The `git ls-files -z` loads the files in the RubyGem that have been added into git.
  gemspec = File.basename(__FILE__)
  spec.files = IO.popen(%w[git ls-files -z], chdir: __dir__, err: IO::NULL) do |ls|
    ls.readlines("\x0", chomp: true).reject do |f|
      (f == gemspec) ||
        f.start_with?(*%w[bin/ Gemfile .rspec spec/ .github/])
    end
  end
  spec.bindir = "exe"
  spec.executables = spec.files.grep(%r{\Aexe/}) { |f| File.basename(f) }
  spec.require_paths = ["lib"]
  spec.extensions = ["ext/rustly_core/extconf.rb"]

  spec.add_dependency "rb_sys", ">= 0.9.117", "< 0.10"
end
