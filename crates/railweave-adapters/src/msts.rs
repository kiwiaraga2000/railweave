use crate::detectors::{decode_text, entries, msts_pat_candidates};
use railweave_core::{
    AssetKind, AssetRef, Diagnostic, ImportError, ImportResult, Provenance, RailProject, Severity,
    SourceFormat, TrackEdge, TrackNode, Vec3,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const TILE_SIZE: f64 = 2048.0;
const POSITION_EPSILON: f64 = 0.001;

#[derive(Debug, Clone)]
struct Pdp {
    tile_x: i32,
    tile_z: i32,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Clone)]
struct PathNode {
    next_main: u32,
    next_siding: u32,
    pdp_index: u32,
}

#[derive(Debug, Clone)]
struct TdbPoint {
    tile_x: i32,
    tile_z: i32,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Clone)]
struct TdbVectorSection {
    section_index: u32,
    shape_index: u32,
    point: TdbPoint,
}

#[derive(Debug, Clone)]
struct TdbPin {
    link: u32,
    direction: i32,
}

#[derive(Debug, Clone)]
struct TdbNode {
    index: u32,
    uid: Option<TdbPoint>,
    pins: Vec<TdbPin>,
    sections: Vec<TdbVectorSection>,
}

fn files_with_extension(root: &Path, extension: &str) -> Vec<PathBuf> {
    let mut matches: Vec<PathBuf> = entries(root)
        .into_iter()
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case(extension))
                    .unwrap_or(false)
        })
        .collect();
    matches.sort();
    matches
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
            for next in chars.by_ref() {
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

fn matching_paren(tokens: &[String], open: usize) -> Option<usize> {
    if tokens.get(open).map(String::as_str) != Some("(") {
        return None;
    }

    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.as_str() {
            "(" => depth += 1,
            ")" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_block(tokens: &[String], name: &str) -> Option<(usize, usize)> {
    for index in 0..tokens.len().saturating_sub(1) {
        if tokens[index].eq_ignore_ascii_case(name)
            && tokens.get(index + 1).map(String::as_str) == Some("(")
        {
            let open = index + 1;
            return matching_paren(tokens, open).map(|close| (open, close));
        }
    }
    None
}

fn parse_pat(text: &str) -> (Vec<Pdp>, Vec<PathNode>, Option<String>) {
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
                    pdps.push(Pdp {
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
                    nodes.push(PathNode {
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

fn parse_uid(tokens: &[String]) -> Option<TdbPoint> {
    let (open, close) = find_block(tokens, "uid")?;
    if close <= open + 12 {
        return None;
    }

    Some(TdbPoint {
        tile_x: parse_i32(tokens.get(open + 5))?,
        tile_z: parse_i32(tokens.get(open + 6))?,
        x: parse_f64(tokens.get(open + 7))?,
        y: parse_f64(tokens.get(open + 8))?,
        z: parse_f64(tokens.get(open + 9))?,
    })
}

fn parse_tdb_pins(tokens: &[String]) -> Vec<TdbPin> {
    let Some((open, close)) = find_block(tokens, "trpins") else {
        return Vec::new();
    };

    let mut pins = Vec::new();
    let mut index = open + 3;
    while index < close {
        if tokens[index].eq_ignore_ascii_case("trpin")
            && tokens.get(index + 1).map(String::as_str) == Some("(")
        {
            if let (Some(link), Some(direction)) = (
                parse_u32(tokens.get(index + 2)),
                parse_i32(tokens.get(index + 3)),
            ) {
                pins.push(TdbPin { link, direction });
            }
            if let Some(end) = matching_paren(tokens, index + 1) {
                index = end + 1;
                continue;
            }
        }
        index += 1;
    }
    pins
}

fn parse_tdb_vector_sections(tokens: &[String]) -> Vec<TdbVectorSection> {
    let Some((open, close)) = find_block(tokens, "trvectorsections") else {
        return Vec::new();
    };
    let Some(expected) = parse_u32(tokens.get(open + 1)).map(|value| value as usize) else {
        return Vec::new();
    };

    let mut sections = Vec::with_capacity(expected);
    let mut cursor = open + 2;
    while sections.len() < expected && cursor < close {
        if tokens[cursor].eq_ignore_ascii_case("trvectorsection")
            && tokens.get(cursor + 1).map(String::as_str) == Some("(")
        {
            let section_open = cursor + 1;
            let Some(section_close) = matching_paren(tokens, section_open) else {
                break;
            };
            if let Some(section) = parse_tdb_vector_section_values(
                tokens,
                section_open + 1,
                section_close,
            ) {
                sections.push(section);
            }
            cursor = section_close + 1;
            continue;
        }

        if let Some(section) = parse_tdb_vector_section_values(tokens, cursor, close) {
            sections.push(section);
            cursor += 16;
        } else {
            cursor += 1;
        }
    }

    sections
}

fn parse_tdb_vector_section_values(
    tokens: &[String],
    start: usize,
    end: usize,
) -> Option<TdbVectorSection> {
    if start + 16 > end {
        return None;
    }

    Some(TdbVectorSection {
        section_index: parse_u32(tokens.get(start))?,
        shape_index: parse_u32(tokens.get(start + 1))?,
        point: TdbPoint {
            tile_x: parse_i32(tokens.get(start + 8))?,
            tile_z: parse_i32(tokens.get(start + 9))?,
            x: parse_f64(tokens.get(start + 10))?,
            y: parse_f64(tokens.get(start + 11))?,
            z: parse_f64(tokens.get(start + 12))?,
        },
    })
}

fn parse_tdb(text: &str) -> Vec<TdbNode> {
    let tokens = stf_tokens(text);
    let Some((open, close)) = find_block(&tokens, "tracknodes") else {
        return Vec::new();
    };

    let mut nodes = Vec::new();
    let mut cursor = open + 2;
    while cursor < close {
        if tokens[cursor].eq_ignore_ascii_case("tracknode")
            && tokens.get(cursor + 1).map(String::as_str) == Some("(")
        {
            let node_open = cursor + 1;
            let Some(node_close) = matching_paren(&tokens, node_open) else {
                break;
            };
            if let Some(index) = parse_u32(tokens.get(node_open + 1)) {
                let body = &tokens[node_open + 2..node_close];
                nodes.push(TdbNode {
                    index,
                    uid: parse_uid(body),
                    pins: parse_tdb_pins(body),
                    sections: parse_tdb_vector_sections(body),
                });
            }
            cursor = node_close + 1;
            continue;
        }
        cursor += 1;
    }

    nodes
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

fn local_position(point: &TdbPoint, origin: &TdbPoint) -> Vec3 {
    Vec3 {
        x: (point.tile_x - origin.tile_x) as f64 * TILE_SIZE + point.x - origin.x,
        y: point.y - origin.y,
        z: (point.tile_z - origin.tile_z) as f64 * TILE_SIZE + point.z - origin.z,
    }
}

fn same_position(a: Vec3, b: Vec3) -> bool {
    (a.x - b.x).abs() <= POSITION_EPSILON
        && (a.y - b.y).abs() <= POSITION_EPSILON
        && (a.z - b.z).abs() <= POSITION_EPSILON
}

fn import_track_database(
    tdb_file: &Path,
    project: &mut RailProject,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), ImportError> {
    let bytes = fs::read(tdb_file).map_err(|error| {
        ImportError::new(
            "RW209_MSTS_TDB_READ_FAILED",
            format!("failed to read {}: {error}", tdb_file.display()),
        )
    })?;
    let text = decode_text(&bytes);
    let lower = text.to_ascii_lowercase();
    if !lower.contains("trackdb") || !lower.contains("tracknodes") {
        return Err(ImportError::new(
            "RW210_MSTS_TDB_UNSUPPORTED",
            format!(
                "{} is not a textual/UTF-16 MSTS TDB that RailWeave can parse yet",
                tdb_file.display()
            ),
        ));
    }

    let nodes = parse_tdb(&text);
    if nodes.is_empty() {
        return Err(ImportError::new(
            "RW211_MSTS_TDB_EMPTY",
            format!("could not extract TrackNodes from {}", tdb_file.display()),
        ));
    }

    let origin = nodes
        .iter()
        .find_map(|node| node.uid.as_ref())
        .or_else(|| {
            nodes
                .iter()
                .find_map(|node| node.sections.first().map(|section| &section.point))
        })
        .ok_or_else(|| {
            ImportError::new(
                "RW211_MSTS_TDB_EMPTY",
                format!("{} contains no usable TDB coordinates", tdb_file.display()),
            )
        })?
        .clone();

    project.metadata.title = tdb_file
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    project.metadata.description =
        Some("Imported from MSTS/OpenRails track database vector sections".to_string());

    let provenance = |source_id: Option<String>| Provenance {
        source_format: SourceFormat::MstsOpenRails,
        source_path: tdb_file.to_path_buf(),
        source_id,
    };

    let mut next_id = max_entity_id(project).saturating_add(1);
    let mut endpoint_ids = HashMap::new();
    let mut node_positions = HashMap::new();

    for node in &nodes {
        if node.sections.is_empty() {
            let Some(uid) = &node.uid else {
                continue;
            };
            let id = next_id;
            next_id += 1;
            let position = local_position(uid, &origin);
            endpoint_ids.insert(node.index, id);
            node_positions.insert(id, position);
            project.network.nodes.push(TrackNode {
                id,
                position,
                provenance: Some(provenance(Some(format!("TrackNode:{}", node.index)))),
            });
        }
    }

    let mut emitted_edges = HashSet::new();
    let mut imported_vector_nodes = 0usize;
    let mut ignored_pin_directions = false;

    for node in &nodes {
        if node.sections.is_empty() {
            continue;
        }
        imported_vector_nodes += 1;
        if node.pins.iter().any(|pin| pin.direction != 0 && pin.direction != 1) {
            ignored_pin_directions = true;
        }

        let mut chain: Vec<(u64, Vec3, Option<usize>)> = Vec::new();
        if let Some(start_pin) = node.pins.first() {
            if let Some(id) = endpoint_ids.get(&start_pin.link).copied() {
                if let Some(position) = node_positions.get(&id).copied() {
                    chain.push((id, position, None));
                }
            }
        }

        for (section_index, section) in node.sections.iter().enumerate() {
            let position = local_position(&section.point, &origin);
            if chain
                .last()
                .map(|(_, previous, _)| same_position(*previous, position))
                .unwrap_or(false)
            {
                continue;
            }

            let id = next_id;
            next_id += 1;
            node_positions.insert(id, position);
            project.network.nodes.push(TrackNode {
                id,
                position,
                provenance: Some(provenance(Some(format!(
                    "TrackNode:{}:TrVectorSection:{}:section={}:shape={}",
                    node.index, section_index, section.section_index, section.shape_index
                )))),
            });
            chain.push((id, position, Some(section_index)));
        }

        if let Some(end_pin) = node.pins.get(1) {
            if let Some(id) = endpoint_ids.get(&end_pin.link).copied() {
                if let Some(position) = node_positions.get(&id).copied() {
                    if !chain
                        .last()
                        .map(|(_, previous, _)| same_position(*previous, position))
                        .unwrap_or(false)
                    {
                        chain.push((id, position, None));
                    }
                }
            }
        }

        if chain.len() < 2 {
            diagnostics.push(
                Diagnostic::new(
                    Severity::Warning,
                    "RW212_MSTS_TDB_INCOMPLETE_VECTOR",
                    format!(
                        "TrackNode {} has {} vector section(s), but fewer than two usable geometry points",
                        node.index,
                        node.sections.len()
                    ),
                )
                .with_provenance(provenance(Some(format!("TrackNode:{}", node.index)))),
            );
            continue;
        }

        for pair_index in 0..chain.len() - 1 {
            let (from, _, from_section) = chain[pair_index];
            let (to, _, to_section) = chain[pair_index + 1];
            if from == to || !emitted_edges.insert((from, to)) {
                continue;
            }
            let section_hint = from_section.or(to_section).unwrap_or(0);
            project.network.edges.push(TrackEdge {
                id: next_id,
                from,
                to,
                gauge_mm: None,
                electrification: None,
                speed_limit_kmh: None,
                length_m: None,
                curve_radius_m: None,
                gradient_per_mille: None,
                provenance: Some(provenance(Some(format!(
                    "TrackNode:{}:TrVectorSection:{}",
                    node.index, section_hint
                )))),
            });
            next_id += 1;
        }
    }

    if project.network.edges.is_empty() {
        return Err(ImportError::new(
            "RW211_MSTS_TDB_EMPTY",
            format!(
                "{} contains TrackNodes, but no usable vector-section connections were produced",
                tdb_file.display()
            ),
        ));
    }

    diagnostics.push(
        Diagnostic::new(
            Severity::Info,
            "RW213_MSTS_TDB_IMPORT_SCOPE",
            format!(
                "imported route-wide TDB topology from {imported_vector_nodes} vector node(s); vector-section start coordinates are connected as straight chords until tsection.dat section geometry is parsed"
            ),
        )
        .with_provenance(provenance(None)),
    );
    if ignored_pin_directions {
        diagnostics.push(
            Diagnostic::new(
                Severity::Warning,
                "RW214_MSTS_TDB_PIN_DIRECTION",
                "one or more TDB TrPin direction values were outside the common 0/1 range; geometry order was kept from TrVectorSections",
            )
            .with_provenance(provenance(None)),
        );
    }

    Ok(())
}

fn import_path_topology(
    path_file: &Path,
    project: &mut RailProject,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), ImportError> {
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

    let (pdps, path_nodes, path_name) = parse_pat(&text);
    if pdps.is_empty() || path_nodes.is_empty() {
        return Err(ImportError::new(
            "RW203_MSTS_PAT_EMPTY",
            format!(
                "could not extract TrackPDPs and TrPathNodes from {}",
                path_file.display()
            ),
        ));
    }

    project.metadata.title = path_name.or_else(|| {
        path_file
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    });
    project.metadata.description =
        Some("Imported from MSTS/OpenRails PAT path topology".to_string());

    let provenance = |source_id: Option<String>| Provenance {
        source_format: SourceFormat::MstsOpenRails,
        source_path: path_file.to_path_buf(),
        source_id,
    };

    let origin_index = path_nodes
        .iter()
        .find_map(|node| {
            pdps.get(node.pdp_index as usize)
                .map(|_| node.pdp_index as usize)
        })
        .unwrap_or(0);
    let origin = &pdps[origin_index];
    let mut node_ids = Vec::with_capacity(path_nodes.len());
    let mut next_id = max_entity_id(project).saturating_add(1);

    for (index, path_node) in path_nodes.iter().enumerate() {
        let Some(pdp) = pdps.get(path_node.pdp_index as usize) else {
            node_ids.push(None);
            diagnostics.push(
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
        project.network.nodes.push(TrackNode {
            id,
            position: Vec3 {
                x: (pdp.tile_x - origin.tile_x) as f64 * TILE_SIZE + pdp.x - origin.x,
                y: pdp.y - origin.y,
                z: (pdp.tile_z - origin.tile_z) as f64 * TILE_SIZE + pdp.z - origin.z,
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
                diagnostics.push(
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

            project.network.edges.push(TrackEdge {
                id: next_id,
                from: from_id,
                to: to_id,
                gauge_mm: None,
                electrification: None,
                speed_limit_kmh: None,
                length_m: None,
                curve_radius_m: None,
                gradient_per_mille: None,
                provenance: Some(provenance(Some(format!("TrPathNode:{index}:{kind}")))),
            });
            next_id += 1;
        }
    }

    diagnostics.push(
        Diagnostic::new(
            Severity::Info,
            "RW207_MSTS_IMPORT_SCOPE",
            "current MSTS/OpenRails PAT importer converts waypoint topology using 2048 m MSTS tiles; route-wide TDB geometry is preferred when a supported .tdb is available",
        )
        .with_provenance(provenance(None)),
    );

    Ok(())
}

fn add_consist_asset(project: &mut RailProject, next_id: &mut u64, path: PathBuf) {
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    project.assets.push(AssetRef {
        id: *next_id,
        kind: AssetKind::RollingStock,
        name,
        provenance: Provenance {
            source_format: SourceFormat::MstsOpenRails,
            source_path: path,
            source_id: None,
        },
    });
    *next_id = next_id.saturating_add(1);
}

pub(crate) fn import(root: &Path) -> Result<ImportResult, ImportError> {
    let tdb_candidates = files_with_extension(root, "tdb");
    let path_candidates = msts_pat_candidates(root);
    let consists = files_with_extension(root, "con");

    if tdb_candidates.is_empty() && path_candidates.is_empty() && consists.is_empty() {
        return Err(ImportError::new(
            "RW200_MSTS_CONTENT_NOT_FOUND",
            "MSTS/OpenRails was detected, but no supported .tdb track database, .pat path or .con consist was found",
        ));
    }

    let mut result = ImportResult::new(RailProject::new());
    let mut route_imported = false;

    if let Some(tdb_file) = tdb_candidates.first() {
        match import_track_database(tdb_file, &mut result.project, &mut result.diagnostics) {
            Ok(()) => {
                route_imported = true;
                if tdb_candidates.len() > 1 {
                    result.diagnostics.push(Diagnostic::new(
                        Severity::Warning,
                        "RW215_MSTS_MULTIPLE_TDB",
                        format!(
                            "{} TDB files found; imported {}. Pass a specific .tdb file to choose another track database.",
                            tdb_candidates.len(),
                            tdb_file.display()
                        ),
                    ));
                }
            }
            Err(error) if !path_candidates.is_empty() => {
                result.diagnostics.push(Diagnostic::new(
                    Severity::Warning,
                    "RW216_MSTS_TDB_FALLBACK_PAT",
                    format!(
                        "TDB import failed ({error}); falling back to PAT topology because a supported path is available"
                    ),
                ));
            }
            Err(error) => return Err(error),
        }
    }

    if !route_imported {
        if let Some(path_file) = path_candidates.first() {
            import_path_topology(path_file, &mut result.project, &mut result.diagnostics)?;
            if path_candidates.len() > 1 {
                result.diagnostics.push(Diagnostic::new(
                    Severity::Warning,
                    "RW204_MSTS_MULTIPLE_PATHS",
                    format!(
                        "{} PAT files found; imported {}. Pass a specific .pat file to choose another path.",
                        path_candidates.len(),
                        path_file.display()
                    ),
                ));
            }
        }
    }

    let mut next_id = max_entity_id(&result.project).saturating_add(1);
    for path in consists {
        add_consist_asset(&mut result.project, &mut next_id, path);
    }

    if result.project.network.nodes.is_empty() && result.project.metadata.title.is_none() {
        result.project.metadata.title = root
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_owned);
        result.project.metadata.description =
            Some("Imported from MSTS/OpenRails rolling-stock content".to_string());
    }

    if !result.project.assets.is_empty() {
        result.diagnostics.push(Diagnostic::new(
            Severity::Info,
            "RW208_MSTS_ASSET_IMPORT_SCOPE",
            "MSTS/OpenRails .con files are represented as rolling-stock source asset references; consist vehicles, physics, cabs and sounds are future work",
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
    fn imports_pat_topology() {
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

    #[test]
    fn imports_tdb_vector_section_geometry() {
        let root = fixture("msts-tdb");
        let tdb = root.join("route.tdb");
        fs::write(
            &tdb,
            r#"SIMISA@@@@@@@@@@JINX0T0t______
TrackDB (
  TrackNodes ( 3
    TrackNode ( 1
      UiD ( 0 0 100 0 10 20 0 100 0 0 0 0 )
      TrEndNode ( )
      TrPins ( 1 0
        TrPin ( 2 1 )
      )
    )
    TrackNode ( 2
      TrVectorNode (
        TrVectorSections ( 2
          1 100 0 0 1 0 1 00 10 20 0 100 0 0 0 0
          2 101 0 0 2 0 1 00 11 20 -1948 101 0 0 0 0
        )
      )
      TrPins ( 1 1
        TrPin ( 1 0 )
        TrPin ( 3 1 )
      )
    )
    TrackNode ( 3
      UiD ( 0 0 101 0 11 20 -1848 102 0 0 0 0 )
      TrEndNode ( )
      TrPins ( 1 0
        TrPin ( 2 0 )
      )
    )
  )
)
"#,
        )
        .unwrap();

        let imported = import_path(&root).unwrap();
        assert_eq!(imported.project.network.nodes.len(), 3);
        assert_eq!(imported.project.network.edges.len(), 2);
        let mut xs: Vec<f64> = imported
            .project
            .network
            .nodes
            .iter()
            .map(|node| node.position.x)
            .collect();
        xs.sort_by(|a, b| a.total_cmp(b));
        assert!((xs[0] - 0.0).abs() < 0.001);
        assert!((xs[1] - 100.0).abs() < 0.001);
        assert!((xs[2] - 200.0).abs() < 0.001);
        assert!(imported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RW213_MSTS_TDB_IMPORT_SCOPE"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn prefers_tdb_over_pat_when_both_are_available() {
        let root = fixture("msts-tdb-priority");
        fs::write(
            root.join("route.tdb"),
            r#"TrackDB (
  TrackNodes ( 3
    TrackNode ( 1 UiD ( 0 0 1 0 0 0 0 0 0 0 0 0 ) TrEndNode ( ) TrPins ( 1 0 TrPin ( 2 1 ) ) )
    TrackNode ( 2 TrVectorNode ( TrVectorSections ( 1 1 1 0 0 1 0 1 00 0 0 0 0 0 0 0 0 ) ) TrPins ( 1 1 TrPin ( 1 0 ) TrPin ( 3 1 ) ) )
    TrackNode ( 3 UiD ( 0 0 2 0 0 0 100 0 0 0 0 0 ) TrEndNode ( ) TrPins ( 1 0 TrPin ( 2 0 ) ) )
  )
)
"#,
        )
        .unwrap();
        let paths = root.join("PATHS");
        fs::create_dir(&paths).unwrap();
        fs::write(
            paths.join("fallback.pat"),
            "TrackPDPs ( TrackPDP ( 0 0 0 0 0 0 0 ) TrackPDP ( 0 0 50 0 0 0 0 ) )\nTrPathNodes ( 2 TrPathNode ( 0 1 4294967295 0 ) TrPathNode ( 0 4294967295 4294967295 1 ) )\n",
        )
        .unwrap();

        let imported = import_path(&root).unwrap();
        assert!(imported
            .project
            .metadata
            .description
            .as_deref()
            .unwrap_or_default()
            .contains("track database"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn imports_consist_as_rolling_stock_asset() {
        let root = fixture("msts-consist");
        let consist = root.join("ED4M.con");
        fs::write(
            &consist,
            "SIMISA@@@@@@@@@@JINX0D0t______\nTrainCfg ( ED4M )\n",
        )
        .unwrap();

        let imported = import_path(&consist).unwrap();
        assert!(imported.project.network.nodes.is_empty());
        assert_eq!(imported.project.assets.len(), 1);
        assert_eq!(imported.project.assets[0].kind, AssetKind::RollingStock);
        assert_eq!(imported.project.assets[0].name.as_deref(), Some("ED4M"));
        fs::remove_dir_all(root).ok();
    }
}
