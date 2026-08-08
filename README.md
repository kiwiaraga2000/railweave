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

Very early development, but import and cross-simulator composition are already real code paths.

Implemented:

- versioned, simulator-independent railway IR
- provenance and explicit conversion diagnostics
- source-format auto-detection for BVE/OpenBVE, MSTS/OpenRails, Trainz, RailWorks and Loksim3D
- BVE/OpenBVE CSV primary-track geometry import (`Curve`, `Pitch`, `Limit`)
- BVE/OpenBVE train asset references from `train.dat`, panel files and `sound.cfg`
- MSTS/OpenRails textual/UTF-16 `.pat` waypoint and path-topology import
- MSTS/OpenRails `.con` rolling-stock asset references
- JSON IR import/output
- TOML composition manifests that can combine supported raw sources and saved IR inputs
- deterministic entity-ID remapping during composition
- fixture-based cross-simulator tests and CI

Not implemented yet:

- full MSTS `.tdb` route geometry
- Trainz route import
- RailWorks or Loksim3D import
- deep rolling-stock/cab/sound conversion
- merging several route networks into one network
- OpenBVE export

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

For an MSTS/OpenRails route containing several paths, pass a specific `.pat` file to select one:

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
