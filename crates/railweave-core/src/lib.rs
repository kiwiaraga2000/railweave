use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const IR_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceFormat {
    BveOpenBve,
    MstsOpenRails,
    Trainz,
    RailWorks,
    Loksim3D,
    Unknown,
}

impl fmt::Display for SourceFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::BveOpenBve => "BVE/OpenBVE",
            Self::MstsOpenRails => "MSTS/OpenRails",
            Self::Trainz => "Trainz",
            Self::RailWorks => "Train Simulator / RailWorks",
            Self::Loksim3D => "Loksim3D",
            Self::Unknown => "unknown",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub detector: &'static str,
    pub format: SourceFormat,
    pub confidence: u8,
    pub evidence: Vec<String>,
}

impl Detection {
    pub fn none(detector: &'static str, format: SourceFormat) -> Self {
        Self {
            detector,
            format,
            confidence: 0,
            evidence: Vec::new(),
        }
    }

    pub fn add(&mut self, points: u8, evidence: impl Into<String>) {
        self.confidence = self.confidence.saturating_add(points).min(100);
        self.evidence.push(evidence.into());
    }
}

pub trait SourceDetector: Send + Sync {
    fn id(&self) -> &'static str;
    fn format(&self) -> SourceFormat;
    fn detect(&self, root: &Path) -> Detection;
}

/// Walk a source tree without letting malformed or enormous add-ons make a scan unbounded.
pub fn walk_limited(root: &Path, max_depth: usize, max_entries: usize) -> Vec<PathBuf> {
    fn visit(
        path: &Path,
        depth: usize,
        max_depth: usize,
        max_entries: usize,
        output: &mut Vec<PathBuf>,
    ) {
        if depth > max_depth || output.len() >= max_entries {
            return;
        }

        let Ok(read_dir) = fs::read_dir(path) else {
            return;
        };

        for entry in read_dir.flatten() {
            if output.len() >= max_entries {
                break;
            }

            let path = entry.path();
            output.push(path.clone());
            if path.is_dir() {
                visit(&path, depth + 1, max_depth, max_entries, output);
            }
        }
    }

    let mut output = Vec::new();
    if root.is_dir() {
        visit(root, 0, max_depth, max_entries, &mut output);
    } else if root.exists() {
        output.push(root.to_path_buf());
    }
    output
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub source_format: SourceFormat,
    pub source_path: PathBuf,
    pub source_id: Option<String>,
}

pub type EntityId = u64;

#[derive(Debug, Clone)]
pub struct TrackNode {
    pub id: EntityId,
    pub position: Vec3,
    pub provenance: Option<Provenance>,
}

#[derive(Debug, Clone)]
pub enum Electrification {
    None,
    Overhead { voltage: u32, dc: bool },
    ThirdRail { voltage: u32, dc: bool },
    Other(String),
}

#[derive(Debug, Clone)]
pub struct TrackEdge {
    pub id: EntityId,
    pub from: EntityId,
    pub to: EntityId,
    pub gauge_mm: Option<u32>,
    pub electrification: Option<Electrification>,
    pub speed_limit_kmh: Option<f64>,
    pub provenance: Option<Provenance>,
}

#[derive(Debug, Clone, Default)]
pub struct RailwayNetwork {
    pub nodes: Vec<TrackNode>,
    pub edges: Vec<TrackEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetKind {
    Mesh,
    Texture,
    Sound,
    Cab,
    RollingStock,
    Signal,
    Other,
}

#[derive(Debug, Clone)]
pub struct AssetRef {
    pub id: EntityId,
    pub kind: AssetKind,
    pub name: Option<String>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Default)]
pub struct RailProject {
    pub schema_version: u32,
    pub network: RailwayNetwork,
    pub assets: Vec<AssetRef>,
}

impl RailProject {
    pub fn new() -> Self {
        Self {
            schema_version: IR_SCHEMA_VERSION,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub provenance: Option<Provenance>,
}
