use railweave_core::{ImportResult, IR_SCHEMA_VERSION};

#[test]
fn version_one_vehicle_without_traction_fields_still_loads() {
    let json = r#"{
  "project": {
    "schema_version": 1,
    "metadata": {
      "title": null,
      "description": null
    },
    "network": {
      "nodes": [],
      "edges": []
    },
    "assets": [
      {
        "id": 1,
        "kind": "rolling_stock",
        "name": "EMU/Motor",
        "provenance": {
          "source_format": "msts-openrails",
          "source_path": "Motor.eng",
          "source_id": null
        }
      }
    ],
    "consists": [],
    "vehicles": [
      {
        "asset_id": 1,
        "name": "Motor car",
        "vehicle_type": "Engine",
        "mass_kg": 50000.0,
        "width_m": 3.0,
        "height_m": 4.0,
        "length_m": 21.0,
        "axle_count": 4,
        "wheel_count": 8.0,
        "brake_system_type": "Air_single_pipe",
        "brake_equipment_type": null,
        "max_brake_force_n": 100000.0
      }
    ]
  },
  "diagnostics": []
}"#;

    let result: ImportResult = serde_json::from_str(json).unwrap();
    assert_eq!(result.project.schema_version, IR_SCHEMA_VERSION);
    assert_eq!(result.project.vehicles.len(), 1);
    let vehicle = &result.project.vehicles[0];
    assert_eq!(vehicle.mass_kg, Some(50_000.0));
    assert_eq!(vehicle.max_power_w, None);
    assert_eq!(vehicle.max_tractive_force_n, None);
    assert_eq!(vehicle.max_continuous_force_n, None);
    assert_eq!(vehicle.max_velocity_mps, None);
}

#[test]
fn version_one_project_without_consists_or_vehicles_still_loads() {
    let json = r#"{
  "project": {
    "schema_version": 1,
    "metadata": {
      "title": null,
      "description": null
    },
    "network": {
      "nodes": [],
      "edges": []
    },
    "assets": []
  },
  "diagnostics": []
}"#;

    let result: ImportResult = serde_json::from_str(json).unwrap();
    assert!(result.project.consists.is_empty());
    assert!(result.project.vehicles.is_empty());
}
