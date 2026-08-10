<p align="center">
  <img src="assets/railweave-mark.svg" width="112" alt="RailWeave mark">
</p>

<h1 align="center">RailWeave</h1>

<p align="center">
  A conversion toolkit for moving railway routes and rolling stock between simulator ecosystems.
</p>

<p align="center">
  <a href="https://github.com/kiwiaraga2000/railweave/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/kiwiaraga2000/railweave/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/kiwiaraga2000/railweave/releases"><img alt="Release" src="https://img.shields.io/github/v/release/kiwiaraga2000/railweave?display_name=tag&sort=semver"></a>
  <a href="LICENSE-MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-5064a8"></a>
  <img alt="Rust 1.74+" src="https://img.shields.io/badge/rust-1.74%2B-b7410e">
</p>

<p align="center">
  <a href="https://github.com/kiwiaraga2000/railweave/releases/latest">Download</a>
  · <a href="docs/capabilities.md">Formats</a>
  · <a href="docs/architecture.md">Architecture</a>
  · <a href="docs/adapter-protocol.md">Adapter protocol</a>
</p>

RailWeave converts source content into a versioned, simulator-neutral railway model and exports that model to a target simulator. OpenBVE is the first production target. The same architecture is designed to add new games once, instead of building and maintaining a converter for every possible pair.

RailWeave is an open-source railway simulator interoperability layer for route conversion, rolling-stock conversion and cross-simulator composition. Its Rust core and adapter boundary cover BVE / OpenBVE, MSTS / OpenRails, Trainz, Train Simulator / RailWorks, Loksim3D, GIS inputs and additional simulator-specific formats without coupling the common model to one game.

```text
BVE / OpenBVE ───────┐
MSTS / OpenRails ────┤
GeoJSON / track CSV ─┼─> source adapters ─> RailWeave IR ─> target adapters ─> OpenBVE
community adapters ──┘                              └──────> future targets
```

## Install

| Platform | Binary |
| --- | --- |
| Linux x86-64 | [`railweave-linux-x86_64.tar.gz`](https://github.com/kiwiaraga2000/railweave/releases/latest/download/railweave-linux-x86_64.tar.gz) |
| macOS Apple silicon | [`railweave-macos-aarch64.tar.gz`](https://github.com/kiwiaraga2000/railweave/releases/latest/download/railweave-macos-aarch64.tar.gz) |
| Windows x86-64 | [`railweave-windows-x86_64.zip`](https://github.com/kiwiaraga2000/railweave/releases/latest/download/railweave-windows-x86_64.zip) |

Or install the current source release with Cargo:

```bash
cargo install --git https://github.com/kiwiaraga2000/railweave --tag v0.1.0 --locked railweave
```

Every release includes a `SHA256SUMS` file for verification.

## One-command conversion

```bash
railweave convert ./MyRoute \
  --to openbve \
  --name "My Route" \
  -o ./build/my-route
```

The output is an OpenBVE-ready package:

```text
build/my-route/
├── Railway/Route/my-route/route.csv
├── Train/my-route/train.dat
├── README.txt
└── railweave-manifest.json
```

The manifest records provenance, entity counts, defaults and every known conversion loss. A conversion is never called lossless merely because it produced files.

## What works today

| Source | Detection | Built-in conversion to OpenBVE |
| --- | :---: | --- |
| BVE / OpenBVE | yes | route geometry, limits and native train assets |
| MSTS / OpenRails | yes | TDB/PAT routes, `tsection.dat` curves, consists and ENG/WAG physics metadata |
| RailWeave JSON | yes | lossless IR interchange |
| GeoJSON | yes | `LineString` / `MultiLineString` track geometry and common gauge/speed properties |
| RailWeave track CSV | yes | metric geometry, gauge, gradient, curves, limits and stations |
| Trainz | yes | portable bridge or external adapter required for proprietary route databases |
| Train Simulator / RailWorks | yes | portable bridge or external adapter required for binary `Tracks.bin` revisions |
| Loksim3D | yes | portable bridge or external adapter required for route modules/packages |

See [the exact capability matrix](docs/capabilities.md). It separates detection, import, composition and export so partial support is visible.

## Any game through the adapter protocol

Proprietary formats change independently of RailWeave and often require an installed game SDK. An external adapter makes those formats first-class without coupling the core to a particular game version:

```bash
railweave convert ./source-route \
  --adapter ./railweave-my-game \
  --to openbve \
  -o ./build/openbve
```

The adapter receives the source path, then writes one versioned RailWeave `ImportResult` JSON document to stdout. This makes support for a new game an `O(1)` adapter, and every current or future target immediately benefits from it. The stable contract is documented in [External adapter protocol](docs/adapter-protocol.md).

For editor or GIS pipelines, use the portable formats directly:

```csv
x,y,z,gauge_mm,speed_limit_kmh,curve_radius_m,gradient_per_mille,station
0,0,0,1435,60,,,Origin
0,1,100,1435,80,,10,
10,2,200,1435,80,800,10,Terminus
```

## Inspect, import, compose, export

```bash
# Identify source fingerprints.
railweave scan ./content

# Keep a reviewable intermediate artifact.
railweave import ./content -o route.railweave.json

# Combine a route from one simulator with a consist from another.
railweave compose railweave.toml -o composed.railweave.json

# Export only OpenBVE route CSV when package generation is not needed.
railweave export openbve composed.railweave.json -o route.csv
```

Composition manifests are intentionally small:

```toml
version = 1

[inputs.route]
source = "./route.csv"

[inputs.stock]
source = "./TRAINS/CONSISTS/demo.con"

[compose]
network = "route"
assets = ["stock"]
```

## Design guarantees

- **Simulator-neutral core.** Track is a graph, not OpenBVE rail zero or an MSTS vector list.
- **Deterministic output.** Stable path and ID selection makes conversions reproducible.
- **Explicit loss.** Unsupported, inferred and defaulted data produce coded diagnostics.
- **Provenance.** Imported entities retain their source format, file and source identifier.
- **Local-only assets.** RailWeave does not bypass DRM or redistribute third-party content.
- **Extensible in both directions.** Source and target adapters meet only at the versioned IR.

The deeper model is described in [Architecture](docs/architecture.md).

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --release
```

The repository targets Rust 1.74 or newer and tests Linux, macOS and Windows in CI. Small synthetic fixtures are used to avoid redistributing simulator assets.

Contributions are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md), and report security-sensitive issues according to [SECURITY.md](SECURITY.md).

## License

Licensed under either [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
