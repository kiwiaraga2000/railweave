# MSTS / OpenRails rolling-stock import

RailWeave imports rolling stock in layers instead of treating a consist as one opaque file.

## Consists

A `.con` file is parsed as an ordered `RollingStockConsist`. Each `Engine` / `Wagon` entry becomes a member with:

- an `asset_id` pointing at the resolved rolling-stock asset;
- `Engine` or `Wagon` role;
- `Flip` orientation;
- the source `UiD` when present.

The normal MSTS/OpenRails layout is resolved case-insensitively:

```text
TRAINS/
  CONSISTS/example.con
  TRAINSET/
    VehicleFolder/
      Motor.eng
      Trailer.wag
```

Missing member files are retained as expected provenance paths and produce diagnostics rather than disappearing from the consist.

## ENG / WAG metadata

For resolved `.eng` and `.wag` members, the outer `Wagon` block currently populates `RollingStockVehicle` with:

- `Name` and `Type`;
- `Mass`;
- `Size` as width, height and length;
- `ORTSNumberAxles` and `NumWheels`;
- `BrakeSystemType` and `BrakeEquipmentType`;
- `MaxBrakeForce`.

For locomotives / motor cars, the nested `Engine` block additionally supplies:

- `MaxPower` -> `max_power_w`;
- `MaxForce` -> `max_tractive_force_n`;
- `MaxContinuousForce` -> `max_continuous_force_n`;
- `MaxVelocity` -> `max_velocity_mps`.

The importer follows the same base units used by the OpenRails MSTS parser and normalizes data to SI. Supported suffixes include the common MSTS/OpenRails mass, distance, force, power and speed units. RailWeave also accepts `MW` as a convenient power extension. Unknown suffixes are not guessed: the affected value remains absent and `RW229_MSTS_VEHICLE_VALUE_UNSUPPORTED` is emitted.

## Composition

Vehicle metadata and consists refer to rolling-stock assets by entity ID. During composition RailWeave remaps those IDs together:

```text
source asset id ----+----> composed asset id
                    |
vehicle.asset_id ---+
                    |
consist member -----+
```

A broken source relationship is rejected with a composition error instead of producing dangling references.

## Current boundary

This is enough to preserve consist formation, basic dimensions/mass/braking, and headline traction limits across the common IR. It is not yet a complete MSTS locomotive simulation model.

Still to import deeply:

- traction curves and controller/notch behavior;
- electric/diesel-specific systems;
- brake-pipe/reservoir parameters beyond the current headline brake fields;
- cab-view definitions and controls;
- sound graphs;
- vehicle meshes/textures as linked structured resources.

The OpenBVE package exporter uses the structured fields above to synthesize a conservative `train.dat`. Missing detailed systems are recorded as defaults or losses in the package manifest; they are not presented as a physics-perfect conversion.
