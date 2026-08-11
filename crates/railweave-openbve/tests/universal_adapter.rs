#![cfg(unix)]

use railweave_adapters::import_path;
use railweave_openbve::{export_package, PackageOptions};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn converts_unknown_format_through_discovered_adapter_to_openbve_package() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("railweave-openbve-adapter-{nonce}"));
    let source = root.join("route.privatefmt");
    let adapter_dir = root.join(".railweave").join("adapters");
    fs::create_dir_all(&adapter_dir).unwrap();
    fs::write(&source, b"private route").unwrap();

    let adapter = adapter_dir.join("railweave-adapter-privatefmt");
    fs::write(
        &adapter,
        "#!/bin/sh\nprintf '%s\\n' '{\"project\":{\"schema_version\":1,\"metadata\":{\"title\":\"Private route\",\"description\":null},\"network\":{\"nodes\":[{\"id\":1,\"position\":{\"x\":0.0,\"y\":0.0,\"z\":0.0},\"provenance\":null},{\"id\":2,\"position\":{\"x\":0.0,\"y\":0.0,\"z\":100.0},\"provenance\":null}],\"edges\":[{\"id\":3,\"from\":1,\"to\":2,\"gauge_mm\":1435,\"electrification\":null,\"speed_limit_kmh\":80.0,\"length_m\":100.0,\"curve_radius_m\":null,\"gradient_per_mille\":null,\"provenance\":null}]},\"assets\":[],\"consists\":[],\"vehicles\":[],\"stations\":[]},\"diagnostics\":[]}'\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&adapter).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&adapter, permissions).unwrap();

    let imported = import_path(&source).unwrap();
    let output = root.join("openbve");
    let package = export_package(&imported.project, &output, &PackageOptions::default()).unwrap();

    assert!(package.route_path.is_file());
    assert!(package.train_path.is_file());
    assert!(package.manifest_path.is_file());
    assert!(fs::read_to_string(&package.route_path)
        .unwrap()
        .contains(".Gauge 1435"));
    fs::remove_dir_all(root).ok();
}
