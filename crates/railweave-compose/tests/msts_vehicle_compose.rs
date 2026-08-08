use railweave_compose::compose_manifest;
use railweave_core::RollingStockRole;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "railweave-msts-vehicle-compose-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn composes_bve_route_with_msts_consist_and_traction_metadata() {
    let root = fixture();
    let stock = root.join("stock");
    let consists = stock.join("TRAINS").join("CONSISTS");
    let trainset = stock.join("TRAINS").join("TRAINSET").join("EMU");
    fs::create_dir_all(&consists).unwrap();
    fs::create_dir_all(&trainset).unwrap();

    fs::write(
        root.join("route.csv"),
        "With Track\n0, .Pitch 5\n100, .Curve 800; 0\n200, .Limit 100\n300, .Curve 0; 0\n",
    )
    .unwrap();
    fs::write(
        consists.join("demo.con"),
        r#"Train ( TrainCfg ( Demo
Engine ( UiD ( 1 ) EngineData ( Motor EMU ) )
Wagon ( WagonData ( Trailer EMU ) UiD ( 2 ) )
) )"#,
    )
    .unwrap();
    fs::write(
        trainset.join("Motor.eng"),
        r#"Wagon (
  Motor
  Name ( "EMU motor" )
  Type ( Engine )
  Mass ( 50t )
  Size ( 3m 4m 21m )
  MaxBrakeForce ( 100kN )
  Engine (
    MaxPower ( 3000kW )
    MaxForce ( 250kN )
    MaxContinuousForce ( 180kN )
    MaxVelocity ( 140km/h )
  )
)"#,
    )
    .unwrap();
    fs::write(
        trainset.join("Trailer.wag"),
        r#"Wagon (
  Trailer
  Name ( "EMU trailer" )
  Type ( Carriage )
  Mass ( 42t )
  Size ( 3m 4m 21m )
  MaxBrakeForce ( 90kN )
)"#,
    )
    .unwrap();

    fs::write(
        root.join("railweave.toml"),
        r#"version = 1

[inputs.route]
source = "route.csv"

[inputs.stock]
source = "stock"

[compose]
network = "route"
assets = ["stock"]
"#,
    )
    .unwrap();

    let composed = compose_manifest(&root.join("railweave.toml")).unwrap();
    assert_eq!(composed.project.network.nodes.len(), 4);
    assert_eq!(composed.project.network.edges.len(), 3);
    assert_eq!(composed.project.consists.len(), 1);
    assert_eq!(composed.project.consists[0].members.len(), 2);
    assert_eq!(composed.project.vehicles.len(), 2);

    let motor_member = &composed.project.consists[0].members[0];
    assert_eq!(motor_member.role, RollingStockRole::Engine);
    let motor = composed
        .project
        .vehicles
        .iter()
        .find(|vehicle| vehicle.asset_id == motor_member.asset_id)
        .unwrap();
    assert_eq!(motor.mass_kg, Some(50_000.0));
    assert_eq!(motor.max_power_w, Some(3_000_000.0));
    assert_eq!(motor.max_tractive_force_n, Some(250_000.0));
    assert_eq!(motor.max_continuous_force_n, Some(180_000.0));
    assert!((motor.max_velocity_mps.unwrap() - 38.888_889_2).abs() < 0.0001);

    let trailer_member = &composed.project.consists[0].members[1];
    assert_eq!(trailer_member.role, RollingStockRole::Wagon);
    let trailer = composed
        .project
        .vehicles
        .iter()
        .find(|vehicle| vehicle.asset_id == trailer_member.asset_id)
        .unwrap();
    assert_eq!(trailer.mass_kg, Some(42_000.0));
    assert_eq!(trailer.max_power_w, None);

    fs::remove_dir_all(root).ok();
}
