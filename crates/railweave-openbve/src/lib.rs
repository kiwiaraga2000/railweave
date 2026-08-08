use railweave_core::{Diagnostic, EntityId, RailProject, Severity, SourceFormat, TrackEdge, Vec3};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;

const BLOCK_LENGTH_M: f64 = 1.0;
const EPSILON: f64 = 1.0e-9;

#[derive(Debug, Clone)]
pub struct ExportedRoute {
    pub csv: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub struct ExportError {
    pub code: &'static str,
    pub message: String,
}

impl ExportError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ExportError {}

fn is_main_edge(edge: &TrackEdge) -> bool {
    edge.provenance
        .as_ref()
        .and_then(|provenance| provenance.source_id.as_deref())
        .map(|source_id| source_id.ends_with(":main"))
        .unwrap_or(false)
}

fn select_driveable_path<'a>(
    project: &'a RailProject,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<&'a TrackEdge>, ExportError> {
    if project.network.edges.is_empty() {
        return Err(ExportError::new(
            "RW400_OPENBVE_EMPTY_NETWORK",
            "the project has no track edges to export",
        ));
    }

    let node_ids: BTreeSet<EntityId> = project.network.nodes.iter().map(|node| node.id).collect();
    let mut outgoing: BTreeMap<EntityId, Vec<&TrackEdge>> = BTreeMap::new();
    let mut indegree: BTreeMap<EntityId, usize> = BTreeMap::new();

    for edge in &project.network.edges {
        if !node_ids.contains(&edge.from) || !node_ids.contains(&edge.to) {
            return Err(ExportError::new(
                "RW401_OPENBVE_DANGLING_EDGE",
                format!(
                    "edge {} references missing node {} -> {}",
                    edge.id, edge.from, edge.to
                ),
            ));
        }
        outgoing.entry(edge.from).or_default().push(edge);
        *indegree.entry(edge.to).or_default() += 1;
        indegree.entry(edge.from).or_default();
    }

    for edges in outgoing.values_mut() {
        edges.sort_by_key(|edge| edge.id);
    }

    let starts: Vec<EntityId> = outgoing
        .keys()
        .copied()
        .filter(|node| indegree.get(node).copied().unwrap_or(0) == 0)
        .collect();

    let Some(&start) = starts.first() else {
        return Err(ExportError::new(
            "RW402_OPENBVE_NO_PATH_START",
            "the track graph has no unambiguous entry node; cyclic networks need an explicit service/path selector",
        ));
    };

    if starts.len() > 1 {
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW403_OPENBVE_MULTIPLE_STARTS",
            format!(
                "{} possible path starts found; selected node {} deterministically",
                starts.len(),
                start
            ),
        ));
    }

    let mut current = start;
    let mut selected = Vec::new();
    let mut selected_ids = HashSet::new();
    let mut visited_nodes = HashSet::new();

    loop {
        if !visited_nodes.insert(current) {
            return Err(ExportError::new(
                "RW404_OPENBVE_PATH_LOOP",
                format!("selected driveable path loops at node {current}"),
            ));
        }

        let Some(candidates) = outgoing.get(&current) else {
            break;
        };
        if candidates.is_empty() {
            break;
        }

        let chosen = if candidates.len() == 1 {
            candidates[0]
        } else {
            let main_edges: Vec<&TrackEdge> = candidates
                .iter()
                .copied()
                .filter(|edge| is_main_edge(edge))
                .collect();
            let chosen = if main_edges.len() == 1 {
                main_edges[0]
            } else {
                candidates[0]
            };
            diagnostics.push(Diagnostic::new(
                Severity::Warning,
                "RW405_OPENBVE_BRANCH_SELECTED",
                format!(
                    "node {} has {} outgoing edges; selected edge {}{}",
                    current,
                    candidates.len(),
                    chosen.id,
                    if main_edges.len() == 1 {
                        " because it is marked as the main path"
                    } else {
                        " by stable edge-id order"
                    }
                ),
            ));
            chosen
        };

        if !selected_ids.insert(chosen.id) {
            return Err(ExportError::new(
                "RW404_OPENBVE_PATH_LOOP",
                format!("selected driveable path repeats edge {}", chosen.id),
            ));
        }
        selected.push(chosen);
        current = chosen.to;
    }

    if selected.len() < project.network.edges.len() {
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW406_OPENBVE_BRANCHES_DROPPED",
            format!(
                "selected {} of {} track edges for player rail 0; remaining branches are not exported yet",
                selected.len(),
                project.network.edges.len()
            ),
        ));
    }

    Ok(selected)
}

fn fmt_number(value: f64) -> String {
    let value = if value.abs() < 0.000_000_5 { 0.0 } else { value };
    let mut text = format!("{value:.6}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn normalize_optional(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.abs() >= EPSILON)
}

fn option_changed(previous: Option<f64>, next: Option<f64>) -> bool {
    match (previous, next) {
        (None, None) => false,
        (Some(a), Some(b)) => (a - b).abs() > 0.000_001,
        _ => true,
    }
}

fn horizontal_distance(from: Vec3, to: Vec3) -> f64 {
    let dx = to.x - from.x;
    let dz = to.z - from.z;
    (dx * dx + dz * dz).sqrt()
}

fn quantize_position(position: f64) -> f64 {
    (position / BLOCK_LENGTH_M).round() * BLOCK_LENGTH_M
}

pub fn render_route(project: &RailProject) -> Result<ExportedRoute, ExportError> {
    let mut diagnostics = Vec::new();
    let path = select_driveable_path(project, &mut diagnostics)?;
    let nodes: BTreeMap<EntityId, Vec3> = project
        .network
        .nodes
        .iter()
        .map(|node| (node.id, node.position))
        .collect();

    let gauges: BTreeSet<u32> = path.iter().filter_map(|edge| edge.gauge_mm).collect();
    let gauge = gauges.iter().next().copied().unwrap_or(1435);
    if gauges.is_empty() {
        diagnostics.push(Diagnostic::new(
            Severity::Info,
            "RW407_OPENBVE_DEFAULT_GAUGE",
            "no gauge is present in the selected IR path; using OpenBVE-compatible default 1435 mm",
        ));
    } else if gauges.len() > 1 {
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW408_OPENBVE_CONFLICTING_GAUGE",
            format!(
                "selected path contains multiple gauges {:?}; exporting {} mm",
                gauges, gauge
            ),
        ));
    }

    let mut csv = String::new();
    csv.push_str("With Options\n");
    csv.push_str(&format!(".BlockLength {}\n\n", fmt_number(BLOCK_LENGTH_M)));
    csv.push_str("With Route\n");
    csv.push_str(&format!(".Gauge {}\n\n", gauge));
    csv.push_str("With Track\n");

    let mut cumulative = 0.0;
    let mut previous_curve = None;
    let mut previous_gradient = None;
    let mut previous_limit = None;
    let mut inferred_length = false;
    let mut inferred_gradient = false;
    let mut flattened_geometry = false;
    let mut quantized = false;

    for edge in path {
        let from = *nodes.get(&edge.from).ok_or_else(|| {
            ExportError::new(
                "RW401_OPENBVE_DANGLING_EDGE",
                format!("missing source node {} for edge {}", edge.from, edge.id),
            )
        })?;
        let to = *nodes.get(&edge.to).ok_or_else(|| {
            ExportError::new(
                "RW401_OPENBVE_DANGLING_EDGE",
                format!("missing target node {} for edge {}", edge.to, edge.id),
            )
        })?;

        let length = match edge.length_m.filter(|length| *length > EPSILON) {
            Some(length) => length,
            None => {
                inferred_length = true;
                let horizontal = horizontal_distance(from, to);
                if horizontal > EPSILON {
                    horizontal
                } else {
                    let dy = to.y - from.y;
                    if dy.abs() <= EPSILON {
                        return Err(ExportError::new(
                            "RW409_OPENBVE_ZERO_EDGE",
                            format!("edge {} has no usable length", edge.id),
                        ));
                    }
                    dy.abs()
                }
            }
        };

        let curve = normalize_optional(edge.curve_radius_m);
        if curve.is_none()
            && edge
                .provenance
                .as_ref()
                .map(|provenance| provenance.source_format == SourceFormat::MstsOpenRails)
                .unwrap_or(false)
        {
            flattened_geometry = true;
        }

        let gradient = match normalize_optional(edge.gradient_per_mille) {
            Some(gradient) => Some(gradient),
            None => {
                let inferred = (to.y - from.y) * 1000.0 / length;
                if inferred.abs() > 0.000_001 {
                    inferred_gradient = true;
                    Some(inferred)
                } else {
                    None
                }
            }
        };
        let limit = edge.speed_limit_kmh.filter(|speed| *speed > EPSILON);

        let position = quantize_position(cumulative);
        if (position - cumulative).abs() > 0.000_001 {
            quantized = true;
        }

        let mut commands = Vec::new();
        if option_changed(previous_gradient, gradient) {
            commands.push(format!(".Pitch {}", fmt_number(gradient.unwrap_or(0.0))));
            previous_gradient = gradient;
        }
        if option_changed(previous_curve, curve) {
            commands.push(format!(
                ".Curve {}; 0",
                fmt_number(curve.unwrap_or(0.0))
            ));
            previous_curve = curve;
        }
        if option_changed(previous_limit, limit) {
            commands.push(format!(".Limit {}", fmt_number(limit.unwrap_or(0.0))));
            previous_limit = limit;
        }

        if !commands.is_empty() {
            csv.push_str(&fmt_number(position));
            csv.push_str(", ");
            csv.push_str(&commands.join(", "));
            csv.push('\n');
        }

        cumulative += length;
    }

    let final_position = quantize_position(cumulative);
    if (final_position - cumulative).abs() > 0.000_001 {
        quantized = true;
    }
    csv.push_str(&fmt_number(final_position));
    csv.push('\n');

    if inferred_length {
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW410_OPENBVE_INFERRED_LENGTH",
            "one or more edge lengths were missing and were approximated from IR node coordinates",
        ));
    }
    if inferred_gradient {
        diagnostics.push(Diagnostic::new(
            Severity::Info,
            "RW411_OPENBVE_INFERRED_GRADIENT",
            "one or more gradients were inferred from node elevation differences",
        ));
    }
    if flattened_geometry {
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW412_OPENBVE_UNKNOWN_CURVATURE",
            "MSTS/OpenRails PAT topology does not contain full track-section curvature; unknown sections are currently exported as straight chords",
        ));
    }
    if quantized {
        diagnostics.push(Diagnostic::new(
            Severity::Info,
            "RW413_OPENBVE_POSITION_QUANTIZED",
            format!(
                "track positions were quantized to {} m OpenBVE blocks",
                fmt_number(BLOCK_LENGTH_M)
            ),
        ));
    }
    if !project.assets.is_empty() {
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW414_OPENBVE_ASSETS_NOT_EXPORTED",
            format!(
                "the IR contains {} asset references; this exporter currently writes route geometry only",
                project.assets.len()
            ),
        ));
    }

    Ok(ExportedRoute { csv, diagnostics })
}

#[cfg(test)]
mod tests {
    use super::*;
    use railweave_adapters::import_path;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "railweave-openbve-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn preserves_bve_geometry_through_ir_and_export() {
        let root = fixture();
        let route = root.join("route.csv");
        fs::write(
            &route,
            "With Track\n0, .Pitch 10\n100, .Curve 500; 0\n200, .Limit 80\n300, .Curve 0; 0\n",
        )
        .unwrap();

        let imported = import_path(&route).unwrap();
        let exported = render_route(&imported.project).unwrap();

        assert!(exported.csv.contains("With Options"));
        assert!(exported.csv.contains(".BlockLength 1"));
        assert!(exported.csv.contains(".Gauge 1435"));
        assert!(exported.csv.contains("0, .Pitch 10"));
        assert!(exported.csv.contains("100, .Curve 500; 0"));
        assert!(exported.csv.contains("200, .Limit 80"));
        assert!(exported.csv.lines().any(|line| line == "300"));
        fs::remove_dir_all(root).ok();
    }
}
