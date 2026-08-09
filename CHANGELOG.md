# Changelog

All notable changes are documented here. This project follows Semantic Versioning.

## [Unreleased]

## [0.1.0] - 2026-08-10

### Added

- one-command OpenBVE package conversion;
- generated OpenBVE trains from structured consist/vehicle metadata;
- native OpenBVE train asset preservation;
- GeoJSON and portable track CSV source adapters;
- station IR and OpenBVE station export;
- stable language-independent external adapter protocol;
- machine-readable package manifests with conversion diagnostics;
- dual MIT/Apache-2.0 licensing and release infrastructure.

### Fixed

- missing `serde_json` test dependency that broke the default CI build;
- schema-1 source format aliases for existing IR documents;
- unfinished MSTS/OpenRails traction metadata parsing;
- noisy self-modifying one-shot GitHub Actions workflows.

[Unreleased]: https://github.com/kiwiaraga2000/railweave/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/kiwiaraga2000/railweave/releases/tag/v0.1.0
