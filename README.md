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
- MSTS/OpenRails `tsection.dat` section geometry, including relative `include` files, route overrides and install-level `GLOBAL/TSECTION.DAT`
- MSTS curve radius and direction reconstruction from `tsection.dat` plus observed TDB yaw changes, with TDB flip state as fallback
- MSTS/OpenRails textual/UTF-16 `.pat` waypoint and path-topology fallback
- MSTS/OpenRails `.con` parsing into ordered structured consists, with engine/wagon role, flip state, source UID and resolved `.eng` / `.wag` member assets
- basic MSTS/OpenRails `.eng` / `.wag` vehicle metadata: name/type, mass, dimensions, axle/wheel counts and brake metadata, with MSTS unit conversion to SI
- JSON IR import/output
- TOML composition manifests that can combine supported raw sources and saved IR inputs
- deterministic entity-ID remapping during composition, including vehicle and consist member references
- OpenBVE CSV route export from a selected driveable IR path
- fixture-based cross-simulator and round-trip tests in CI, including MSTS TDB curve -> OpenBVE `Track.Curve`

Not implemented yet:

- compressed/binary MSTS `.tdb` parsing
- MSTS dynamic-track transforms beyond the currently supported section geometry forms
- deep MSTS traction/motor physics, cab and sound conversion
- Trainz route import
- RailWorks or Loksim3D import
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

For an MSTS/OpenRails route directory containing both `.tdb` and `.pat` data, RailWeave prefers the route-wide track database. It resolves section geometry from route-local `tsection.dat`, relative includes and the install-level `GLOBAL/TSECTION.DAT` layout used by MSTS/OpenRails. Exact section lengths are attached to TDB edges, and curved sections get a signed radius from the observed TDB yaw change when that orientation is available. If the textual/UTF-16 TDB cannot be parsed but a supported PAT path is present, RailWeave falls back to PAT topology and reports that fallback as a diagnostic.

MSTS `.con` files are not just opaque asset references anymore. RailWeave reads the ordered `Engine` / `Wagon` list, resolves the normal `TRAINS/TRAINSET/<folder>/<name>.eng|.wag` layout case-insensitively, and stores a structured consist whose members reference those rolling-stock assets while preserving role, orientation and source UID.

For resolved `.eng` and `.wag` members, RailWeave also reads the `Wagon` block into structured vehicle metadata. The current fields include `Name`, `Type`, `Mass`, `Size`, `ORTSNumberAxles`, `NumWheels`, `BrakeSystemType`, `BrakeEquipmentType` and `MaxBrakeForce`. MSTS/OpenRails mass, distance and force suffixes are normalized to SI; unsupported units produce diagnostics instead of guessed values.

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

That example imports the route through the BVE/OpenBVE adapter and the consist through the MSTS/OpenRails adapter, then puts both into one RailWeave IR while preserving provenance and remapping vehicle/consist asset IDs consistently.

Inputs may also use a previously generated IR file:

```toml
[inputs.route]
ir = "./route.railweave.json"
```

Export the driveable network path to an OpenBVE CSV route:

```bash
railweave export openbve composed.railweave.json -o route.csv
```

The current OpenBVE exporter writes route geometry only. If the composed IR contains rolling-stock, structured vehicle metadata, consists, cab or sound assets, they remain in the IR and the exporter reports that they were not yet emitted as an OpenBVE train package.

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
