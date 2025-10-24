# Rustly::Core

<p align="center">
  <img src="docs/assets/rustly-core-logo.png" width="80", align="left"><br>
</p>

Rustly::Core is the performance-heavy foundation of the Rustly stack.
It compiles model schemas, validates payloads outside the GVL,
and materialises Ruby objects with minimal crossing overhead.

<br>

## Features

- **Schema compilation** – crushes the Ruby AST into a compact `CompiledSchema` typed object.
- **Validation & coercion** – launches heavy lifting in Rust without the GVL and returns an `ErrorSet` on failure.
- **Materialisation** – materialises Ruby structs directly; `@attributes` is only built when
  `store_attributes: true`.
- **Ractor safety** – typed data objects avoid storing Ruby values inside Rust heaps, so you can run flat-out across threads.

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
instance = Rustly::Core.build(compiled, { email: "skuf@justregulardude.ru" }, Struct.new(:email))
puts "Materialized: #{instance.inspect}"
```

`Rustly::Core.build` returns the materialized instance on success and raises `Rustly::Core::ValidationError`
when validation fails. The raised exception exposes an `ErrorSet` via `#errors`. By default the object is
left unfrozen (`freeze: :none`) and does not allocate `@attributes`; enable the relevant switches when you
need them.

### Compile Options

`Rustly::Core.compile` merges user options with `Rustly::Core::DEFAULT_OPTIONS`. Key switches:

- `strict` (default: `false`) — enforce strict coercions.
- `extra` — extras policy (`:forbid`, `:ignore`, `:allow`).
- `input_mode` — `:auto`, `:ruby`, or `:json`.
- `freeze` (default: `:none`) — choose between `:none`, `:shallow`, `:deep`.
- `store_attributes` (default: `false`) — capture the raw payload in `@attributes`.

## Project Layout

- `ext/rustly_core` – Rust sources (`cdylib`) and Cargo configuration.
- `lib/rustly` – Ruby wrapper and helpers (`DEFAULT_OPTIONS`, normalised options).
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

## Benchmarks

### Environment

- Date: October 24, 2025
- Ruby: 3.4.7 (2025-10-08) +PRISM [x86_64-linux]
- OS: Linux 6.14.0-34-generic
- Commands: `bin/compare_models`, `bundle exec ruby bench/throughput.rb --count 10000 --rounds 5`

### Model Construction (`bin/compare_models`)

- Dataset: 100,000 payloads that follow the `SCHEMA` constant in `bin/compare_models` (symbol keys, frozen tag arrays).
- Rustly options: `freeze: :none`, `store_attributes: false`.
- Memory allocations ([benchmark-memory](https://github.com/michaelherold/benchmark-memory), single threaded):

| Implementation | Allocated Memory | Objects Allocated | Strings Allocated |
| --- | --- | --- | --- |
| Rustly::Core | 28.000 MB | 500,000 | 50 |
| [Dry::Struct](https://github.com/dry-rb/dry-struct) | 24.006 MB | 300,067 | 35 |
| [SmartCore::ValueObject](https://github.com/smart-rb/smart_value-object) | 216.000 MB | 3,900,000 | 5 |

- Single-thread throughput (Benchmark.bmbm, real time):

| Implementation | Real Time (s) |
| --- | --- |
| Rustly::Core | 0.123 |
| [Dry::Struct](https://github.com/dry-rb/dry-struct) | 0.437 |
| [SmartCore::ValueObject](https://github.com/smart-rb/smart_value-object) | 1.952 |

- Parallel throughput with 10 threads ([Parallel](https://github.com/grosser/parallel) + Benchmark.bmbm, real time):

| Implementation | Real Time (s) |
| --- | --- |
| Rustly::Core (10 threads) | 0.236 |
| [Dry::Struct](https://github.com/dry-rb/dry-struct) (10 threads) | 0.474 |
| [SmartCore::ValueObject](https://github.com/smart-rb/smart_value-object) (10 threads) | 1.429 |

Rustly::Core still clocks in **3.6× faster** than [Dry::Struct](https://github.com/dry-rb/dry-struct) and **15.9× faster** than [SmartCore::ValueObject](https://github.com/smart-rb/smart_value-object) in single-threaded runs. It chews through the workload with 28 MB of allocations—just **1.17× over Dry::Struct** while burning **7.7× less than SmartCore**. Crank the dial to 10 threads and Rustly pulls further ahead, finishing **2× faster** than Dry and **6× faster** than SmartCore while staying allocation-efficient.

### Rustly Throughput (`bench/throughput.rb`)

- Options: `--count 10000`, `--rounds 5`, JSON mode enabled (default).
- Dataset: generated map of 10,000 keys to nested structs (see `bench/throughput.rb`).

| Measurement | Avg (s) | Min (s) | Max (s) |
| --- | --- | --- | --- |
| prepare_input(:ruby) | 0.004 | 0.004 | 0.005 |
| prepare_input(:json) | 0.001 | 0.000 | 0.001 |
| validate_no_gvl | 0.011 | 0.010 | 0.012 |

- All rounds completed without validation failures; the helper raises if any preparation or validation step returns an error tuple.

## Documentation

- [CHANGELOG.md](./CHANGELOG.md)
- [CONTRIBUTING.md](./CONTRIBUTING.md)
- [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md)

## License

Rustly::Core is released under the [MIT License](./LICENSE.txt).
