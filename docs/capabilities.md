# Capability matrix

RailWeave reports unsupported, inferred and defaulted data as diagnostics. Detection only identifies a source; it is not a claim of lossless import.

## Sources

| Source | Detection | Network | Rolling stock | Assets | Notes |
| --- | :---: | --- | --- | --- | --- |
| BVE / OpenBVE | yes | primary rail geometry, curve, pitch, limits | native train reference | train, panel, sound references | auxiliary rails, scenery and signalling are diagnosed but not yet converted |
| MSTS / OpenRails | yes | textual/UTF-16 TDB, PAT fallback, `tsection.dat` lengths and curves | CON consists; ENG/WAG mass, dimensions, brakes, force, power and speed | rolling-stock references | compressed TDB and deep cab/sound systems require an adapter or future built-in work |
| RailWeave JSON | yes | full IR | full IR | full IR | versioned, deterministic interchange |
| GeoJSON | yes | LineString and MultiLineString geometry | — | — | reads `gauge_mm`, `speed_limit_kmh` / `maxspeed` properties |
| RailWeave track CSV | yes | coordinates, line grouping, gauge, limits, radius and gradient | — | — | also imports stations and dwell time |
| Trainz | yes | via portable bridge or external adapter | via adapter | via adapter | native route topology is stored in proprietary game databases |
| Train Simulator / RailWorks | yes | via portable bridge or external adapter | via adapter | via adapter | binary `Tracks.bin` revisions normally need Serz/game tooling |
| Loksim3D | yes | via portable bridge or external adapter | via adapter | via adapter | packages/modules vary by release |

Portable bridge discovery looks for `.railweave.json`, `.geojson`, `.railweave.csv`, or `railweave-track.csv` inside a detected game source. For other formats or deeper game-specific data, use the [external adapter protocol](adapter-protocol.md).

## Composition

The version-1 TOML composer can:

- choose a network and metadata source independently;
- combine raw supported sources and saved IR documents;
- remap entity and asset IDs deterministically;
- preserve rolling-stock asset, vehicle and consist references;
- reject dangling references rather than writing corrupt IR;
- carry every input diagnostic into the composed result.

Geometric merging of independent route graphs is not yet automatic. Select one route network and compose assets from other inputs.

## OpenBVE target

`railweave convert ... --to openbve` emits a complete package tree with:

- deterministic driveable-path selection;
- gauge, curve, pitch and speed-limit state;
- station and stop commands when stations exist in the IR;
- exact source segment lengths when available;
- a generated `train.dat` from structured vehicle/consist physics;
- native OpenBVE train asset copying when a native train source exists;
- a conservative playable fallback train when no rolling stock exists;
- a UTF-8 `README.txt` and machine-readable `railweave-manifest.json` report.

Current target losses include non-selected branches, complex signalling, scenery/object placement, cabs and non-native sound translation. Each is surfaced as a coded diagnostic where the source importer can observe it.

## Diagnostic classes

| Range | Area |
| --- | --- |
| `RW0xx` | discovery, adapter protocol and source dispatch |
| `RW1xx` | BVE and portable interchange inputs |
| `RW2xx` | MSTS / OpenRails |
| `RW3xx` | composition |
| `RW4xx` | OpenBVE export and packaging |

Errors stop a conversion. Warnings identify a meaningful approximation or omitted feature. Informational diagnostics record deterministic choices and successful enrichments.
