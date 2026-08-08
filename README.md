# RailWeave

RailWeave is a toolkit for importing, combining and exporting railway-simulator content across otherwise incompatible ecosystems.

The goal is not just `A -> B` conversion. RailWeave uses a common railway model so content from different simulators can be composed before export. A route can come from one simulator, rolling stock from another, and cabs, sounds or other assets from still others.

```text
Trainz ----------\
MSTS/OpenRails ---\
BVE ---------------+--> adapters --> RailWeave IR --> compose --> targets
RailWorks ---------/
Loksim3D ---------/
```

OpenBVE is the first target.

## Status

Very early development, but import, cross-simulator composition and the first OpenBVE export path are real code paths.

Implemented:

- versioned, simulator-independent railway IR
- provenance and explicit conversion diagnostics
- source-format auto-detection for BVE/OpenBVE, MSTS/OpenRails, Trainz, RailWorks and Loksim3D
- BVE/OpenBVE CSV primary-track geometry import (`Curve`, `Pitch`, `Limit`)
- BVE/OpenBVE train asset references from `train.dat`, panel files and `sound.cfg`
- MSTS/OpenRails textual/UTF-16 `.tdb` route-wide vector-section topology and coordinates
- MSTS/OpenRails textual/UTF-16 `.pat` waypoint and path-topology fallback
- MSTS/OpenRails `.con` rolling-stock asset references
- JSON IR import/output
- TOML composition manifests that can combine supported raw sources and saved IR inputs
- deterministic entity-ID remapping during composition
- OpenBVE CSV route export from a selected driveable IR path
- fixture-based cross-simulator and round-trip tests in CI

Not implemented yet:

- exact MSTS `tsection.dat` section geometry and curvature
- compressed/binary MSTS `.tdb` parsing
- Trainz route import
- RailWorks or Loksim3D import
- deep rolling-stock/cab/sound conversion
- merging several route networks into one network
- exporting composed rolling-stock/cab/sound assets into an OpenBVE train package

See [`docs/capabilities.md`](docs/capabilities.md) for the exact support matrix.

## CLI

Detect a source:

```bash
railweave scan /path/to/content
```

Import supported source content into the versioned IR:

```bash
railweave import /path/to/content -o project.railweave.json
```

Without `-o`, the JSON is written to stdout.

For an MSTS/OpenRails route directory containing both `.tdb` and `.pat` data, RailWeave prefers the route-wide track database. If the textual/UTF-16 TDB cannot be parsed but a supported PAT path is present, it falls back to PAT topology and reports that fallback as a diagnostic.

Compose several inputs:

```bash
railweave compose railweave.toml -o composed.railweave.json
```

A composition manifest can point directly at raw supported sources:

```toml
version = 1

[inputs.route]
source = "./route.csv"

[inputs.stock]
source = "./ED4M.con"

[compose]
network = "route"
assets = ["stock"]
```

That example imports the route through the BVE/OpenBVE adapter and the rolling-stock reference through the MSTS/OpenRails adapter, then puts both into one RailWeave IR while preserving their provenance.

Inputs may also use a previously generated IR file:

```toml
[inputs.route]
ir = "./route.railweave.json"
```

Export the driveable network path to an OpenBVE CSV route:

```bash
railweave export openbve composed.railweave.json -o route.csv
```

The current OpenBVE exporter writes route geometry only. If the composed IR contains rolling-stock, cab or sound assets, they remain in the IR and the exporter reports that they were not yet emitted as an OpenBVE train package.

For an MSTS/OpenRails route containing several PAT services, a specific `.pat` file can still be imported directly when path topology rather than the full TDB network is wanted:

```bash
railweave import ROUTES/MyRoute/PATHS/service.pat -o service.railweave.json
```

## Design rules

1. **The IR is not OpenBVE-shaped.** Network topology is stored as a graph; OpenBVE-specific concepts are introduced only by the OpenBVE exporter.
2. **Composition is a core operation.** Mixing sources is not an afterthought.
3. **Never silently lose information.** Unsupported or approximated features are reported as diagnostics.
4. **Keep provenance.** Imported entities remember which source files/assets they came from.
5. **Do not redistribute third-party assets.** RailWeave converts content locally; source-content licensing remains the user's responsibility.

See [`docs/architecture.md`](docs/architecture.md) for the model and roadmap.
