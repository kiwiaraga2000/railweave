# Capability matrix

RailWeave is intentionally explicit about partial support. Detection means that a format can be identified; it does not mean that all of its content can be imported or exported losslessly.

| Format / stage | Detection | Support | Current data |
| --- | --- | --- | --- |
| BVE / OpenBVE | yes | partial source -> IR | CSV primary rail geometry from `Track.Curve` and `Track.Pitch`, `Track.Limit` speed state, and train/cab/sound source asset references |
| MSTS / OpenRails | yes | partial source -> IR | textual/UTF-16 `.tdb` route-wide vector-section topology and coordinates, `tsection.dat` lengths/curves, `.pat` waypoint/path topology fallback, structured `.con` consists and resolved `.eng` / `.wag` member asset references |
| Trainz | yes | no importer yet | — |
| Train Simulator / RailWorks | yes | no importer yet | — |
| Loksim3D | yes | no importer yet | — |
| Composition | n/a | partial | select one input network, metadata from a named input, and assets/structured consists from any number of supported raw-source or saved-IR inputs |
| OpenBVE target | n/a | partial IR -> target | deterministic player-rail path selection and CSV export of gauge, curve, gradient and speed-limit state |

## BVE / OpenBVE

The route importer reads the implicit player rail and integrates curvature and pitch into 3D node positions. Source segment length, curve radius, gradient and speed-limit state are retained on IR edges when available.

For train content, RailWeave currently creates provenance-preserving asset references for `train.dat`, `panel.animated` / `panel.cfg`, and `sound.cfg`. These references make train content available to composition before deep parsing of rolling-stock physics, cabs and sounds is implemented.

Known route gaps include auxiliary rails, switches, stations, signal logic, scenery, structures, power-supply state, cant, `Track.Turn`, and most route metadata. When a supported route uses auxiliary rails or `Track.Turn`, RailWeave emits a diagnostic instead of silently claiming a lossless conversion.

## MSTS / OpenRails

For route directories, RailWeave prefers a textual or UTF-16 `.tdb` Track Database when one is available. The importer reads MSTS `TrackNode`, `UiD`, `TrPins`, `TrVectorNode` and `TrVectorSections` data, normalizes the 2048 metre tile coordinate system, reuses junction/end points as graph nodes, and emits the route-wide vector network into the common IR.

A TDB vector section identifies a `tsection.dat` section and stores the beginning coordinate and orientation of that section. RailWeave searches route-local OpenRails data, route-level overrides and the install-level `GLOBAL/TSECTION.DAT` layout case-insensitively; relative `include` directives are followed and later route definitions override included/global sections. Standard straight/curve definitions provide exact section length and curve radius/angle. Section lengths are attached to IR edges and are also used to derive average edge gradient from TDB endpoint elevations.

For curved sections, the sign of `curve_radius_m` is resolved primarily from the observed change in TDB yaw (`AY`) between the start of a vector section and the following section or endpoint. This means a flipped placement can override an opposite sign in the reusable `tsection.dat` definition. When an observed yaw delta is unavailable, the signed section angle plus the TDB flip flag is used as the fallback. Known straight/curve state is retained in provenance, and an import diagnostic reports how many TDB edges were resolved.

Compressed/binary TDB variants are not parsed yet. Dynamic/custom track is supported only where its section geometry can be represented by the current `TrackSection` parser; more complicated MSTS transforms remain future work. If a detected TDB cannot be handled and a supported PAT path is available, RailWeave reports the failed TDB import and falls back to PAT topology rather than silently dropping the route.

`.pat` import remains available directly. A PAT path contains `TrackPDP` waypoints and `TrPathNode` main/siding links; those are mapped into the same graph IR. If a route has multiple PAT files and PAT import is being used, RailWeave imports the first sorted candidate unless a specific `.pat` path is supplied.

`.con` handling is now structured rather than only file-level. RailWeave reads ordered `Engine` and `Wagon` entries, preserving engine/wagon role, `Flip` orientation and source `UiD`, and resolves each member to the standard `TRAINS/TRAINSET/<folder>/<name>.eng|.wag` layout case-insensitively. Each member is a rolling-stock asset and each `RollingStockConsist` stores ordered member references to those asset IDs. Missing member files are retained as expected source paths and reported diagnostically instead of being silently dropped. Full `.eng` / `.wag` vehicle physics, cab and sound data are not parsed into structured fields yet.

## Composition

A version-1 TOML manifest can load either supported raw sources or saved RailWeave JSON IR files. The current composer:

- chooses the network from one named input;
- optionally chooses metadata from another input;
- gathers assets and structured consists from any number of named inputs;
- remaps asset IDs deterministically so IDs do not collide with the selected network;
- rewrites every consist member's asset reference through the same remapping table and rejects internally broken consist references;
- preserves each entity's original provenance and carries input diagnostics forward.

This is enough to exercise a genuine cross-simulator path such as a BVE/OpenBVE route plus an ordered MSTS/OpenRails consist in one IR. It does not yet geometrically merge two independent railway networks or translate MSTS vehicle physics into an OpenBVE train definition.

## OpenBVE target

The first target backend exports a driveable path from the generic graph IR as an OpenBVE CSV route. It:

- selects a deterministic entry path through the graph;
- prefers an MSTS/OpenRails edge marked `main` when PAT path provenance identifies a branch as the main path;
- exports route gauge, `Track.Curve`, `Track.Pitch` and `Track.Limit` state when represented by the IR;
- uses source segment lengths when available and otherwise approximates length from node coordinates;
- uses a 1 metre OpenBVE block length and reports any position quantization;
- reports dropped branches, inferred geometry and other target loss explicitly.

MSTS TDB edges enriched from `tsection.dat` carry exact section length, average gradient and signed curve radius when the section geometry is available. An end-to-end fixture verifies that a curved TDB section becomes OpenBVE `.Curve` with the expected radius and that observed TDB yaw wins when a reusable section definition has the opposite sign. Known straight MSTS sections are distinguished from unresolved curvature so they no longer generate a false unknown-curvature warning. PAT-only routes remain coarser because PAT stores service/path waypoints rather than the complete route track database.

The target currently writes route CSV only. Composed rolling-stock assets, structured consists, cab, sound and other asset references remain in the RailWeave IR and trigger a diagnostic rather than being silently ignored.

## Why BVE and MSTS first

BVE and MSTS represent route data in substantially different ways. Getting both through the same graph model early pressure-tests the IR and avoids accidentally designing the core around one simulator.
