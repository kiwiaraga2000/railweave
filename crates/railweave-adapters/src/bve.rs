use crate::detectors::{bve_route_candidates, decode_text, entries};
use railweave_core::{
    AssetKind, AssetRef, Diagnostic, ImportError, ImportResult, Provenance, RailProject, Severity,
    SourceFormat, TrackEdge, TrackNode, Vec3,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct Command {
    name: String,
    args: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct Event {
    commands: Vec<Command>,
}

#[derive(Debug, Clone, Copy)]
struct GeometryState {
    radius: f64,
    pitch_per_mille: f64,
    speed_limit_kmh: Option<f64>,
}

impl Default for GeometryState {
    fn default() -> Self {
        Self {
            radius: 0.0,
            pitch_per_mille: 0.0,
            speed_limit_kmh: None,
        }
    }
}

fn lower_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn files_named(root: &Path, name: &str) -> Vec<PathBuf> {
    let name = name.to_ascii_lowercase();
    let mut matches: Vec<PathBuf> = entries(root)
        .into_iter()
        .filter(|path| path.is_file() && lower_name(path) == name)
        .collect();
    matches.sort();
    matches
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

fn parse_command(expression: &str, namespace_track: bool) -> Option<Command> {
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

    Some(Command { name, args })
}

fn parse_events(text: &str) -> BTreeMap<i64, (f64, Event)> {
    let mut events: BTreeMap<i64, (f64, Event)> = BTreeMap::new();
    let mut namespace_track = false;
    let mut current_position = 0.0;

    for raw_line in text.lines() {
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

            if let Some(command) = parse_command(&expression, namespace_track) {
                let key = (current_position * 1000.0).round() as i64;
                events
                    .entry(key)
                    .or_insert_with(|| (current_position, Event::default()))
                    .1
                    .commands
                    .push(command);
            }
        }
    }

    events
}

fn command_number(command: &Command, index: usize) -> Option<f64> {
    command
        .args
        .get(index)
        .and_then(|arg| arg.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn integrate(position: Vec3, heading: f64, distance: f64, state: GeometryState) -> (Vec3, f64) {
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

fn max_entity_id(project: &RailProject) -> u64 {
    project
        .network
        .nodes
        .iter()
        .map(|node| node.id)
        .chain(project.network.edges.iter().map(|edge| edge.id))
        .chain(project.assets.iter().map(|asset| asset.id))
        .max()
        .unwrap_or(0)
}

fn asset_name(path: &Path) -> Option<String> {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .or_else(|| {
            path.file_stem()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        })
}

fn add_asset(project: &mut RailProject, next_id: &mut u64, kind: AssetKind, path: PathBuf) {
    project.assets.push(AssetRef {
        id: *next_id,
        kind,
        name: asset_name(&path),
        provenance: Provenance {
            source_format: SourceFormat::BveOpenBve,
            source_path: path,
            source_id: None,
        },
    });
    *next_id = next_id.saturating_add(1);
}

fn import_route(
    route_path: &Path,
    project: &mut RailProject,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), ImportError> {
    let bytes = fs::read(route_path).map_err(|error| {
        ImportError::new(
            "RW101_BVE_READ_FAILED",
            format!("failed to read {}: {error}", route_path.display()),
        )
    })?;
    let text = decode_text(&bytes);
    let events = parse_events(&text);
    if events.is_empty() {
        return Err(ImportError::new(
            "RW102_BVE_NO_TRACK_EVENTS",
            format!(
                "no supported track commands found in {}",
                route_path.display()
            ),
        ));
    }

    project.metadata.title = route_path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    project.metadata.description = Some("Imported from BVE/OpenBVE CSV route geometry".to_string());

    let route_provenance = |source_id: Option<String>| Provenance {
        source_format: SourceFormat::BveOpenBve,
        source_path: route_path.to_path_buf(),
        source_id,
    };

    let mut state = GeometryState::default();
    let mut world = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let mut heading = 0.0;
    let mut previous_position = 0.0;
    let mut next_id = max_entity_id(project).saturating_add(1);
    let first_node_id = next_id;
    next_id += 1;
    project.network.nodes.push(TrackNode {
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
                integrate(world, heading, position - previous_position, state);
            world = new_world;
            heading = new_heading;

            let node_id = next_id;
            next_id += 1;
            project.network.nodes.push(TrackNode {
                id: node_id,
                position: world,
                provenance: Some(route_provenance(Some(format!("track:{position}")))),
            });
            project.network.edges.push(TrackEdge {
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
                "turn" => ignored_turn = true,
                "rail" | "railstart" | "railend" | "switch" => ignored_auxiliary_rails = true,
                _ => {}
            }
        }
    }

    if ignored_auxiliary_rails {
        diagnostics.push(
            Diagnostic::new(
                Severity::Warning,
                "RW110_BVE_AUX_RAILS_NOT_IMPORTED",
                "auxiliary rails and switches are detected but not represented in IR yet",
            )
            .with_provenance(route_provenance(None)),
        );
    }
    if ignored_turn {
        diagnostics.push(
            Diagnostic::new(
                Severity::Warning,
                "RW111_BVE_TURN_NOT_IMPORTED",
                "Track.Turn is not converted yet; generated geometry may differ at those points",
            )
            .with_provenance(route_provenance(None)),
        );
    }
    diagnostics.push(
        Diagnostic::new(
            Severity::Info,
            "RW112_BVE_IMPORT_SCOPE",
            "current BVE route importer converts primary-rail Curve, Pitch and Limit data; stations, signalling, scenery and auxiliary rails are future work",
        )
        .with_provenance(route_provenance(None)),
    );

    Ok(())
}

pub(crate) fn import(root: &Path) -> Result<ImportResult, ImportError> {
    let route_candidates = bve_route_candidates(root);
    let train_files = files_named(root, "train.dat");
    let animated_panels = files_named(root, "panel.animated");
    let legacy_panels = files_named(root, "panel.cfg");
    let sound_configs = files_named(root, "sound.cfg");

    if route_candidates.is_empty()
        && train_files.is_empty()
        && animated_panels.is_empty()
        && legacy_panels.is_empty()
        && sound_configs.is_empty()
    {
        return Err(ImportError::new(
            "RW100_BVE_CONTENT_NOT_FOUND",
            "BVE/OpenBVE was detected, but no supported route or train content was found",
        ));
    }

    let mut result = ImportResult::new(RailProject::new());

    if let Some(route_path) = route_candidates.first() {
        import_route(route_path, &mut result.project, &mut result.diagnostics)?;
        if route_candidates.len() > 1 {
            result.diagnostics.push(Diagnostic::new(
                Severity::Warning,
                "RW103_BVE_MULTIPLE_ROUTES",
                format!(
                    "{} candidate route files found; imported {}",
                    route_candidates.len(),
                    route_path.display()
                ),
            ));
        }
    }

    let mut next_id = max_entity_id(&result.project).saturating_add(1);
    for path in train_files {
        add_asset(
            &mut result.project,
            &mut next_id,
            AssetKind::RollingStock,
            path,
        );
    }
    for path in animated_panels {
        add_asset(&mut result.project, &mut next_id, AssetKind::Cab, path);
    }
    for path in legacy_panels {
        add_asset(&mut result.project, &mut next_id, AssetKind::Cab, path);
    }
    for path in sound_configs {
        add_asset(&mut result.project, &mut next_id, AssetKind::Sound, path);
    }

    if result.project.network.nodes.is_empty() && result.project.metadata.title.is_none() {
        result.project.metadata.title = root
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned);
        result.project.metadata.description =
            Some("Imported from BVE/OpenBVE train content".to_string());
    }

    if !result.project.assets.is_empty() {
        result.diagnostics.push(Diagnostic::new(
            Severity::Info,
            "RW113_BVE_ASSET_IMPORT_SCOPE",
            "BVE train.dat, panel and sound.cfg files are represented as source asset references; deep rolling-stock, cab and sound parsing is future work",
        ));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import_path;
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
    fn imports_primary_track_geometry() {
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
    fn imports_train_asset_references_without_a_route() {
        let root = fixture("bve-train");
        fs::write(root.join("train.dat"), "OPENBVE2000\n").unwrap();
        fs::write(root.join("panel.animated"), "Version 1.0\n").unwrap();
        fs::write(root.join("sound.cfg"), "Version 1.0\n").unwrap();

        let imported = import_path(&root).unwrap();
        assert!(imported.project.network.nodes.is_empty());
        assert_eq!(imported.project.assets.len(), 3);
        assert!(imported
            .project
            .assets
            .iter()
            .any(|asset| asset.kind == AssetKind::RollingStock));
        assert!(imported
            .project
            .assets
            .iter()
            .any(|asset| asset.kind == AssetKind::Cab));
        fs::remove_dir_all(root).ok();
    }
}
