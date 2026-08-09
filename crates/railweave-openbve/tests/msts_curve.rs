use railweave_adapters::import_path;
use railweave_openbve::render_route;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn fixture() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "railweave-msts-curve-{}-{nonce}-{sequence}",
        std::process::id(),
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn exports_tdb_curve_using_tsection_radius_and_tdb_yaw() {
    let root = fixture();
    let openrails = root.join("OPENRAILS");
    fs::create_dir(&openrails).unwrap();
    fs::write(
        openrails.join("tsection.dat"),
        r#"TrackSections ( 8
TrackSection ( 7
    SectionSize ( 1.5 0 )
    SectionCurve ( 100 10 )
)
)"#,
    )
    .unwrap();

    fs::write(
        root.join("curve.tdb"),
        r#"TrackDB (
  TrackNodes ( 3
    TrackNode ( 1
      UiD ( 0 0 1 0 0 0 0 0 0 0 0 0 )
      TrEndNode ( )
      TrPins ( 1 0 TrPin ( 2 1 ) )
    )
    TrackNode ( 2
      TrVectorNode (
        TrVectorSections ( 1
          7 1 0 0 1 0 1 00 0 0 0 0 0 0 0 0
        )
      )
      TrPins ( 1 1 TrPin ( 1 0 ) TrPin ( 3 1 ) )
    )
    TrackNode ( 3
      UiD ( 0 0 2 0 0 0 1.5192247 0 17.3648178 0 0.174532925 0 )
      TrEndNode ( )
      TrPins ( 1 0 TrPin ( 2 0 ) )
    )
  )
)"#,
    )
    .unwrap();

    let imported = import_path(&root).unwrap();
    assert_eq!(imported.project.network.edges.len(), 1);
    let edge = &imported.project.network.edges[0];
    assert!((edge.length_m.unwrap() - 17.45329252).abs() < 0.001);
    assert_eq!(edge.curve_radius_m, Some(100.0));
    assert!(edge
        .provenance
        .as_ref()
        .and_then(|provenance| provenance.source_id.as_deref())
        .unwrap_or_default()
        .contains("geometry=curve"));

    let exported = render_route(&imported.project).unwrap();
    assert!(exported.csv.contains(".Curve 100; 0"));
    assert!(!exported
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "RW412_OPENBVE_UNKNOWN_CURVATURE"));
    assert!(imported
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "RW223_MSTS_TSECTION_CURVES"));

    fs::remove_dir_all(root).ok();
}

#[test]
fn tdb_yaw_overrides_opposite_tsection_angle_sign() {
    let root = fixture();
    let openrails = root.join("OPENRAILS");
    fs::create_dir(&openrails).unwrap();
    fs::write(
        openrails.join("tsection.dat"),
        r#"TrackSections ( 8
TrackSection ( 7
    SectionSize ( 1.5 0 )
    SectionCurve ( 100 -10 )
)
)"#,
    )
    .unwrap();

    fs::write(
        root.join("curve.tdb"),
        r#"TrackDB (
  TrackNodes ( 3
    TrackNode ( 1 UiD ( 0 0 1 0 0 0 0 0 0 0 0 0 ) TrEndNode ( ) TrPins ( 1 0 TrPin ( 2 1 ) ) )
    TrackNode ( 2
      TrVectorNode ( TrVectorSections ( 1
        7 1 0 0 1 0 1 00 0 0 0 0 0 0 0 0
      ) )
      TrPins ( 1 1 TrPin ( 1 0 ) TrPin ( 3 1 ) )
    )
    TrackNode ( 3 UiD ( 0 0 2 0 0 0 1.5192247 0 17.3648178 0 0.174532925 0 ) TrEndNode ( ) TrPins ( 1 0 TrPin ( 2 0 ) ) )
  )
)"#,
    )
    .unwrap();

    let imported = import_path(&root).unwrap();
    assert_eq!(
        imported.project.network.edges[0].curve_radius_m,
        Some(100.0)
    );

    fs::remove_dir_all(root).ok();
}
