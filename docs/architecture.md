# Architecture

RailWeave uses a hub-and-spoke conversion model:

```text
source files ─> detector ─> source adapter ─> versioned IR ─> target adapter ─> package
                                              │
                                              └─> composition
```

With `n` simulators, direct pairwise converters trend toward `n²` implementations. RailWeave needs one importer and one exporter per simulator, or `2n` adapters. A new source immediately gains every target; a new target immediately gains every source.

## Crates

| Crate | Responsibility |
| --- | --- |
| `railweave-core` | versioned IR, provenance, diagnostics and bounded filesystem walking |
| `railweave-adapters` | source detection, built-in importers, portable bridges and external adapter protocol |
| `railweave-compose` | deterministic cross-source composition and reference remapping |
| `railweave-openbve` | driveable path selection, route rendering, train synthesis and package output |
| `railweave-cli` | stable command-line workflow |

The dependency direction points inward: targets and source adapters depend on the core; the core does not know any simulator-specific command syntax.

## Intermediate representation

IR schema version 1 stores:

- graph nodes and edges with 3D positions;
- length, gauge, gradient, radius, speed and electrification state;
- stations and stop time;
- typed asset references;
- vehicle mass, dimensions, braking and traction metadata;
- ordered consists;
- provenance for imported entities;
- coded diagnostics outside the project document.

New optional fields use Serde defaults so older schema-1 artifacts continue to load. A breaking semantic change requires a schema increment and an explicit migration.

## Path selection

OpenBVE is centered on a driveable player path. The IR is not. The target adapter therefore:

1. validates every edge reference;
2. finds graph entry nodes;
3. selects deterministically by entity ID;
4. prefers an edge explicitly marked as a main path;
5. detects loops;
6. reports dropped components and branches.

This choice belongs in the target adapter because another simulator may represent and export the whole graph.

## Conversion loss

Every adapter returns a project plus diagnostics. Information that cannot be represented must be either retained as provenance/asset metadata or diagnosed. Silent success is a bug.

The OpenBVE package also includes the final diagnostics in `railweave-manifest.json`, so automation can gate releases without scraping terminal text.

## External adapters

Built-in importers cover formats that can be implemented and tested using redistributable synthetic fixtures. Proprietary or fast-moving formats use an executable boundary. The adapter accepts a source path and returns the same JSON document a built-in importer would have produced.

This boundary is intentionally language-independent. See [adapter-protocol.md](adapter-protocol.md).

## Asset policy

RailWeave converts locally owned content. It does not bypass encryption or DRM, ship proprietary fixtures, or grant permission to redistribute converted assets. Symlinks are skipped when copying native OpenBVE trains, and scans/copies are bounded to prevent malformed add-ons from exhausting a conversion run.
