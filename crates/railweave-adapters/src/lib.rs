mod bve;
mod detectors;
mod msts;
mod msts_consist;
mod msts_curve;
mod msts_tsection;
mod msts_vehicle;

use railweave_core::{ImportError, ImportResult, SourceFormat};
use std::path::Path;

pub use detectors::{built_in_detectors, detect_all};

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
        SourceFormat::BveOpenBve => bve::import(root),
        SourceFormat::MstsOpenRails => {
            let mut imported = msts::import(root)?;
            msts_consist::enrich_consists(root, &mut imported);
            msts_vehicle::enrich_vehicle_metadata(&mut imported);
            msts_curve::enrich_tdb_curves(root, &mut imported);
            Ok(imported)
        }
        format => Err(ImportError::new(
            "RW003_IMPORT_NOT_IMPLEMENTED",
            format!("{format} was detected, but its source-to-IR importer is not implemented yet"),
        )),
    }
}
