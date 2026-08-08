# RailWeave

RailWeave is a toolkit for importing, combining and exporting railway-simulator content across otherwise incompatible ecosystems.

The goal is not just `A -> B` conversion. RailWeave uses a common railway model so content from different simulators can be composed before export. A route might come from Trainz, rolling stock from MSTS/OpenRails, sounds from BVE, and the resulting package can be exported to OpenBVE.

```text
Trainz ---------\
MSTS/OpenRails --\
BVE -------------+--> adapters --> RailWeave IR --> compose --> targets
RailWorks -------/
Loksim3D -------/
```

OpenBVE is the first target.

## Status

Very early development. The first milestone is to make the architecture real rather than build one large one-off converter:

- versioned, simulator-independent railway IR
- provenance and conversion diagnostics so lossy conversions are visible
- source-format auto-detection
- at least two independent source adapters
- composition of route / rolling stock / cab / sounds / traffic from different sources
- OpenBVE export as the first backend

## Planned CLI

```bash
railweave scan /path/to/content
railweave import /path/to/content -o project.railweave.json
railweave compose project.toml -o composed.railweave.json
railweave export openbve composed.railweave.json -o ~/Documents/OpenBVE\ Addons
```

`scan` is being implemented first. It should identify a source without requiring the user to know which simulator layout it came from.

## Design rules

1. **The IR is not OpenBVE-shaped.** Network topology is stored as a graph; OpenBVE-specific concepts are introduced only by the OpenBVE exporter.
2. **Composition is a core operation.** Mixing sources is not an afterthought.
3. **Never silently lose information.** Unsupported or approximated features are reported as diagnostics.
4. **Keep provenance.** Imported entities remember which source files/assets they came from.
5. **Do not redistribute third-party assets.** RailWeave converts content locally; source-content licensing remains the user's responsibility.

See [`docs/architecture.md`](docs/architecture.md) for the initial model.
