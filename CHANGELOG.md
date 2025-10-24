# Changelog

All notable changes to this project are documented in this file following the
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

## [0.1.0] - 2025-10-24
### Changed
- Default build options now use `strict: false` and `freeze: :none`.
- Reworked hash/array parsing to avoid intermediate Ruby allocations (`rb_hash_foreach`,
  `rb_array_len`).
- Materialisation now allocates objects with `rb_obj_alloc`, skips `@attributes` unless requested,
  and freezes via C APIs only when needed.
- `Rustly::Core.build` now returns the materialized instance directly and raises `Rustly::Core::ValidationError`
  on invalid input; validation options are captured during `compile` instead of each build call.
- Validator micro-optimisations: `HashSet::with_capacity`, ASCII-aware string length checks,
  and leaner struct materialisation.
- Benchmarks: `Rustly::Core.build`.

## [0.0.0] - 2025-10-24
### Added
- Initial `rustly-core` version with stub implementations for `compile`, `build`, and `version`.
- Typed data classes `CompiledSchema` and `ErrorSet` with safe allocators.
- Baseline RSpec and Rust unit tests plus configurations for RuboCop, `rustfmt`.
- CI covering Linux/macOS, Ruby 3.2–3.4, and Rust toolchain checks.
- Documentation set: README, contributing guide, and code of conduct.

[0.1.0]: https://github.com/rustly/rustly-core/releases/tag/v0.1.0
