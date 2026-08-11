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
use std::path::Path;
use std::process::Command;

pub use detectors::{built_in_detectors, detect_all};
pub use trainz_config::{parse_trainz_config, TrainzConfig, TrainzConfigDiagnostic, TrainzConfigParse};

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

pub fn import_path(root: &Path) -> Result<ImportResult, ImportError> {
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
