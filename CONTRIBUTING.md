# Contributing

RailWeave welcomes focused source adapters, target adapters, parser fixes, synthetic fixtures and documentation improvements.

## Before opening a change

1. Search existing issues and the capability matrix.
2. Keep third-party simulator assets out of the repository.
3. Add a small original or synthetic fixture for parser behavior.
4. Report approximations through a stable diagnostic code.
5. Preserve provenance whenever a source can provide it.

## Local checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --release
```

Behavior changes need a regression test. Format support should update `docs/capabilities.md`. Public API changes should update `CHANGELOG.md`.

## Commit and PR scope

Prefer one coherent change per pull request. Explain the input revision, the preserved data, known loss and the exact validation command. Do not describe detection-only work as conversion support.

By contributing, you agree that your contribution is licensed under MIT OR Apache-2.0, at the recipient's option.
