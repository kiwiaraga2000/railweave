use railweave_core::{walk_limited, Detection, SourceDetector, SourceFormat};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_SCAN_DEPTH: usize = 4;
const MAX_SCAN_ENTRIES: usize = 20_000;

pub fn built_in_detectors() -> Vec<Box<dyn SourceDetector>> {
    vec![
        Box::new(MstsDetector),
        Box::new(TrainzDetector),
        Box::new(BveDetector),
        Box::new(RailWorksDetector),
        Box::new(LoksimDetector),
    ]
}

pub fn detect_all(root: &Path) -> Vec<Detection> {
    let mut detections: Vec<Detection> = built_in_detectors()
        .into_iter()
        .map(|detector| detector.detect(root))
        .filter(|detection| detection.confidence > 0)
        .collect();
    detections.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    detections
}

pub(crate) fn entries(root: &Path) -> Vec<PathBuf> {
    if root.is_file() {
        vec![root.to_path_buf()]
    } else {
        walk_limited(root, MAX_SCAN_DEPTH, MAX_SCAN_ENTRIES)
    }
}

fn lower_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn has_file(paths: &[PathBuf], name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    paths
        .iter()
        .any(|path| path.is_file() && lower_name(path) == name)
}

fn has_dir(paths: &[PathBuf], name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    paths
        .iter()
        .any(|path| path.is_dir() && lower_name(path) == name)
}

fn has_extension(paths: &[PathBuf], extension: &str) -> bool {
    paths.iter().any(|path| {
        path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case(extension))
                .unwrap_or(false)
    })
}

pub(crate) fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn read_prefix(path: &Path, max_bytes: usize) -> String {
    let Ok(bytes) = fs::read(path) else {
        return String::new();
    };
    decode_text(&bytes[..bytes.len().min(max_bytes)]).to_ascii_lowercase()
}

pub(crate) fn bve_route_candidates(root: &Path) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = entries(root)
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("csv"))
                .unwrap_or(false)
        })
        .filter(|path| {
            let text = read_prefix(path, 256 * 1024);
            text.contains("with track")
                || text.contains("track.curve")
                || text.contains("track.pitch")
                || text.contains("track.sta")
                || text.contains(".curve ")
                || text.contains(".pitch ")
        })
        .collect();
    candidates.sort();
    candidates
}

pub(crate) fn msts_pat_candidates(root: &Path) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = entries(root)
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("pat"))
                .unwrap_or(false)
        })
        .collect();
    candidates.sort();
    candidates
}

pub struct MstsDetector;

impl SourceDetector for MstsDetector {
    fn id(&self) -> &'static str {
        "msts-openrails"
    }

    fn format(&self) -> SourceFormat {
        SourceFormat::MstsOpenRails
    }

    fn detect(&self, root: &Path) -> Detection {
        let paths = entries(root);
        let mut result = Detection::none(self.id(), self.format());

        if has_extension(&paths, "trk") {
            result.add(45, "found MSTS .trk route file");
        }
        if has_extension(&paths, "tdb") {
            result.add(20, "found track database (.tdb)");
        }
        if has_extension(&paths, "rdb") {
            result.add(10, "found road database (.rdb)");
        }
        if has_dir(&paths, "world") {
            result.add(12, "found WORLD directory");
        }
        if has_dir(&paths, "paths") {
            result.add(8, "found PATHS directory");
        }
        if has_extension(&paths, "pat") {
            result.add(25, "found MSTS path (.pat)");
        }
        if has_dir(&paths, "activities") {
            result.add(5, "found ACTIVITIES directory");
        }
        if has_extension(&paths, "con") {
            result.add(8, "found MSTS consist (.con)");
        }

        result
    }
}

pub struct TrainzDetector;

impl SourceDetector for TrainzDetector {
    fn id(&self) -> &'static str {
        "trainz"
    }

    fn format(&self) -> SourceFormat {
        SourceFormat::Trainz
    }

    fn detect(&self, root: &Path) -> Detection {
        let paths = entries(root);
        let mut result = Detection::none(self.id(), self.format());

        if has_extension(&paths, "cdp") {
            result.add(55, "found Trainz CDP package");
        }

        for config in paths.iter().filter(|path| lower_name(path) == "config.txt") {
            let text = read_prefix(config, 64 * 1024);
            if text.contains("kuid") && text.contains("kind") {
                result.add(75, "found Trainz config.txt with KUID and kind fields");
                break;
            }
        }

        result
    }
}

pub struct BveDetector;

impl SourceDetector for BveDetector {
    fn id(&self) -> &'static str {
        "bve-openbve"
    }

    fn format(&self) -> SourceFormat {
        SourceFormat::BveOpenBve
    }

    fn detect(&self, root: &Path) -> Detection {
        let paths = entries(root);
        let mut result = Detection::none(self.id(), self.format());

        if has_file(&paths, "train.dat") {
            result.add(35, "found train.dat");
        }
        if has_file(&paths, "panel.animated") {
            result.add(25, "found panel.animated");
        } else if has_file(&paths, "panel.cfg") {
            result.add(15, "found panel.cfg");
        }
        if has_file(&paths, "extensions.cfg") {
            result.add(10, "found extensions.cfg");
        }
        if has_dir(&paths, "railway") {
            result.add(20, "found Railway directory");
        }
        if has_dir(&paths, "train") {
            result.add(10, "found Train directory");
        }
        if has_extension(&paths, "rw") {
            result.add(20, "found BVE route (.rw)");
        }
        if !bve_route_candidates(root).is_empty() {
            result.add(45, "found CSV file with BVE/OpenBVE route commands");
        }

        result
    }
}

pub struct RailWorksDetector;

impl SourceDetector for RailWorksDetector {
    fn id(&self) -> &'static str {
        "railworks"
    }

    fn format(&self) -> SourceFormat {
        SourceFormat::RailWorks
    }

    fn detect(&self, root: &Path) -> Detection {
        let paths = entries(root);
        let mut result = Detection::none(self.id(), self.format());

        if has_file(&paths, "routeproperties.xml") {
            result.add(65, "found RouteProperties.xml");
        }
        if has_file(&paths, "tracks.bin") {
            result.add(20, "found Tracks.bin");
        }
        if has_dir(&paths, "routes") && has_dir(&paths, "content") {
            result.add(15, "found Content/Routes-style directory layout");
        }
        if has_dir(&paths, "assets") {
            result.add(5, "found Assets directory");
        }

        result
    }
}

pub struct LoksimDetector;

impl SourceDetector for LoksimDetector {
    fn id(&self) -> &'static str {
        "loksim3d"
    }

    fn format(&self) -> SourceFormat {
        SourceFormat::Loksim3D
    }

    fn detect(&self, root: &Path) -> Detection {
        let paths = entries(root);
        let mut result = Detection::none(self.id(), self.format());

        if has_extension(&paths, "l3dobj") {
            result.add(50, "found Loksim3D object (.l3dobj)");
        }
        if has_extension(&paths, "l3dgrp") {
            result.add(35, "found Loksim3D object group (.l3dgrp)");
        }
        if has_extension(&paths, "l3dpack") {
            result.add(60, "found Loksim3D package (.l3dpack)");
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("railweave-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn detects_msts_route_layout() {
        let root = fixture("msts");
        fs::write(root.join("route.trk"), "").unwrap();
        fs::write(root.join("route.tdb"), "").unwrap();
        fs::create_dir(root.join("WORLD")).unwrap();

        let detection = MstsDetector.detect(&root);
        assert!(detection.confidence >= 70);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn detects_trainz_config() {
        let root = fixture("trainz");
        fs::write(root.join("config.txt"), "kuid <kuid:1:2>\nkind track\n").unwrap();

        let detection = TrainzDetector.detect(&root);
        assert!(detection.confidence >= 70);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn detects_openbve_train() {
        let root = fixture("openbve");
        fs::write(root.join("train.dat"), "OPENBVE2000\n").unwrap();
        fs::write(root.join("panel.animated"), "Version 1.0\n").unwrap();

        let detection = BveDetector.detect(&root);
        assert!(detection.confidence >= 50);
        fs::remove_dir_all(root).ok();
    }
}
