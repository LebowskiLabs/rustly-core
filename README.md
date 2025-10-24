# Rustly::Core

Rustly::Core is the native core for the Rustly project. It compiles model schemas, performs
validation and coercion, and materialises Ruby objects while minimising Ruby ↔ Rust transitions.

## Features

- **Schema compilation** – transforms the Ruby AST into a compact `CompiledSchema` typed object.
- **Validation & coercion** – runs heavy work in Rust without the GVL and returns an `ErrorSet` on failure.
- **Materialisation** – builds frozen Ruby instances and preserves the original attributes for debugging.
- **Ractor safety** – typed data objects avoid storing Ruby values inside Rust heaps, enabling safe reuse.

## Requirements

- Ruby 3.2 – 3.4 (CI covers Linux and macOS).
- Rust 1.85 (minimum supported Rust version).
- Bundler 2.5 or newer.

## Quick Start

```bash
bundle install
bundle exec rake compile
bundle exec rspec
cargo test --manifest-path Cargo.toml
```

### Usage Example

```ruby
require "rustly/core"

schema_ast = {
  type: :struct,
  fields: [[:required, :email, :string, { format: :email }]],
  extra: :forbid
}

compiled = Rustly::Core.compile(schema_ast)
ok, result = Rustly::Core.build(compiled, { email: "skuf@justregulardude.ru" }, {}, Struct.new(:email))

if ok
  puts "Materialized: #{result.inspect}"
else
  warn "Validation errors: #{result.messages.join(", ")}"
end
```

`Rustly::Core.build` returns `[Boolean, Object | ErrorSet]`. The happy path freezes the materialised
object and stores the original input in `@attributes`.

## Project Layout

- `ext/rustly_core` – Rust sources (`cdylib`) and Cargo configuration.
- `lib/rustly` – Ruby wrapper and helpers (`DEFAULT_BUILD_OPTIONS`, normalised options).
- `spec/` – RSpec smoke tests.
- `.github/workflows/ci.yml` – CI for Linux/macOS, Ruby 3.2–3.4, `rustfmt`, `clippy`, and `cargo-deny`.

## Tooling

```bash
bundle exec rake compile
bundle exec rspec
cargo test --manifest-path Cargo.toml
cargo fmt --manifest-path Cargo.toml -- --check
cargo clippy --manifest-path Cargo.toml --all-targets -- -D warnings
bundle exec rubocop
cargo deny check --manifest-path Cargo.toml
```

`bundle exec rake lint` runs RuboCop together with `cargo fmt` and `cargo clippy`.

## Documentation

- [CHANGELOG.md](./CHANGELOG.md)
- [CONTRIBUTING.md](./CONTRIBUTING.md)
- [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md)

## License

Rustly::Core is released under the [MIT License](./LICENSE.txt).
