use railweave_core::{
    walk_limited, Detection, Diagnostic, ImportError, ImportResult, Provenance, RailProject,
    Severity, SourceDetector, SourceFormat, TrackEdge, TrackNode, Vec3,
};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_SCAN_DEPTH: usize = 4;
const MAX_SCAN_ENTRIES: usize = 20_000;
const MSTS_TILE_SIZE: f64 = 2048.0;

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
            format!("no supported simulator format detected at {}", root.display()),
        ));
    };

    match best.format {
        SourceFormat::BveOpenBve => import_bve(root),
        SourceFormat::MstsOpenRails => import_msts(root),
        format => Err(ImportError::new(
            "RW003_IMPORT_NOT_IMPLEMENTED",
            format!(
                "{format} was detected, but its source-to-IR importer is not implemented yet"
            ),
        )),
    }
}

fn entries(root: &Path) -> Vec<PathBuf> {
    walk_limited(root, MAX_SCAN_DEPTH, MAX_SCAN_ENTRIES)
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

fn read_prefix(path: &Path, max_bytes: usize) -> String {
    let Ok(bytes) = fs::read(path) else {
        return String::new();
    };
    decode_text(&bytes[..bytes.len().min(max_bytes)]).to_ascii_lowercase()
}

fn decode_text(bytes: &[u8]) -> String {
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

        if bve_route_candidates(root).iter().any(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("csv"))
                .unwrap_or(false)
        }) {
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

#[derive(Debug, Clone)]
struct BveCommand {
    line: usize,
    name: String,
    args: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct BveEvent {
    commands: Vec<BveCommand>,
}

#[derive(Debug, Clone, Copy)]
struct BveGeometryState {
    radius: f64,
    pitch_per_mille: f64,
    speed_limit_kmh: Option<f64>,
}

impl Default for BveGeometryState {
    fn default() -> Self {
        Self {
            radius: 0.0,
            pitch_per_mille: 0.0,
            speed_limit_kmh: None,
        }
    }
}

fn bve_route_candidates(root: &Path) -> Vec<PathBuf> {
    let paths = if root.is_file() {
        vec![root.to_path_buf()]
    } else {
        entries(root)
    };

    let mut candidates: Vec<PathBuf> = paths
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

fn split_quoted(input: &str, separator: char) -> Vec<String> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut quoted = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                current.push(ch);
            }
            _ if ch == separator && !quoted => {
                output.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    output.push(current.trim().to_string());
    output
}

fn parse_bve_command(expression: &str, namespace_track: bool, line: usize) -> Option<BveCommand> {
    let expression = expression.trim();
    if expression.is_empty() {
        return None;
    }

    let lower = expression.to_ascii_lowercase();
    let is_track = lower.starts_with("track.") || (namespace_track && lower.starts_with('.'));
    if !is_track {
        return None;
    }

    let without_prefix = if lower.starts_with("track.") {
        &expression[6..]
    } else {
        expression.trim_start_matches('.')
    };

    let split_at = without_prefix
        .find(|ch: char| ch.is_whitespace() || ch == '=' || ch == '(')
        .unwrap_or(without_prefix.len());
    let name = without_prefix[..split_at].trim().to_ascii_lowercase();
    let mut remainder = without_prefix[split_at..].trim();

    if let Some(stripped) = remainder.strip_prefix('=') {
        remainder = stripped.trim();
    }
    if remainder.starts_with('(') && remainder.ends_with(')') && remainder.len() >= 2 {
        remainder = &remainder[1..remainder.len() - 1];
    }

    let args = split_quoted(remainder, ';')
        .into_iter()
        .map(|arg| arg.trim_matches('"').trim().to_string())
        .collect();

    Some(BveCommand { line, name, args })
}

fn parse_bve_events(text: &str) -> BTreeMap<i64, (f64, BveEvent)> {
    let mut events: BTreeMap<i64, (f64, BveEvent)> = BTreeMap::new();
    let mut namespace_track = false;
    let mut current_position = 0.0;

    for (line_index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('\'') {
            continue;
        }

        let lower = line.to_ascii_lowercase();
        if lower == "with track" {
            namespace_track = true;
            continue;
        }
        if lower.starts_with("with ") {
            namespace_track = false;
            continue;
        }

        for expression in split_quoted(line, ',') {
            if let Ok(position) = expression.trim().parse::<f64>() {
                if position.is_finite() && position >= 0.0 {
                    current_position = position;
                }
                continue;
            }

            if let Some(command) = parse_bve_command(&expression, namespace_track, line_index + 1) {
                let key = (current_position * 1000.0).round() as i64;
                events
                    .entry(key)
                    .or_insert_with(|| (current_position, BveEvent::default()))
                    .1
                    .commands
                    .push(command);
            }
        }
    }

    events
}

fn command_number(command: &BveCommand, index: usize) -> Option<f64> {
    command
        .args
        .get(index)
        .and_then(|arg| arg.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn integrate_bve(
    position: Vec3,
    heading: f64,
    distance: f64,
    state: BveGeometryState,
) -> (Vec3, f64) {
    let dy = distance * state.pitch_per_mille / 1000.0;
    if state.radius.abs() < 1e-9 {
        return (
            Vec3 {
                x: position.x + distance * heading.sin(),
                y: position.y + dy,
                z: position.z + distance * heading.cos(),
            },
            heading,
        );
    }

    let new_heading = heading + distance / state.radius;
    (
        Vec3 {
            x: position.x + state.radius * (heading.cos() - new_heading.cos()),
            y: position.y + dy,
            z: position.z + state.radius * (new_heading.sin() - heading.sin()),
        },
        new_heading,
    )
}

fn import_bve(root: &Path) -> Result<ImportResult, ImportError> {
    let candidates = bve_route_candidates(root);
    let Some(route_path) = candidates.first() else {
        return Err(ImportError::new(
            "RW100_BVE_ROUTE_NOT_FOUND",
            "BVE/OpenBVE was detected, but no supported CSV route file was found",
        ));
    };

    let bytes = fs::read(route_path).map_err(|error| {
        ImportError::new(
            "RW101_BVE_READ_FAILED",
            format!("failed to read {}: {error}", route_path.display()),
        )
    })?;
    let text = decode_text(&bytes);
    let events = parse_bve_events(&text);
    if events.is_empty() {
        return Err(ImportError::new(
            "RW102_BVE_NO_TRACK_EVENTS",
            format!("no supported track commands found in {}", route_path.display()),
        ));
    }

    let mut project = RailProject::new();
    project.metadata.title = route_path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    project.metadata.description = Some("Imported from BVE/OpenBVE CSV route geometry".to_string());

    let mut result = ImportResult::new(project);
    if candidates.len() > 1 {
        result.diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW103_BVE_MULTIPLE_ROUTES",
            format!(
                "{} candidate route files found; imported {}",
                candidates.len(),
                route_path.display()
            ),
        ));
    }

    let route_provenance = |source_id: Option<String>| Provenance {
        source_format: SourceFormat::BveOpenBve,
        source_path: route_path.clone(),
        source_id,
    };

    let mut state = BveGeometryState::default();
    let mut world = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let mut heading = 0.0;
    let mut previous_position = 0.0;
    let mut next_id = 1_u64;
    let first_node_id = next_id;
    next_id += 1;
    result.project.network.nodes.push(TrackNode {
        id: first_node_id,
        position: world,
        provenance: Some(route_provenance(Some("track:0".to_string()))),
    });
    let mut previous_node_id = first_node_id;
    let mut ignored_auxiliary_rails = false;
    let mut ignored_turn = false;

    for (_, (position, event)) in events {
        if position > previous_position {
            let edge_state = state;
            let (new_world, new_heading) =
                integrate_bve(world, heading, position - previous_position, state);
            world = new_world;
            heading = new_heading;

            let node_id = next_id;
            next_id += 1;
            result.project.network.nodes.push(TrackNode {
                id: node_id,
                position: world,
                provenance: Some(route_provenance(Some(format!("track:{position}")))),
            });
            result.project.network.edges.push(TrackEdge {
                id: next_id,
                from: previous_node_id,
                to: node_id,
                gauge_mm: None,
                electrification: None,
                speed_limit_kmh: edge_state.speed_limit_kmh,
                provenance: Some(route_provenance(Some(format!(
                    "segment:{previous_position}-{position}"
                )))),
            });
            next_id += 1;
            previous_node_id = node_id;
            previous_position = position;
        }

        for command in event.commands {
            match command.name.as_str() {
                "curve" => {
                    if let Some(radius) = command_number(&command, 0) {
                        state.radius = radius;
                    }
                }
                "pitch" => {
                    if let Some(pitch) = command_number(&command, 0) {
                        state.pitch_per_mille = pitch;
                    }
                }
                "limit" => {
                    if let Some(speed) = command_number(&command, 0) {
                        state.speed_limit_kmh = if speed > 0.0 { Some(speed) } else { None };
                    }
                }
                "turn" => {
                    ignored_turn = true;
                }
                "rail" | "railstart" | "railend" | "switch" => {
                    ignored_auxiliary_rails = true;
                }
                _ => {}
            }
        }
    }

    if ignored_auxiliary_rails {
        result.diagnostics.push(
            Diagnostic::new(
                Severity::Warning,
                "RW110_BVE_AUX_RAILS_NOT_IMPORTED",
                "auxiliary rails and switches are detected but not represented in IR yet",
            )
            .with_provenance(route_provenance(None)),
        );
    }
    if ignored_turn {
        result.diagnostics.push(
            Diagnostic::new(
                Severity::Warning,
                "RW111_BVE_TURN_NOT_IMPORTED",
                "Track.Turn is not converted yet; generated geometry may differ at those points",
            )
            .with_provenance(route_provenance(None)),
        );
    }
    result.diagnostics.push(
        Diagnostic::new(
            Severity::Info,
            "RW112_BVE_IMPORT_SCOPE",
            "current BVE importer converts primary-rail Curve, Pitch and Limit data; stations, signalling, scenery and auxiliary rails are future work",
        )
        .with_provenance(route_provenance(None)),
    );

    Ok(result)
}

#[derive(Debug, Clone)]
struct MstsPdp {
    tile_x: i32,
    tile_z: i32,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Clone)]
struct MstsPathNode {
    next_main: u32,
    next_siding: u32,
    pdp_index: u32,
}

fn stf_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        if ch == '(' || ch == ')' {
            tokens.push(ch.to_string());
            continue;
        }
        if ch == '"' {
            let mut value = String::new();
            while let Some(next) = chars.next() {
                if next == '"' {
                    break;
                }
                value.push(next);
            }
            tokens.push(value);
            continue;
        }

        let mut value = String::new();
        value.push(ch);
        while let Some(next) = chars.peek().copied() {
            if next.is_whitespace() || next == '(' || next == ')' {
                break;
            }
            value.push(next);
            chars.next();
        }
        tokens.push(value);
    }

    tokens
}

fn parse_i32(token: Option<&String>) -> Option<i32> {
    token?.parse().ok()
}

fn parse_u32(token: Option<&String>) -> Option<u32> {
    token?.parse().ok()
}

fn parse_f64(token: Option<&String>) -> Option<f64> {
    token?.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn parse_msts_pat(text: &str) -> (Vec<MstsPdp>, Vec<MstsPathNode>, Option<String>) {
    let tokens = stf_tokens(text);
    let mut pdps = Vec::new();
    let mut nodes = Vec::new();
    let mut path_name = None;
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index].to_ascii_lowercase().as_str() {
            "trackpdp" if tokens.get(index + 1).map(String::as_str) == Some("(") => {
                let values = (
                    parse_i32(tokens.get(index + 2)),
                    parse_i32(tokens.get(index + 3)),
                    parse_f64(tokens.get(index + 4)),
                    parse_f64(tokens.get(index + 5)),
                    parse_f64(tokens.get(index + 6)),
                );
                if let (Some(tile_x), Some(tile_z), Some(x), Some(y), Some(z)) = values {
                    pdps.push(MstsPdp {
                        tile_x,
                        tile_z,
                        x,
                        y,
                        z,
                    });
                }
            }
            "trpathnode" if tokens.get(index + 1).map(String::as_str) == Some("(") => {
                let next_main = parse_u32(tokens.get(index + 3));
                let next_siding = parse_u32(tokens.get(index + 4));
                let pdp_index = parse_u32(tokens.get(index + 5));
                if let (Some(next_main), Some(next_siding), Some(pdp_index)) =
                    (next_main, next_siding, pdp_index)
                {
                    nodes.push(MstsPathNode {
                        next_main,
                        next_siding,
                        pdp_index,
                    });
                }
            }
            "name" if tokens.get(index + 1).map(String::as_str) == Some("(") => {
                if path_name.is_none() {
                    path_name = tokens.get(index + 2).cloned();
                }
            }
            _ => {}
        }
        index += 1;
    }

    (pdps, nodes, path_name)
}

fn msts_pat_candidates(root: &Path) -> Vec<PathBuf> {
    let paths = if root.is_file() {
        vec![root.to_path_buf()]
    } else {
        entries(root)
    };
    let mut paths: Vec<PathBuf> = paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("pat"))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();
    paths
}

fn import_msts(root: &Path) -> Result<ImportResult, ImportError> {
    let candidates = msts_pat_candidates(root);
    let Some(path_file) = candidates.first() else {
        return Err(ImportError::new(
            "RW200_MSTS_PATH_NOT_FOUND",
            "MSTS/OpenRails was detected, but no .pat file was found; full .tdb import is not implemented yet",
        ));
    };

    let bytes = fs::read(path_file).map_err(|error| {
        ImportError::new(
            "RW201_MSTS_READ_FAILED",
            format!("failed to read {}: {error}", path_file.display()),
        )
    })?;
    let text = decode_text(&bytes);

    if !text.to_ascii_lowercase().contains("trackpdps") {
        return Err(ImportError::new(
            "RW202_MSTS_PAT_UNSUPPORTED",
            format!(
                "{} is not a textual/UTF-16 MSTS PAT file that RailWeave can parse yet",
                path_file.display()
            ),
        ));
    }

    let (pdps, path_nodes, path_name) = parse_msts_pat(&text);
    if pdps.is_empty() || path_nodes.is_empty() {
        return Err(ImportError::new(
            "RW203_MSTS_PAT_EMPTY",
            format!(
                "could not extract TrackPDPs and TrPathNodes from {}",
                path_file.display()
            ),
        ));
    }

    let mut project = RailProject::new();
    project.metadata.title = path_name.or_else(|| {
        path_file
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    });
    project.metadata.description =
        Some("Imported from MSTS/OpenRails PAT path topology".to_string());
    let mut result = ImportResult::new(project);

    if candidates.len() > 1 {
        result.diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW204_MSTS_MULTIPLE_PATHS",
            format!(
                "{} PAT files found; imported {}. Pass a specific .pat file to choose another path.",
                candidates.len(),
                path_file.display()
            ),
        ));
    }

    let provenance = |source_id: Option<String>| Provenance {
        source_format: SourceFormat::MstsOpenRails,
        source_path: path_file.clone(),
        source_id,
    };

    let origin = &pdps[path_nodes
        .iter()
        .find_map(|node| pdps.get(node.pdp_index as usize).map(|_| node.pdp_index as usize))
        .unwrap_or(0)];
    let mut node_ids = Vec::with_capacity(path_nodes.len());
    let mut next_id = 1_u64;

    for (index, path_node) in path_nodes.iter().enumerate() {
        let Some(pdp) = pdps.get(path_node.pdp_index as usize) else {
            node_ids.push(None);
            result.diagnostics.push(
                Diagnostic::new(
                    Severity::Warning,
                    "RW205_MSTS_BAD_PDP_REFERENCE",
                    format!(
                        "TrPathNode {index} references missing TrackPDP {}",
                        path_node.pdp_index
                    ),
                )
                .with_provenance(provenance(Some(format!("TrPathNode:{index}")))),
            );
            continue;
        };

        let id = next_id;
        next_id += 1;
        node_ids.push(Some(id));
        result.project.network.nodes.push(TrackNode {
            id,
            position: Vec3 {
                x: (pdp.tile_x - origin.tile_x) as f64 * MSTS_TILE_SIZE + pdp.x - origin.x,
                y: pdp.y - origin.y,
                z: (pdp.tile_z - origin.tile_z) as f64 * MSTS_TILE_SIZE + pdp.z - origin.z,
            },
            provenance: Some(provenance(Some(format!("TrPathNode:{index}")))),
        });
    }

    let mut emitted = HashSet::new();
    for (index, path_node) in path_nodes.iter().enumerate() {
        let Some(from_id) = node_ids.get(index).and_then(|id| *id) else {
            continue;
        };

        for (kind, target) in [
            ("main", path_node.next_main),
            ("siding", path_node.next_siding),
        ] {
            if target == u32::MAX {
                continue;
            }
            let target_index = target as usize;
            let Some(to_id) = node_ids.get(target_index).and_then(|id| *id) else {
                result.diagnostics.push(
                    Diagnostic::new(
                        Severity::Warning,
                        "RW206_MSTS_BAD_PATH_REFERENCE",
                        format!("TrPathNode {index} has invalid {kind} link to {target}"),
                    )
                    .with_provenance(provenance(Some(format!("TrPathNode:{index}")))),
                );
                continue;
            };

            if !emitted.insert((from_id, to_id)) {
                continue;
            }

            result.project.network.edges.push(TrackEdge {
                id: next_id,
                from: from_id,
                to: to_id,
                gauge_mm: None,
                electrification: None,
                speed_limit_kmh: None,
                provenance: Some(provenance(Some(format!(
                    "TrPathNode:{index}:{kind}"
                )))),
            });
            next_id += 1;
        }
    }

    result.diagnostics.push(
        Diagnostic::new(
            Severity::Info,
            "RW207_MSTS_IMPORT_SCOPE",
            "current MSTS/OpenRails importer converts PAT waypoint topology using 2048 m MSTS tiles; full TDB geometry, track sections, signalling and world scenery are future work",
        )
        .with_provenance(provenance(None)),
    );

    Ok(result)
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

    #[test]
    fn imports_bve_primary_track_geometry() {
        let root = fixture("bve-import");
        let route = root.join("route.csv");
        fs::write(
            &route,
            "With Track\n0, .Pitch 10\n100, .Curve 500; 0\n200, .Limit 80\n300, .Curve 0; 0\n",
        )
        .unwrap();

        let imported = import_path(&route).unwrap();
        assert_eq!(imported.project.network.nodes.len(), 4);
        assert_eq!(imported.project.network.edges.len(), 3);
        assert!(imported.project.network.nodes[1].position.y > 0.9);
        assert_eq!(
            imported.project.network.edges[2].speed_limit_kmh,
            Some(80.0)
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn imports_msts_pat_topology() {
        let root = fixture("msts-import");
        let paths = root.join("PATHS");
        fs::create_dir(&paths).unwrap();
        let pat = paths.join("demo.pat");
        fs::write(
            &pat,
            r#"SIMISA@@@@@@@@@@JINX0P0t______
TrackPDPs (
  TrackPDP ( 10 20 0 100 0 1 1 )
  TrackPDP ( 10 20 100 101 0 2 0 )
  TrackPDP ( 11 20 -900 102 0 1 1 )
)
TrackPath (
  Name ( "Synthetic path" )
  TrPathNodes ( 3
    TrPathNode ( 00000000 1 4294967295 0 )
    TrPathNode ( 00000000 2 4294967295 1 )
    TrPathNode ( 00000000 4294967295 4294967295 2 )
  )
)
"#,
        )
        .unwrap();

        let imported = import_path(&root).unwrap();
        assert_eq!(
            imported.project.metadata.title.as_deref(),
            Some("Synthetic path")
        );
        assert_eq!(imported.project.network.nodes.len(), 3);
        assert_eq!(imported.project.network.edges.len(), 2);
        assert!((imported.project.network.nodes[2].position.x - 1148.0).abs() < 0.001);
        fs::remove_dir_all(root).ok();
    }
}
