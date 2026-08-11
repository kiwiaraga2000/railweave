mod bve;
mod detectors;
mod interchange;
mod msts;
mod msts_consist;
mod msts_curve;
mod msts_tsection;
mod msts_vehicle;
mod trainz_config;

use railweave_core::{
    Diagnostic, ImportError, ImportResult, Severity, SourceFormat, IR_SCHEMA_VERSION,
};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use detectors::{built_in_detectors, detect_all};
pub use trainz_config::{
    parse_trainz_config, TrainzConfig, TrainzConfigDiagnostic, TrainzConfigParse,
};

fn adapter_keys(root: &Path) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(best) = detect_all(root).first() {
        keys.push(best.detector.to_string());
    }
    if let Some(extension) = root.extension().and_then(|value| value.to_str()) {
        let normalized: String = extension
            .chars()
            .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
            .flat_map(char::to_lowercase)
            .collect();
        if !normalized.is_empty() && !keys.contains(&normalized) {
            keys.push(normalized);
        }
    }
    keys
}

fn adapter_names(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for key in adapter_keys(root) {
        names.push(format!("railweave-adapter-{key}"));
        names.push(format!("railweave-{key}"));
    }
    names.push("railweave-adapter".to_string());
    names
}

fn executable_in(directory: &Path, name: &str) -> Option<PathBuf> {
    let candidate = directory.join(name);
    if candidate.is_file() {
        return Some(candidate);
    }
    #[cfg(windows)]
    {
        let candidate = directory.join(format!("{name}.exe"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Discover a source adapter without requiring a format-specific CLI flag.
pub fn discover_external_adapter(source: &Path) -> Option<PathBuf> {
    let names = adapter_names(source);
    let source_base = if source.is_dir() {
        source
    } else {
        source.parent().unwrap_or_else(|| Path::new("."))
    };
    let mut directories = vec![source_base.join(".railweave").join("adapters")];
    if let Some(configured) = env::var_os("RAILWEAVE_ADAPTER_PATH") {
        directories.extend(env::split_paths(&configured));
    }
    if let Some(path) = env::var_os("PATH") {
        directories.extend(env::split_paths(&path));
    }
    directories
        .iter()
        .find_map(|directory| names.iter().find_map(|name| executable_in(directory, name)))
}

/// Run a user-supplied source adapter using the stable RailWeave adapter protocol.
///
/// The executable receives the source path as its sole positional argument and
/// must write one `ImportResult` JSON document to stdout. Diagnostics and logs
/// belong on stderr. This boundary lets proprietary or community formats evolve
/// outside the core binary without creating pairwise converter code.
pub fn import_external(adapter: &Path, source: &Path) -> Result<ImportResult, ImportError> {
    if !adapter.exists() {
        return Err(ImportError::new(
            "RW004_ADAPTER_NOT_FOUND",
            format!("external adapter does not exist: {}", adapter.display()),
        ));
    }
    let output = Command::new(adapter)
        .arg(source)
        .env("RAILWEAVE_ADAPTER_PROTOCOL", "1")
        .env("RAILWEAVE_IR_SCHEMA", IR_SCHEMA_VERSION.to_string())
        .output()
        .map_err(|error| {
            ImportError::new(
                "RW005_ADAPTER_LAUNCH_FAILED",
                format!("failed to launch {}: {error}", adapter.display()),
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ImportError::new(
            "RW006_ADAPTER_FAILED",
            format!(
                "adapter {} exited with {}: {}",
                adapter.display(),
                output.status,
                stderr.trim()
            ),
        ));
    }
    if output.stdout.len() > 64 * 1024 * 1024 {
        return Err(ImportError::new(
            "RW007_ADAPTER_OUTPUT_LIMIT",
            "external adapter output exceeds the 64 MiB protocol limit",
        ));
    }
    let mut result: ImportResult = serde_json::from_slice(&output.stdout).map_err(|error| {
        ImportError::new(
            "RW008_ADAPTER_OUTPUT_INVALID",
            format!("adapter returned invalid RailWeave JSON: {error}"),
        )
    })?;
    if result.project.schema_version != IR_SCHEMA_VERSION {
        return Err(ImportError::new(
            "RW009_ADAPTER_SCHEMA",
            format!(
                "adapter returned IR schema {}, expected {}",
                result.project.schema_version, IR_SCHEMA_VERSION
            ),
        ));
    }
    result.diagnostics.push(Diagnostic::new(
        Severity::Info,
        "RW010_EXTERNAL_ADAPTER",
        format!(
            "source imported through external adapter {}",
            adapter.display()
        ),
    ));
    Ok(result)
}

fn import_built_in(root: &Path) -> Result<ImportResult, ImportError> {
    if !root.exists() {
        return Err(ImportError::new(
            "RW001_PATH_NOT_FOUND",
            format!("path does not exist: {}", root.display()),
        ));
    }

    let detections = detect_all(root);
    let Some(best) = detections.first() else {
        return Err(ImportError::new(
            "RW002_UNKNOWN_FORMAT",
            format!(
                "no supported simulator format detected at {}",
                root.display()
            ),
        ));
    };

    match best.format {
        SourceFormat::RailWeave => interchange::import_ir(root),
        SourceFormat::GeoJson => interchange::import_geojson(root),
        SourceFormat::TrackCsv => interchange::import_track_csv(root),
        SourceFormat::BveOpenBve => bve::import(root),
        SourceFormat::MstsOpenRails => {
            let mut imported = msts::import(root)?;
            msts_consist::enrich_consists(root, &mut imported);
            msts_vehicle::enrich_vehicle_metadata(&mut imported);
            msts_curve::enrich_tdb_curves(root, &mut imported);
            Ok(imported)
        }
        format => interchange::import_game_bridge(root, format),
    }
}

/// Import with a built-in importer, portable bridge, or discovered adapter.
pub fn import_path(root: &Path) -> Result<ImportResult, ImportError> {
    match import_built_in(root) {
        Ok(imported) => Ok(imported),
        Err(error)
            if matches!(
                error.code,
                "RW002_UNKNOWN_FORMAT" | "RW003_IMPORT_NEEDS_ADAPTER"
            ) =>
        {
            if let Some(adapter) = discover_external_adapter(root) {
                import_external(&adapter, root)
            } else {
                let expected = adapter_names(root).join(", ");
                Err(ImportError::new(
                    error.code,
                    format!(
                        "{}. No compatible adapter was discovered; install one of [{expected}] in .railweave/adapters, RAILWEAVE_ADAPTER_PATH, or PATH",
                        error.message
                    ),
                ))
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod adapter_discovery_tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn discovers_extension_adapter_next_to_unknown_source() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("railweave-adapter-{nonce}"));
        let source = root.join("route.customfmt");
        let adapters = root.join(".railweave").join("adapters");
        fs::create_dir_all(&adapters).unwrap();
        fs::write(&source, b"unknown route format").unwrap();
        let expected = adapters.join("railweave-adapter-customfmt");
        fs::write(&expected, b"adapter placeholder").unwrap();

        assert_eq!(discover_external_adapter(&source), Some(expected));
        fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn unknown_format_runs_discovered_adapter() {
        use std::os::unix::fs::PermissionsExt;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("railweave-adapter-e2e-{nonce}"));
        let source = root.join("route.privatefmt");
        let adapters = root.join(".railweave").join("adapters");
        fs::create_dir_all(&adapters).unwrap();
        fs::write(&source, b"private route").unwrap();
        let adapter = adapters.join("railweave-adapter-privatefmt");
        fs::write(
            &adapter,
            "#!/bin/sh\nprintf '%s\\n' '{\"project\":{\"schema_version\":1,\"metadata\":{\"title\":\"Adapter route\",\"description\":null},\"network\":{\"nodes\":[],\"edges\":[]},\"assets\":[],\"consists\":[],\"vehicles\":[],\"stations\":[]},\"diagnostics\":[]}'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&adapter).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&adapter, permissions).unwrap();

        let imported = import_path(&source).unwrap();
        assert_eq!(
            imported.project.metadata.title.as_deref(),
            Some("Adapter route")
        );
        assert!(imported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RW010_EXTERNAL_ADAPTER"));
        fs::remove_dir_all(root).ok();
    }
}
