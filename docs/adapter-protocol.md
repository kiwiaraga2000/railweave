# External adapter protocol

External adapters let any game, editor or private asset pipeline participate in RailWeave without being linked into the Rust workspace.

## Invocation

```bash
railweave convert ./source \
  --adapter /path/to/railweave-example-game \
  --to openbve \
  -o ./build/openbve
```

RailWeave launches:

```text
/path/to/railweave-example-game ./source
```

The process receives:

| Variable | Value |
| --- | --- |
| `RAILWEAVE_ADAPTER_PROTOCOL` | `1` |
| `RAILWEAVE_IR_SCHEMA` | the required IR schema, currently `1` |

## Output contract

- Write exactly one UTF-8 JSON `ImportResult` document to stdout.
- Write progress and logs to stderr.
- Exit zero only when the document is complete.
- Keep stdout below 64 MiB.
- Use source-relative or absolute provenance paths consistently.
- Emit a warning for every known approximation or omission.

Minimal output:

```json
{
  "project": {
    "schema_version": 1,
    "metadata": { "title": "Example", "description": null },
    "network": {
      "nodes": [
        { "id": 1, "position": { "x": 0.0, "y": 0.0, "z": 0.0 }, "provenance": null },
        { "id": 2, "position": { "x": 0.0, "y": 0.0, "z": 100.0 }, "provenance": null }
      ],
      "edges": [
        {
          "id": 3,
          "from": 1,
          "to": 2,
          "gauge_mm": 1435,
          "electrification": null,
          "speed_limit_kmh": 80.0,
          "length_m": 100.0,
          "curve_radius_m": null,
          "gradient_per_mille": null,
          "provenance": null
        }
      ]
    },
    "assets": [],
    "consists": [],
    "vehicles": [],
    "stations": []
  },
  "diagnostics": []
}
```

The adapter must produce internally consistent entity IDs. RailWeave validates the schema boundary and the target exporter validates graph references before writing output.

## Portable bridge alternative

When an editor can already export coordinates, no executable is necessary. Use:

- GeoJSON `LineString` / `MultiLineString`;
- `*.railweave.csv` with metric `x,y,z` columns;
- a saved `*.railweave.json` document.

A game directory containing one of these files is imported through the bridge automatically after its native format is detected.
