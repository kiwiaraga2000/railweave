use crate::detectors::{bve_route_candidates, decode_text};
use railweave_core::{
    Diagnostic, ImportError, ImportResult, Provenance, RailProject, Severity, SourceFormat, TrackEdge,
    TrackNode, Vec3,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

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

fn integrate(
    position: Vec3,
    heading: f64,
    distance: f64,
    state: GeometryState,
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

pub(crate) fn import(root: &Path) -> Result<ImportResult, ImportError> {
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

    let mut state = GeometryState::default();
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
                integrate(world, heading, position - previous_position, state);
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
                "turn" => ignored_turn = true,
                "rail" | "railstart" | "railend" | "switch" => ignored_auxiliary_rails = true,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import_path;
    use std::path::PathBuf;
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
}
