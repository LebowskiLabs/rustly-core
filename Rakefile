# frozen_string_literal: true

require "bundler/gem_tasks"
require "rspec/core/rake_task"
require "rb_sys/extensiontask"

SPEC = Gem::Specification.load("rustly-core.gemspec")

RbSys::ExtensionTask.new("rustly_core", SPEC) do |ext|
  ext.lib_dir = "lib/rustly_core"
end

RSpec::Core::RakeTask.new(:spec)

task default: %i[compile spec]

desc "Run cargo fmt --check"
task :"cargo:fmt" do
  sh "cargo fmt --manifest-path Cargo.toml -- --check"
end

desc "Run cargo clippy with warnings as errors"
task :"cargo:clippy" do
  sh "cargo clippy --manifest-path Cargo.toml --all-targets -- -D warnings"
end

desc "Run cargo test"
task :"cargo:test" do
  sh "cargo test --manifest-path Cargo.toml"
end

desc "Run bundle exec rubocop"
task :rubocop do
  sh "bundle exec rubocop"
end

desc "Run all linters"
task lint: ["cargo:fmt", "cargo:clippy", :rubocop]
