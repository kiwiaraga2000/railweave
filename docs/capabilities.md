# Capability matrix

RailWeave is intentionally explicit about partial support. Detection means that a format can be identified; it does not mean that all of its content can be imported.

| Format | Detection | Source -> IR | Current imported data |
| --- | --- | --- | --- |
| BVE / OpenBVE | yes | partial | CSV primary rail geometry from `Track.Curve` and `Track.Pitch`, plus `Track.Limit` speed state |
| MSTS / OpenRails | yes | partial | textual or UTF-16 `.pat` waypoint topology, including main and siding links |
| Trainz | yes | no | — |
| Train Simulator / RailWorks | yes | no | — |
| Loksim3D | yes | no | — |
| OpenBVE target | n/a | not yet | exporter planned |

## BVE / OpenBVE

The current importer is deliberately focused on proving the route-geometry path into the common IR. It imports the implicit player rail and integrates curvature and pitch into 3D node positions.

Known gaps include auxiliary rails, switches, stations, signal logic, scenery, structures, power-supply state, cant, `Track.Turn`, and most route metadata. When a supported route uses auxiliary rails or `Track.Turn`, RailWeave emits a diagnostic instead of silently claiming a lossless conversion.

## MSTS / OpenRails

The first MSTS/OpenRails importer reads `.pat` files. A PAT path contains `TrackPDP` waypoints and `TrPathNode` links; RailWeave maps those links into the common graph IR. MSTS world tiles are normalized into local coordinates using a 2048 metre tile size.

This is path topology, not the complete route track database. Full `.tdb` / track-section geometry, world scenery, signalling and route-wide infrastructure remain future work.

If a route directory contains multiple `.pat` files, RailWeave currently imports the first sorted candidate and reports a diagnostic. Passing a specific `.pat` path selects it directly.

## Why these two first

BVE and MSTS represent route data in substantially different ways. Getting both through the same graph model early is a pressure test for the IR and avoids accidentally designing the core around one simulator.
