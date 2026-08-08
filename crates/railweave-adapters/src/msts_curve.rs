use crate::detectors::{decode_text, entries};
use crate::msts_tsection::{load_section_geometry, SectionGeometry};
use railweave_core::{Diagnostic, ImportResult, Provenance, Severity, SourceFormat};
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};
use std::fs;
use std::path::{Path, PathBuf};

const ANGLE_EPSILON: f64 = 1.0e-7;

#[derive(Debug, Clone, Copy)]
struct VectorSectionOrientation {
    section_index: u32,
    flag2: i32,
    start_yaw: f64,
}

#[derive(Debug, Clone)]
struct TdbNodeOrientation {
    index: u32,
    endpoint_yaw: Option<f64>,
    pins: Vec<u32>,
    sections: Vec<VectorSectionOrientation>,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedSection {
    radius_m: Option<f64>,
    known_geometry: bool,
}

fn tdb_candidates(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = entries(root)
        .into_iter()
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(|extension| extension.eq_ignore_ascii_case("tdb"))
                    .unwrap_or(false)
        })
        .collect();
    paths.sort();
    paths
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

fn parse_u32(token: Option<&String>) -> Option<u32> {
    token?.parse().ok()
}

fn parse_i32(token: Option<&String>) -> Option<i32> {
    token?.parse().ok()
}

fn parse_f64(token: Option<&String>) -> Option<f64> {
    token?.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn parse_endpoint_yaw(tokens: &[String]) -> Option<f64> {
    let (open, close) = find_block(tokens, "uid")?;
    if close <= open + 11 {
        return None;
    }
    parse_f64(tokens.get(open + 11))
}

fn parse_pins(tokens: &[String]) -> Vec<u32> {
    let Some((open, close)) = find_block(tokens, "trpins") else {
        return Vec::new();
    };
    let mut pins = Vec::new();
    let mut cursor = open + 3;
    while cursor < close {
        if tokens[cursor].eq_ignore_ascii_case("trpin")
            && tokens.get(cursor + 1).map(String::as_str) == Some("(")
        {
            if let Some(link) = parse_u32(tokens.get(cursor + 2)) {
                pins.push(link);
            }
            if let Some(end) = matching_paren(tokens, cursor + 1) {
                cursor = end + 1;
                continue;
            }
        }
        cursor += 1;
    }
    pins
}

fn parse_vector_section_values(
    tokens: &[String],
    start: usize,
    end: usize,
) -> Option<VectorSectionOrientation> {
    if start + 16 > end {
        return None;
    }
    Some(VectorSectionOrientation {
        section_index: parse_u32(tokens.get(start))?,
        flag2: parse_i32(tokens.get(start + 6))?,
        start_yaw: parse_f64(tokens.get(start + 14))?,
    })
}

fn parse_vector_sections(tokens: &[String]) -> Vec<VectorSectionOrientation> {
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
            if let Some(section) =
                parse_vector_section_values(tokens, section_open + 1, section_close)
            {
                sections.push(section);
            }
            cursor = section_close + 1;
            continue;
        }
        if let Some(section) = parse_vector_section_values(tokens, cursor, close) {
            sections.push(section);
            cursor += 16;
        } else {
            cursor += 1;
        }
    }
    sections
}

fn parse_nodes(text: &str) -> Vec<TdbNodeOrientation> {
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
                nodes.push(TdbNodeOrientation {
                    index,
                    endpoint_yaw: parse_endpoint_yaw(body),
                    pins: parse_pins(body),
                    sections: parse_vector_sections(body),
                });
            }
            cursor = node_close + 1;
            continue;
        }
        cursor += 1;
    }
    nodes
}

fn wrapped_angle_delta(from: f64, to: f64) -> f64 {
    (to - from + PI).rem_euclid(TAU) - PI
}

fn fallback_turn(geometry: SectionGeometry, flag2: i32) -> f64 {
    let turn = geometry.curve_angle_rad.unwrap_or(0.0);
    if flag2 == 0 {
        -turn
    } else {
        turn
    }
}

fn resolve_sections(
    nodes: &[TdbNodeOrientation],
    geometry: &HashMap<u32, SectionGeometry>,
) -> HashMap<(u32, usize), ResolvedSection> {
    let endpoint_yaws: HashMap<u32, f64> = nodes
        .iter()
        .filter_map(|node| node.endpoint_yaw.map(|yaw| (node.index, yaw)))
        .collect();
    let mut resolved = HashMap::new();

    for node in nodes {
        if node.sections.is_empty() {
            continue;
        }
        let end_yaw = node
            .pins
            .get(1)
            .and_then(|link| endpoint_yaws.get(link))
            .copied();

        for (index, section) in node.sections.iter().enumerate() {
            let Some(section_geometry) = geometry.get(&section.section_index).copied() else {
                continue;
            };
            let radius_m = section_geometry.curve_radius_m.map(|radius| {
                let next_yaw = node
                    .sections
                    .get(index + 1)
                    .map(|next| next.start_yaw)
                    .or(end_yaw);
                let observed_turn = next_yaw
                    .map(|next| wrapped_angle_delta(section.start_yaw, next))
                    .unwrap_or(0.0);
                let turn = if observed_turn.abs() > ANGLE_EPSILON {
                    observed_turn
                } else {
                    fallback_turn(section_geometry, section.flag2)
                };
                if turn < 0.0 {
                    -radius
                } else {
                    radius
                }
            });
            resolved.insert(
                (node.index, index),
                ResolvedSection {
                    radius_m,
                    known_geometry: true,
                },
            );
        }
    }

    resolved
}

fn parse_source_key(source_id: &str) -> Option<(u32, usize)> {
    let mut parts = source_id.split(':');
    if parts.next()? != "TrackNode" {
        return None;
    }
    let node_index = parts.next()?.parse().ok()?;
    if parts.next()? != "TrVectorSection" {
        return None;
    }
    let section_index = parts.next()?.parse().ok()?;
    Some((node_index, section_index))
}

fn mark_geometry(provenance: &mut Provenance, state: &str) {
    let Some(source_id) = provenance.source_id.as_mut() else {
        return;
    };
    if source_id.contains(":geometry=") {
        return;
    }
    source_id.push_str(":geometry=");
    source_id.push_str(state);
}

pub(crate) fn enrich_tdb_curves(root: &Path, result: &mut ImportResult) {
    if result.project.network.edges.is_empty() {
        return;
    }
    let tdb_files = tdb_candidates(root);
    let Some(tdb_file) = tdb_files.first() else {
        return;
    };

    let bytes = match fs::read(tdb_file) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let text = decode_text(&bytes);
    let nodes = parse_nodes(&text);
    if nodes.is_empty() {
        return;
    }

    let geometry = load_section_geometry(root, &mut Vec::new());
    if geometry.is_empty() {
        return;
    }
    let resolved = resolve_sections(&nodes, &geometry);
    let mut curve_count = 0usize;
    let mut straight_count = 0usize;

    for edge in &mut result.project.network.edges {
        let Some(provenance) = edge.provenance.as_mut() else {
            continue;
        };
        if provenance.source_format != SourceFormat::MstsOpenRails {
            continue;
        }
        let Some(source_id) = provenance.source_id.as_deref() else {
            continue;
        };
        let Some(key) = parse_source_key(source_id) else {
            continue;
        };
        let Some(section) = resolved.get(&key) else {
            mark_geometry(provenance, "unknown");
            continue;
        };
        if !section.known_geometry {
            mark_geometry(provenance, "unknown");
            continue;
        }
        edge.curve_radius_m = section.radius_m;
        if section.radius_m.is_some() {
            curve_count += 1;
            mark_geometry(provenance, "curve");
        } else {
            straight_count += 1;
            mark_geometry(provenance, "straight");
        }
    }

    if curve_count > 0 || straight_count > 0 {
        result.diagnostics.push(
            Diagnostic::new(
                Severity::Info,
                "RW223_MSTS_TSECTION_CURVES",
                format!(
                    "resolved tsection geometry for {curve_count} curved and {straight_count} straight TDB edge(s); curve direction uses the observed TDB yaw change when available"
                ),
            )
            .with_provenance(Provenance {
                source_format: SourceFormat::MstsOpenRails,
                source_path: tdb_file.clone(),
                source_id: None,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_yaw_across_pi() {
        let delta = wrapped_angle_delta(PI - 0.1, -PI + 0.2);
        assert!((delta - 0.3).abs() < 0.000001);
    }

    #[test]
    fn observed_yaw_selects_curve_sign() {
        let nodes = vec![TdbNodeOrientation {
            index: 2,
            endpoint_yaw: None,
            pins: vec![1, 3],
            sections: vec![
                VectorSectionOrientation {
                    section_index: 7,
                    flag2: 1,
                    start_yaw: 0.0,
                },
                VectorSectionOrientation {
                    section_index: 8,
                    flag2: 1,
                    start_yaw: 0.1,
                },
            ],
        }];
        let geometry = HashMap::from([(
            7,
            SectionGeometry {
                length_m: 10.0,
                curve_radius_m: Some(100.0),
                curve_angle_rad: Some(-0.1),
            },
        )]);
        let resolved = resolve_sections(&nodes, &geometry);
        assert_eq!(resolved[&(2, 0)].radius_m, Some(100.0));
    }
}
