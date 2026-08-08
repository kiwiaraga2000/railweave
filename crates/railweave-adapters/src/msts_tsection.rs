use crate::detectors::{decode_text, entries};
use railweave_core::{Diagnostic, Provenance, Severity, SourceFormat};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const EPSILON: f64 = 1.0e-9;
const MAX_INCLUDE_DEPTH: usize = 8;

#[derive(Debug, Clone, Copy)]
pub(crate) struct SectionGeometry {
    pub length_m: f64,
    pub curve_radius_m: Option<f64>,
    pub curve_angle_rad: Option<f64>,
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

fn parse_f64(token: Option<&String>) -> Option<f64> {
    token?.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn parse_u32(token: Option<&String>) -> Option<u32> {
    token?.parse().ok()
}

fn standard_section_geometry(
    tokens: &[String],
    open: usize,
    close: usize,
) -> Option<(u32, SectionGeometry)> {
    let section_index = parse_u32(tokens.get(open + 1))?;
    let body = &tokens[open + 2..close];

    let straight_length = find_block(body, "sectionsize")
        .and_then(|(size_open, _)| parse_f64(body.get(size_open + 2)))
        .filter(|length| length.abs() > EPSILON)
        .map(f64::abs);

    let curve = find_block(body, "sectioncurve").and_then(|(curve_open, _)| {
        let radius = parse_f64(body.get(curve_open + 1))?.abs();
        let angle_rad = parse_f64(body.get(curve_open + 2))?.to_radians();
        (radius > EPSILON && angle_rad.abs() > EPSILON).then_some((radius, angle_rad))
    });
    let curve_length = curve.map(|(radius, angle_rad)| radius * angle_rad.abs());
    let length_m = straight_length.or(curve_length)?;

    Some((
        section_index,
        SectionGeometry {
            length_m,
            curve_radius_m: curve.map(|(radius, _)| radius),
            curve_angle_rad: curve.map(|(_, angle_rad)| angle_rad),
        },
    ))
}

fn dynamic_section_geometry(
    tokens: &[String],
    open: usize,
    close: usize,
) -> Option<(u32, SectionGeometry)> {
    let body = &tokens[open + 1..close];
    let (curve_open, curve_close) = find_block(body, "sectioncurve")?;
    if curve_open != 1 {
        return None;
    }

    let index_offset = curve_close + 1;
    let section_index = parse_u32(body.get(index_offset))?;
    let a = parse_f64(body.get(index_offset + 1))?;
    let b = parse_f64(body.get(index_offset + 2))?;
    let curve = (b.abs() > EPSILON && a.abs() > EPSILON).then_some((b.abs(), a));
    let length_m = if let Some((radius, angle_rad)) = curve {
        radius * angle_rad.abs()
    } else {
        a.abs()
    };
    if length_m <= EPSILON {
        return None;
    }

    Some((
        section_index,
        SectionGeometry {
            length_m,
            curve_radius_m: curve.map(|(radius, _)| radius),
            curve_angle_rad: curve.map(|(_, angle_rad)| angle_rad),
        },
    ))
}

fn parse_sections(text: &str) -> HashMap<u32, SectionGeometry> {
    let tokens = stf_tokens(text);
    let mut sections = HashMap::new();
    let mut index = 0usize;

    while index + 1 < tokens.len() {
        if tokens[index].eq_ignore_ascii_case("tracksection")
            && tokens.get(index + 1).map(String::as_str) == Some("(")
        {
            let open = index + 1;
            let Some(close) = matching_paren(&tokens, open) else {
                break;
            };
            let parsed = standard_section_geometry(&tokens, open, close)
                .or_else(|| dynamic_section_geometry(&tokens, open, close));
            if let Some((section_index, geometry)) = parsed {
                sections.insert(section_index, geometry);
            }
            index = close + 1;
            continue;
        }
        index += 1;
    }

    sections
}

fn include_paths(text: &str) -> Vec<String> {
    let tokens = stf_tokens(text);
    let mut paths = Vec::new();
    let mut index = 0usize;
    while index + 2 < tokens.len() {
        if tokens[index].eq_ignore_ascii_case("include")
            && tokens.get(index + 1).map(String::as_str) == Some("(")
        {
            if let Some(path) = tokens.get(index + 2) {
                paths.push(path.clone());
            }
        }
        index += 1;
    }
    paths
}

fn normalized_include(base: &Path, raw: &str) -> PathBuf {
    let portable = raw.replace('\\', "/");
    base.join(portable)
}

fn provenance(path: &Path) -> Provenance {
    Provenance {
        source_format: SourceFormat::MstsOpenRails,
        source_path: path.to_path_buf(),
        source_id: Some("tsection.dat".to_string()),
    }
}

fn load_file(
    path: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    sections: &mut HashMap<u32, SectionGeometry>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if depth > MAX_INCLUDE_DEPTH || !visited.insert(path.to_path_buf()) {
        return;
    }

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            diagnostics.push(
                Diagnostic::new(
                    Severity::Warning,
                    "RW217_MSTS_TSECTION_READ_FAILED",
                    format!("failed to read {}: {error}", path.display()),
                )
                .with_provenance(provenance(path)),
            );
            return;
        }
    };
    let text = decode_text(&bytes);
    let base = path.parent().unwrap_or_else(|| Path::new("."));

    for include in include_paths(&text) {
        let include_path = normalized_include(base, &include);
        if include_path.exists() {
            load_file(&include_path, depth + 1, visited, sections, diagnostics);
        } else {
            diagnostics.push(
                Diagnostic::new(
                    Severity::Warning,
                    "RW218_MSTS_TSECTION_INCLUDE_MISSING",
                    format!(
                        "{} includes {}, but the referenced tsection file is not available",
                        path.display(),
                        include_path.display()
                    ),
                )
                .with_provenance(provenance(path)),
            );
        }
    }

    for (section_index, geometry) in parse_sections(&text) {
        sections.insert(section_index, geometry);
    }
}

fn candidates(root: &Path) -> Vec<PathBuf> {
    let search_root = if root.is_file() {
        root.parent().unwrap_or_else(|| Path::new("."))
    } else {
        root
    };
    let mut paths: Vec<PathBuf> = entries(search_root)
        .into_iter()
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.eq_ignore_ascii_case("tsection.dat"))
                    .unwrap_or(false)
        })
        .collect();
    paths.sort();
    paths
}

pub(crate) fn load_section_geometry(
    root: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> HashMap<u32, SectionGeometry> {
    let candidates = candidates(root);
    let mut sections = HashMap::new();
    let mut visited = HashSet::new();

    for path in &candidates {
        load_file(path, 0, &mut visited, &mut sections, diagnostics);
    }

    if candidates.is_empty() {
        diagnostics.push(Diagnostic::new(
            Severity::Info,
            "RW219_MSTS_TSECTION_NOT_FOUND",
            "no tsection.dat was found near the imported MSTS/OpenRails content; section geometry remains unknown",
        ));
    } else {
        diagnostics.push(Diagnostic::new(
            Severity::Info,
            "RW220_MSTS_TSECTION_LOADED",
            format!(
                "loaded geometry metadata for {} MSTS track section(s) from {} tsection root file(s)",
                sections.len(),
                candidates.len()
            ),
        ));
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("railweave-tsection-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn parses_standard_straight_and_curve_geometry() {
        let parsed = parse_sections(
            r#"TrackSections ( 3
TrackSection ( 1 SectionSize ( 1.5 50 ) )
TrackSection ( 2 SectionSize ( 1.5 0 ) SectionCurve ( 1000 5 ) )
TrackSection ( 3 SectionSize ( 1.5 0 ) SectionCurve ( 1000 -5 ) )
)"#,
        );
        assert!((parsed[&1].length_m - 50.0).abs() < 0.001);
        assert_eq!(parsed[&1].curve_radius_m, None);
        let arc = 1000.0 * 5.0_f64.to_radians();
        assert!((parsed[&2].length_m - arc).abs() < 0.001);
        assert_eq!(parsed[&2].curve_radius_m, Some(1000.0));
        assert!((parsed[&2].curve_angle_rad.unwrap() - 5.0_f64.to_radians()).abs() < 0.000001);
        assert!((parsed[&3].curve_angle_rad.unwrap() + 5.0_f64.to_radians()).abs() < 0.000001);
    }

    #[test]
    fn follows_relative_includes_and_allows_local_overrides() {
        let root = fixture();
        let global = root.join("GLOBAL");
        let route = root.join("ROUTES").join("Demo").join("OPENRAILS");
        fs::create_dir_all(&global).unwrap();
        fs::create_dir_all(&route).unwrap();
        fs::write(
            global.join("tsection.dat"),
            "TrackSections ( 2 TrackSection ( 1 SectionSize ( 1.5 10 ) ) TrackSection ( 2 SectionSize ( 1.5 20 ) ) )",
        )
        .unwrap();
        fs::write(
            route.join("tsection.dat"),
            "include ( \"../../../GLOBAL/tsection.dat\" )\nTrackSections ( 2 TrackSection ( 2 SectionSize ( 1.5 25 ) ) )",
        )
        .unwrap();

        let mut diagnostics = Vec::new();
        let sections = load_section_geometry(&root.join("ROUTES").join("Demo"), &mut diagnostics);
        assert!((sections[&1].length_m - 10.0).abs() < 0.001);
        assert!((sections[&2].length_m - 25.0).abs() < 0.001);
        fs::remove_dir_all(root).ok();
    }
}
