# Capability matrix

RailWeave is intentionally explicit about partial support. Detection means that a format can be identified; it does not mean that all of its content can be imported or exported losslessly.

| Format / stage | Detection | Support | Current data |
| --- | --- | --- | --- |
| BVE / OpenBVE | yes | partial source -> IR | CSV primary rail geometry from `Track.Curve` and `Track.Pitch`, `Track.Limit` speed state, and train/cab/sound source asset references |
| MSTS / OpenRails | yes | partial source -> IR | textual/UTF-16 `.tdb` route-wide vector-section topology and coordinates, `.pat` waypoint/path topology fallback, and `.con` rolling-stock source asset references |
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

For route directories, RailWeave now prefers a textual or UTF-16 `.tdb` Track Database when one is available. The importer reads MSTS `TrackNode`, `UiD`, `TrPins`, `TrVectorNode` and `TrVectorSections` data, normalizes the 2048 metre tile coordinate system, reuses junction/end points as graph nodes, and emits the route-wide vector network into the common IR.

A TDB vector section identifies a `tsection.dat` section and stores the beginning coordinate and orientation of that section. RailWeave currently preserves the section/shape identifiers in provenance and connects those coordinates as straight chords. Exact standard/dynamic track-section length and curvature from `tsection.dat` are not parsed yet, so this stage is route-wide topology and sampled geometry rather than a lossless reconstruction of every rail arc.

Compressed/binary TDB variants are not parsed yet. If a detected TDB cannot be handled and a supported PAT path is available, RailWeave reports the failed TDB import and falls back to PAT topology rather than silently dropping the route.

`.pat` import remains available directly. A PAT path contains `TrackPDP` waypoints and `TrPathNode` main/siding links; those are mapped into the same graph IR. If a route has multiple PAT files and PAT import is being used, RailWeave imports the first sorted candidate unless a specific `.pat` path is supplied.

`.con` files can also be imported as rolling-stock asset references. A direct `.con` input therefore produces a valid asset-only RailWeave project that can be combined with a network from another source. Consist vehicles, physics, cabs, sounds, world scenery and signalling remain future work.

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
- prefers an MSTS/OpenRails edge marked `main` when PAT path provenance identifies a branch as the main path;
- exports route gauge, `Track.Curve`, `Track.Pitch` and `Track.Limit` state when represented by the IR;
- uses source segment lengths when available and otherwise approximates length from node coordinates;
- uses a 1 metre OpenBVE block length and reports any position quantization;
- reports dropped branches, inferred geometry and other target loss explicitly.

MSTS geometry that has not yet been enriched from `tsection.dat` has unknown exact curvature, so those sections are currently exported as straight chords with diagnostics. PAT-only routes are coarser still because PAT stores service/path waypoints rather than the complete route track database. Neither case is presented as lossless conversion.

The target currently writes route CSV only. Composed rolling-stock, cab, sound and other asset references remain in the RailWeave IR and trigger a diagnostic rather than being silently ignored.

## Why BVE and MSTS first

BVE and MSTS represent route data in substantially different ways. Getting both through the same graph model early pressure-tests the IR and avoids accidentally designing the core around one simulator.
