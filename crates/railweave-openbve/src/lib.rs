use railweave_core::{
    Diagnostic, EntityId, RailProject, RollingStockRole, Severity, SourceFormat, TrackEdge, Vec3,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const BLOCK_LENGTH_M: f64 = 1.0;
const EPSILON: f64 = 1.0e-9;

#[derive(Debug, Clone)]
pub struct ExportedRoute {
    pub csv: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default)]
pub struct PackageOptions {
    pub name: Option<String>,
    pub copy_native_openbve_train: bool,
}

#[derive(Debug, Clone)]
pub struct ExportedPackage {
    pub root: PathBuf,
    pub route_path: PathBuf,
    pub train_path: PathBuf,
    pub manifest_path: PathBuf,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackageManifest {
    schema_version: u32,
    generator: String,
    package_name: String,
    route: String,
    train: String,
    source_formats: Vec<String>,
    counts: PackageCounts,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackageCounts {
    nodes: usize,
    edges: usize,
    stations: usize,
    assets: usize,
    vehicles: usize,
    consists: usize,
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

fn io_error(code: &'static str, context: impl Into<String>, error: std::io::Error) -> ExportError {
    ExportError::new(code, format!("{}: {error}", context.into()))
}

fn package_slug(value: &str) -> String {
    let mut output = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            output.push(character.to_ascii_lowercase());
            separator = false;
        } else if !separator && !output.is_empty() {
            output.push('-');
            separator = true;
        }
    }
    output.trim_matches('-').to_string()
}

fn mean(values: impl Iterator<Item = f64>, fallback: f64) -> f64 {
    let values: Vec<f64> = values
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect();
    if values.is_empty() {
        fallback
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn selected_vehicle_ids(project: &RailProject) -> Vec<(EntityId, RollingStockRole)> {
    if let Some(consist) = project.consists.first() {
        return consist
            .members
            .iter()
            .map(|member| (member.asset_id, member.role))
            .collect();
    }
    project
        .vehicles
        .iter()
        .map(|vehicle| {
            let role = if vehicle.max_power_w.unwrap_or(0.0) > 0.0
                || vehicle.max_tractive_force_n.unwrap_or(0.0) > 0.0
            {
                RollingStockRole::Engine
            } else {
                RollingStockRole::Wagon
            };
            (vehicle.asset_id, role)
        })
        .collect()
}

fn render_train_dat(project: &RailProject, diagnostics: &mut Vec<Diagnostic>) -> String {
    let selected = selected_vehicle_ids(project);
    let by_id: BTreeMap<EntityId, _> = project
        .vehicles
        .iter()
        .map(|vehicle| (vehicle.asset_id, vehicle))
        .collect();
    let vehicles: Vec<(_, RollingStockRole)> = selected
        .iter()
        .filter_map(|(id, role)| by_id.get(id).map(|vehicle| (*vehicle, *role)))
        .collect();

    if vehicles.is_empty() {
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW420_OPENBVE_FALLBACK_TRAIN",
            "no structured rolling stock was available; generated a conservative one-car OpenBVE fallback train",
        ));
    }

    let motor_count = selected
        .iter()
        .filter(|(_, role)| *role == RollingStockRole::Engine)
        .count()
        .max(1);
    let total_count = selected.len().max(vehicles.len()).max(1);
    let trailer_count = total_count.saturating_sub(motor_count);
    let motor_mass_t = mean(
        vehicles
            .iter()
            .filter(|(_, role)| *role == RollingStockRole::Engine)
            .filter_map(|(vehicle, _)| vehicle.mass_kg)
            .map(|mass| mass / 1000.0),
        42.0,
    );
    let trailer_mass_t = mean(
        vehicles
            .iter()
            .filter(|(_, role)| *role == RollingStockRole::Wagon)
            .filter_map(|(vehicle, _)| vehicle.mass_kg)
            .map(|mass| mass / 1000.0),
        36.0,
    );
    let length_m = mean(
        vehicles.iter().filter_map(|(vehicle, _)| vehicle.length_m),
        20.0,
    );
    let width_m = mean(
        vehicles.iter().filter_map(|(vehicle, _)| vehicle.width_m),
        2.8,
    );
    let height_m = mean(
        vehicles.iter().filter_map(|(vehicle, _)| vehicle.height_m),
        3.8,
    );
    let total_mass_kg = vehicles
        .iter()
        .filter_map(|(vehicle, _)| vehicle.mass_kg)
        .sum::<f64>()
        .max((motor_count as f64 * motor_mass_t + trailer_count as f64 * trailer_mass_t) * 1000.0);
    let tractive_force_n = vehicles
        .iter()
        .filter_map(|(vehicle, _)| vehicle.max_tractive_force_n)
        .sum::<f64>();
    let brake_force_n = vehicles
        .iter()
        .filter_map(|(vehicle, _)| vehicle.max_brake_force_n)
        .sum::<f64>();
    let acceleration = if tractive_force_n > 0.0 {
        (tractive_force_n / total_mass_kg * 3.6).clamp(0.2, 4.0)
    } else {
        1.0
    };
    let deceleration = if brake_force_n > 0.0 {
        (brake_force_n / total_mass_kg * 3.6).clamp(0.5, 5.0)
    } else {
        1.0
    };
    let maximum_speed_kmh = vehicles
        .iter()
        .filter_map(|(vehicle, _)| vehicle.max_velocity_mps)
        .map(|speed| speed * 3.6)
        .reduce(f64::min)
        .unwrap_or(120.0)
        .max(20.0);
    let front_is_motor = selected
        .first()
        .map(|(_, role)| *role == RollingStockRole::Engine)
        .unwrap_or(true);

    if !vehicles.is_empty()
        && vehicles
            .iter()
            .any(|(vehicle, _)| vehicle.mass_kg.is_none() || vehicle.length_m.is_none())
    {
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW421_OPENBVE_TRAIN_DEFAULTS",
            "missing rolling-stock dimensions or masses were filled with documented conservative defaults",
        ));
    }

    let acceleration_low = (acceleration * 0.75).max(0.1);
    format!(
        "OPENBVE\n\n#ACCELERATION\n{a},{b},20,{v1},1.2\n{a},{b},35,{v2},1.4\n{a},{b},50,{v3},1.6\n{a},{b},65,{v4},1.8\n\n#PERFORMANCE\n{d}\n0.35\n0\n0.0025\n1.1\n\n#BRAKE\n0\n0\n0\n\n#PRESSURE\n440\n440\n690\n780\n490\n\n#HANDLE\n0\n4\n8\n0\n\n#CAB\n0\n2600\n-1000\n0\n\n#CAR\n{mm}\n{mc}\n{tm}\n{tc}\n{length}\n{front}\n{width}\n{height}\n1.6\n{area}\n{unexposed}\n\n#DEVICE\n-1\n0\n0\n0\n0\n0\n0\n0\n",
        a = fmt_number(acceleration),
        b = fmt_number(acceleration_low),
        v1 = fmt_number((maximum_speed_kmh * 0.35).max(20.0)),
        v2 = fmt_number((maximum_speed_kmh * 0.55).max(35.0)),
        v3 = fmt_number((maximum_speed_kmh * 0.75).max(50.0)),
        v4 = fmt_number(maximum_speed_kmh),
        d = fmt_number(deceleration),
        mm = fmt_number(motor_mass_t),
        mc = motor_count,
        tm = fmt_number(trailer_mass_t),
        tc = trailer_count,
        length = fmt_number(length_m),
        front = u8::from(front_is_motor),
        width = fmt_number(width_m),
        height = fmt_number(height_m),
        area = fmt_number(width_m * height_m * 0.6),
        unexposed = fmt_number(width_m * height_m * 0.2),
    )
}

fn native_train_root(project: &RailProject) -> Option<PathBuf> {
    project
        .assets
        .iter()
        .find(|asset| {
            asset.provenance.source_format == SourceFormat::BveOpenBve
                && asset
                    .provenance
                    .source_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.eq_ignore_ascii_case("train.dat"))
                    .unwrap_or(false)
        })
        .and_then(|asset| asset.provenance.source_path.parent().map(Path::to_path_buf))
}

fn copy_tree(source: &Path, target: &Path, copied: &mut usize) -> Result<(), ExportError> {
    if *copied >= 20_000 {
        return Err(ExportError::new(
            "RW422_OPENBVE_ASSET_LIMIT",
            "native OpenBVE train contains more than 20,000 entries",
        ));
    }
    fs::create_dir_all(target).map_err(|error| {
        io_error(
            "RW423_OPENBVE_WRITE_FAILED",
            target.display().to_string(),
            error,
        )
    })?;
    for entry in fs::read_dir(source).map_err(|error| {
        io_error(
            "RW424_OPENBVE_ASSET_READ",
            source.display().to_string(),
            error,
        )
    })? {
        let entry = entry.map_err(|error| {
            io_error(
                "RW424_OPENBVE_ASSET_READ",
                source.display().to_string(),
                error,
            )
        })?;
        let file_type = entry.file_type().map_err(|error| {
            io_error(
                "RW424_OPENBVE_ASSET_READ",
                entry.path().display().to_string(),
                error,
            )
        })?;
        if file_type.is_symlink() {
            continue;
        }
        let destination = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_tree(&entry.path(), &destination, copied)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &destination).map_err(|error| {
                io_error(
                    "RW423_OPENBVE_WRITE_FAILED",
                    destination.display().to_string(),
                    error,
                )
            })?;
            *copied += 1;
        }
    }
    Ok(())
}

pub fn export_package(
    project: &RailProject,
    output: &Path,
    options: &PackageOptions,
) -> Result<ExportedPackage, ExportError> {
    let display_name = options
        .name
        .as_deref()
        .or(project.metadata.title.as_deref())
        .unwrap_or("RailWeave conversion");
    let slug = package_slug(display_name);
    let slug = if slug.is_empty() { "railweave" } else { &slug };
    let route_dir = output.join("Railway").join("Route").join(slug);
    let train_dir = output.join("Train").join(slug);
    fs::create_dir_all(&route_dir).map_err(|error| {
        io_error(
            "RW423_OPENBVE_WRITE_FAILED",
            route_dir.display().to_string(),
            error,
        )
    })?;
    fs::create_dir_all(&train_dir).map_err(|error| {
        io_error(
            "RW423_OPENBVE_WRITE_FAILED",
            train_dir.display().to_string(),
            error,
        )
    })?;

    let mut exported_route = render_route(project)?;
    exported_route
        .diagnostics
        .retain(|diagnostic| diagnostic.code != "RW414_OPENBVE_ASSETS_NOT_EXPORTED");
    let train_namespace = format!("With Train\n.Folder {slug}\n\n");
    exported_route.csv =
        exported_route
            .csv
            .replacen("With Track\n", &format!("{train_namespace}With Track\n"), 1);
    let route_path = route_dir.join("route.csv");
    fs::write(&route_path, &exported_route.csv).map_err(|error| {
        io_error(
            "RW423_OPENBVE_WRITE_FAILED",
            route_path.display().to_string(),
            error,
        )
    })?;

    let mut diagnostics = exported_route.diagnostics;
    let mut copied_native = false;
    if options.copy_native_openbve_train {
        if let Some(native) = native_train_root(project).filter(|path| path.is_dir()) {
            let canonical_native = fs::canonicalize(&native).map_err(|error| {
                io_error(
                    "RW424_OPENBVE_ASSET_READ",
                    native.display().to_string(),
                    error,
                )
            })?;
            let canonical_train_dir = fs::canonicalize(&train_dir).map_err(|error| {
                io_error(
                    "RW423_OPENBVE_WRITE_FAILED",
                    train_dir.display().to_string(),
                    error,
                )
            })?;
            if canonical_train_dir.starts_with(&canonical_native) {
                return Err(ExportError::new(
                    "RW427_OPENBVE_OUTPUT_INSIDE_SOURCE",
                    "OpenBVE package output may not be placed inside the native source train directory",
                ));
            }
            let mut copied = 0;
            copy_tree(&native, &train_dir, &mut copied)?;
            copied_native = true;
            diagnostics.push(Diagnostic::new(
                Severity::Info,
                "RW425_OPENBVE_NATIVE_TRAIN_COPIED",
                format!("copied {copied} native OpenBVE train asset file(s)"),
            ));
        }
    }

    let train_path = train_dir.join("train.dat");
    if !copied_native || !train_path.exists() {
        let train_dat = render_train_dat(project, &mut diagnostics);
        fs::write(&train_path, train_dat).map_err(|error| {
            io_error(
                "RW423_OPENBVE_WRITE_FAILED",
                train_path.display().to_string(),
                error,
            )
        })?;
    }

    let readme = format!(
        "{display_name}\n\nGenerated by RailWeave {}.\n\nRoute: Railway/Route/{slug}/route.csv\nTrain: Train/{slug}/train.dat\n\nOpen this package root in OpenBVE. Review railweave-manifest.json for conversion diagnostics and provenance.\n",
        env!("CARGO_PKG_VERSION")
    );
    fs::write(output.join("README.txt"), readme).map_err(|error| {
        io_error(
            "RW423_OPENBVE_WRITE_FAILED",
            output.display().to_string(),
            error,
        )
    })?;

    let source_formats: BTreeSet<String> = project
        .network
        .edges
        .iter()
        .filter_map(|edge| edge.provenance.as_ref())
        .map(|provenance| provenance.source_format.to_string())
        .chain(
            project
                .assets
                .iter()
                .map(|asset| asset.provenance.source_format.to_string()),
        )
        .collect();
    let manifest = PackageManifest {
        schema_version: 1,
        generator: format!("RailWeave {}", env!("CARGO_PKG_VERSION")),
        package_name: display_name.to_string(),
        route: format!("Railway/Route/{slug}/route.csv"),
        train: format!("Train/{slug}/train.dat"),
        source_formats: source_formats.into_iter().collect(),
        counts: PackageCounts {
            nodes: project.network.nodes.len(),
            edges: project.network.edges.len(),
            stations: project.stations.len(),
            assets: project.assets.len(),
            vehicles: project.vehicles.len(),
            consists: project.consists.len(),
        },
        diagnostics: diagnostics.clone(),
    };
    let manifest_path = output.join("railweave-manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|error| {
        ExportError::new(
            "RW426_OPENBVE_MANIFEST",
            format!("failed to encode manifest: {error}"),
        )
    })?;
    fs::write(&manifest_path, format!("{manifest_json}\n")).map_err(|error| {
        io_error(
            "RW423_OPENBVE_WRITE_FAILED",
            manifest_path.display().to_string(),
            error,
        )
    })?;

    Ok(ExportedPackage {
        root: output.to_path_buf(),
        route_path,
        train_path,
        manifest_path,
        diagnostics,
    })
}

fn is_main_edge(edge: &TrackEdge) -> bool {
    edge.provenance
        .as_ref()
        .and_then(|provenance| provenance.source_id.as_deref())
        .map(|source_id| source_id.ends_with(":main"))
        .unwrap_or(false)
}

fn is_known_msts_straight(edge: &TrackEdge) -> bool {
    edge.provenance
        .as_ref()
        .filter(|provenance| provenance.source_format == SourceFormat::MstsOpenRails)
        .and_then(|provenance| provenance.source_id.as_deref())
        .map(|source_id| source_id.contains(":geometry=straight"))
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
    let value = if value.abs() < 0.000_000_5 {
        0.0
    } else {
        value
    };
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

fn position_key(position: f64) -> i64 {
    (quantize_position(position) * 1000.0).round() as i64
}

fn station_name(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            ',' | ';' | '\n' | '\r' => ' ',
            other => other,
        })
        .collect::<String>()
        .trim()
        .to_string()
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
    let mut node_positions = BTreeMap::new();
    let mut path_position = 0.0;
    for edge in &path {
        node_positions.entry(edge.from).or_insert(path_position);
        let from = nodes[&edge.from];
        let to = nodes[&edge.to];
        let length = edge
            .length_m
            .filter(|length| *length > EPSILON)
            .unwrap_or_else(|| {
                let horizontal = horizontal_distance(from, to);
                if horizontal > EPSILON {
                    horizontal
                } else {
                    (to.y - from.y).abs()
                }
            });
        path_position += length;
        node_positions.insert(edge.to, path_position);
    }
    let mut stations: BTreeMap<i64, Vec<&railweave_core::Station>> = BTreeMap::new();
    for station in &project.stations {
        let position = station.position_m.or_else(|| {
            station
                .node_id
                .and_then(|id| node_positions.get(&id).copied())
        });
        if let Some(position) = position.filter(|position| position.is_finite() && *position >= 0.0)
        {
            stations
                .entry(position_key(position))
                .or_default()
                .push(station);
        } else {
            diagnostics.push(Diagnostic::new(
                Severity::Warning,
                "RW415_OPENBVE_STATION_UNPLACED",
                format!(
                    "station {:?} is not on the selected driveable path",
                    station.name
                ),
            ));
        }
    }

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
            && !is_known_msts_straight(edge)
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
        if let Some(at_position) = stations.get(&position_key(position)) {
            for station in at_position {
                commands.push(format!(
                    ".Sta {};;;;B;;;;{};100",
                    station_name(&station.name),
                    fmt_number(station.stop_time_s.max(1.0))
                ));
                commands.push(".Stop 0;5;5;0".to_string());
            }
        }
        if option_changed(previous_gradient, gradient) {
            commands.push(format!(".Pitch {}", fmt_number(gradient.unwrap_or(0.0))));
            previous_gradient = gradient;
        }
        if option_changed(previous_curve, curve) {
            commands.push(format!(".Curve {}; 0", fmt_number(curve.unwrap_or(0.0))));
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
    if let Some(at_position) = stations.get(&position_key(final_position)) {
        let mut commands = Vec::new();
        for station in at_position {
            commands.push(format!(
                ".Sta {};S;T;;B;1;;;{};100",
                station_name(&station.name),
                fmt_number(station.stop_time_s.max(1.0))
            ));
            commands.push(".Stop 0;5;5;0".to_string());
        }
        csv.push_str(&format!(
            "{}, {}\n",
            fmt_number(final_position),
            commands.join(", ")
        ));
    } else {
        csv.push_str(&fmt_number(final_position));
        csv.push('\n');
    }

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
            "one or more MSTS/OpenRails sections do not yet carry exact curve direction/radius; unknown sections are currently exported as straight chords",
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
        let path =
            std::env::temp_dir().join(format!("railweave-openbve-{}-{nonce}", std::process::id()));
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

    #[test]
    fn uses_tsection_lengths_when_exporting_msts_tdb() {
        let root = fixture();
        let openrails = root.join("OPENRAILS");
        fs::create_dir(&openrails).unwrap();
        fs::write(
            openrails.join("tsection.dat"),
            "TrackSections ( 3 TrackSection ( 1 SectionSize ( 1.5 75 ) ) TrackSection ( 2 SectionSize ( 1.5 125 ) ) )",
        )
        .unwrap();
        fs::write(
            root.join("route.tdb"),
            r#"TrackDB (
  TrackNodes ( 3
    TrackNode ( 1 UiD ( 0 0 1 0 0 0 0 0 0 0 0 0 ) TrEndNode ( ) TrPins ( 1 0 TrPin ( 2 1 ) ) )
    TrackNode ( 2
      TrVectorNode ( TrVectorSections ( 2
        1 1 0 0 1 0 1 00 0 0 0 0 0 0 0 0
        2 1 0 0 2 0 1 00 0 0 75 0 0 0 0 0
      ) )
      TrPins ( 1 1 TrPin ( 1 0 ) TrPin ( 3 1 ) )
    )
    TrackNode ( 3 UiD ( 0 0 2 0 0 0 200 0 0 0 0 0 ) TrEndNode ( ) TrPins ( 1 0 TrPin ( 2 0 ) ) )
  )
)"#,
        )
        .unwrap();

        let imported = import_path(&root).unwrap();
        let exported = render_route(&imported.project).unwrap();
        assert!(exported.csv.lines().any(|line| line == "200"));
        assert!(!exported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RW410_OPENBVE_INFERRED_LENGTH"));
        assert!(!exported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RW412_OPENBVE_UNKNOWN_CURVATURE"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn writes_a_playable_package_with_route_train_and_manifest() {
        let root = fixture();
        let source = root.join("demo.railweave.csv");
        fs::write(
            &source,
            "x,y,z,gauge_mm,speed_limit_kmh,station\n0,0,0,1435,60,Origin\n0,1,100,1435,80,\n0,2,200,1435,80,Terminus\n",
        )
        .unwrap();
        let imported = import_path(&source).unwrap();
        let output = root.join("package");
        let package = export_package(
            &imported.project,
            &output,
            &PackageOptions {
                name: Some("Demo Route".to_string()),
                copy_native_openbve_train: true,
            },
        )
        .unwrap();

        let route = fs::read_to_string(&package.route_path).unwrap();
        let train = fs::read_to_string(&package.train_path).unwrap();
        let manifest = fs::read_to_string(&package.manifest_path).unwrap();
        assert!(route.contains(".Folder demo-route"));
        assert!(route.contains(".Sta Origin"));
        assert!(route.contains(".Sta Terminus"));
        assert!(route.contains(".Limit 60"));
        assert!(route.contains("100, .Limit 80"));
        assert!(train.starts_with("OPENBVE"));
        assert!(train.contains("#CAR"));
        assert!(manifest.contains("\"package_name\": \"Demo Route\""));
        fs::remove_dir_all(root).ok();
    }
}
