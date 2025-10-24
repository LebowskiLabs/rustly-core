# Contributing Guide

Thank you for checking out Rustly! Follow the guidelines below to keep reviews fast and releases
predictable.

## Prerequisites

- Ruby 3.2–3.4 (we recommend installing via rbenv or ruby-install).
- Rust 1.85.0 (`rustup default 1.85.0`).
- Bundler 2.5+ (`gem install bundler`).

## Workflow

1. Fork the repository and create a branch such as `feature/<topic>`.
2. Run `bundle exec rake lint`, `bundle exec rspec`, and `cargo test --manifest-path Cargo.toml` before
   opening a pull request.
3. Keep commits focused and use descriptive messages. Update `CHANGELOG.md` under the relevant
   heading when you change public behaviour.

## Coding Style

- Ruby: RuboCop configuration (`.rubocop.yml`), double quotes, 120-character lines.
- Rust: `rustfmt` with `rustfmt.toml`, warnings treated as errors by `clippy`.

## Documentation & Tests

- Update README examples and docs whenever you change the public API.
- Cover new behaviour with RSpec specs or Rust unit tests.

## Communication

- Follow the [Code of Conduct](./CODE_OF_CONDUCT.md).
- For questions or discussions use GitHub Discussions or Issues.

Thanks for contributing!
