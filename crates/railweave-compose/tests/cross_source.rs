use railweave_compose::compose_manifest;
use railweave_core::{AssetKind, SourceFormat};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "railweave-cross-source-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn composes_bve_route_with_msts_rolling_stock() {
    let root = fixture();

    fs::write(
        root.join("route.csv"),
        "With Track\n0, .Pitch 5\n100, .Curve 800; 0\n200, .Limit 100\n300, .Curve 0; 0\n",
    )
    .unwrap();
    fs::write(
        root.join("ED4M.con"),
        "SIMISA@@@@@@@@@@JINX0D0t______\nTrainCfg ( ED4M )\n",
    )
    .unwrap();
    fs::write(
        root.join("railweave.toml"),
        r#"version = 1

[inputs.route]
source = "route.csv"

[inputs.stock]
source = "ED4M.con"

[compose]
network = "route"
assets = ["stock"]
"#,
    )
    .unwrap();

    let composed = compose_manifest(&root.join("railweave.toml")).unwrap();

    assert_eq!(composed.project.network.nodes.len(), 4);
    assert_eq!(composed.project.network.edges.len(), 3);
    assert_eq!(composed.project.assets.len(), 1);
    assert_eq!(composed.project.assets[0].kind, AssetKind::RollingStock);
    assert_eq!(composed.project.assets[0].name.as_deref(), Some("ED4M"));
    assert_eq!(
        composed.project.assets[0].provenance.source_format,
        SourceFormat::MstsOpenRails
    );
    assert!(composed.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RW300_COMPOSED" && diagnostic.message.contains("network from \"route\"")
    }));

    fs::remove_dir_all(root).ok();
}
