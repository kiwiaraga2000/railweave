# Architecture

RailWeave treats simulator formats as adapters around a versioned intermediate representation (IR). The IR must preserve railway meaning without inheriting one simulator's storage model.

## Pipeline

```text
source content
    |
    v
format detection
    |
    v
source adapter
    |
    v
RailWeave IR + provenance + diagnostics
    |
    +--> composition / overrides / conflict resolution
    |
    v
target adapter
    |
    v
target simulator package
```

OpenBVE is the first target, but the core must not depend on OpenBVE concepts.

## Why an IR

A direct converter for every pair of simulators scales poorly and makes mixed-source packages awkward. With `n` formats, pairwise conversion trends toward `n^2` conversion paths. RailWeave instead gives every source one import path and every target one export path.

The IR also gives composition a stable boundary. A route, train, cab, sound set and traffic plan can originate in different ecosystems and still be reasoned about consistently.

## Core model

The first IR version starts deliberately small:

- a graph of track nodes and edges
- physical properties such as gauge, electrification and speed limits
- asset references for meshes, textures, sounds, cabs, signals and rolling stock
- provenance for every imported entity where practical
- diagnostics for unsupported, approximated or conflicting data

Later versions will add richer geometry, switches, stations/platforms, signalling logic, rolling-stock physics, cabs/controls, timetables and traffic.

### Track topology

Track is a graph in the IR. It is not represented as OpenBVE's primary rail plus offsets. That transformation belongs in the OpenBVE target adapter.

This is important for sources such as Trainz and MSTS/OpenRails, where the source may contain yards, branches and multiple possible paths through a network.

### Provenance

Imported data should retain enough information to answer questions such as:

- Which source file produced this track edge?
- Which simulator format did this asset come from?
- Was this value native, inferred or overridden during composition?

Provenance is also useful for diagnostics and reproducible builds.

### Diagnostics and loss

Conversion loss must be explicit. A target adapter should emit a diagnostic when it has to drop, approximate or flatten a feature rather than silently producing a plausible-looking but semantically different result.

Examples include unsupported signal logic, animation features, complex track topology or rolling-stock systems that the target cannot represent.

## Composition

Composition is a first-class stage rather than a post-processing trick. A composition manifest will eventually be able to select parts from different imported projects, for example:

```toml
[route]
source = "trainz-route"

[rolling_stock]
source = "msts-ed4m"

[sounds]
source = "bve-ed4m-sounds"

[target]
format = "openbve"
```

Conflict resolution must be deterministic and visible in diagnostics.

## Source adapters

Initial detector work covers fingerprints for:

- BVE / OpenBVE
- MSTS / OpenRails
- Trainz
- Train Simulator / RailWorks
- Loksim3D

Detection does not imply full import support. Each adapter will publish a capability matrix as implementation progresses.

The first implementation goal is to support at least two independent source formats end-to-end early enough to pressure-test the IR.

## Target adapters

The first target is OpenBVE. Its exporter will be responsible for mapping a general railway graph to OpenBVE route concepts and for reporting topology or feature loss.

Target-specific constraints must not leak back into the core IR unless they describe a genuinely simulator-independent railway concept.

## Asset policy

RailWeave is a local conversion tool. The repository should contain only original test fixtures, freely redistributable fixtures, or small synthetic examples created for testing.

Adapters should work on content the user can lawfully access. RailWeave will not include DRM circumvention or distribute third-party simulator assets.

## Near-term milestones

1. `railweave scan <path>` with source-format detection.
2. Versioned IR types and conversion diagnostics.
3. Two source adapters capable of producing meaningful IR data.
4. Composition manifest and deterministic merge rules.
5. First OpenBVE route export.
6. Capability reports and fixture-based regression tests.
