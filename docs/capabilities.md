# Capability matrix

RailWeave is intentionally explicit about partial support. Detection means that a format can be identified; it does not mean that all of its content can be imported or exported losslessly.

| Format / stage | Detection | Support | Current data |
| --- | --- | --- | --- |
| BVE / OpenBVE | yes | partial source -> IR | CSV primary rail geometry from `Track.Curve` and `Track.Pitch`, `Track.Limit` speed state, and train/cab/sound source asset references |
| MSTS / OpenRails | yes | partial source -> IR | textual or UTF-16 `.pat` waypoint topology with main/siding links, plus `.con` rolling-stock source asset references |
| Trainz | yes | no importer yet | — |
| Train Simulator / RailWorks | yes | no importer yet | — |
| Loksim3D | yes | no importer yet | — |
| Composition | n/a | partial | select one input network, metadata from a named input, and assets from any number of supported raw-source or saved-IR inputs |
| OpenBVE target | n/a | partial IR -> target | deterministic player-rail path selection and CSV export of gauge, curve, gradient and speed-limit state |

## BVE / OpenBVE

The route importer reads the implicit player rail and integrates curvature and pitch into 3D node positions. Source segment length, curve radius, gradient and speed-limit state are retained on IR edges when available.

For train content, RailWeave currently creates provenance-preserving asset references for `train.dat`, `panel.animated` / `panel.cfg`, and `sound.cfg`. These references make train content available to composition before deep parsing of rolling-stock physics, cabs and sounds is implemented.

Known route gaps include auxiliary rails, switches, stations, signal logic, scenery, structures, power-supply state, cant, `Track.Turn`, and most route metadata. When a supported route uses auxiliary rails or `Track.Turn`, RailWeave emits a diagnostic instead of silently claiming a lossless conversion.

## MSTS / OpenRails

The route importer currently reads `.pat` files. A PAT path contains `TrackPDP` waypoints and `TrPathNode` links; RailWeave maps those links into the common graph IR. MSTS world tiles are normalized into local coordinates using a 2048 metre tile size.

`.con` files can also be imported as rolling-stock asset references. A direct `.con` input therefore produces a valid asset-only RailWeave project that can be combined with a network from another source.

PAT import is path topology, not the complete route track database. Full `.tdb` / track-section geometry, consist vehicles, physics, cabs, sounds, world scenery, signalling and route-wide infrastructure remain future work.

If a route directory contains multiple `.pat` files, RailWeave currently imports the first sorted candidate and reports a diagnostic. Passing a specific `.pat` path selects it directly.

## Composition

A version-1 TOML manifest can load either supported raw sources or saved RailWeave JSON IR files. The current composer:

- chooses the network from one named input;
- optionally chooses metadata from another input;
- gathers assets from any number of named inputs;
- remaps asset IDs deterministically so IDs do not collide with the selected network;
- preserves each entity's original provenance and carries input diagnostics forward.

This is enough to exercise a genuine cross-simulator path such as a BVE/OpenBVE route plus an MSTS/OpenRails `.con` rolling-stock asset in one IR. It does not yet geometrically merge two independent railway networks.

## OpenBVE target

The first target backend exports a driveable path from the generic graph IR as an OpenBVE CSV route. It:

- selects a deterministic entry path through the graph;
- prefers an MSTS/OpenRails edge marked `main` when a path node branches;
- exports route gauge, `Track.Curve`, `Track.Pitch` and `Track.Limit` state when represented by the IR;
- uses source segment lengths when available and otherwise approximates length from node coordinates;
- uses a 1 metre OpenBVE block length and reports any position quantization;
- reports dropped branches, inferred geometry and other target loss explicitly.

MSTS `.pat` files do not contain full track-section geometry, so PAT-only routes are currently exported as straight chords between path waypoints with diagnostics. This is deliberately not presented as a lossless conversion.

The target currently writes route CSV only. Composed rolling-stock, cab, sound and other asset references remain in the RailWeave IR and trigger a diagnostic rather than being silently ignored.

## Why BVE and MSTS first

BVE and MSTS represent route data in substantially different ways. Getting both through the same graph model early pressure-tests the IR and avoids accidentally designing the core around one simulator.
