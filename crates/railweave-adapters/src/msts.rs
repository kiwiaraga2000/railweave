use crate::detectors::{decode_text, entries, msts_pat_candidates};
use railweave_core::{
    AssetKind, AssetRef, Diagnostic, ImportError, ImportResult, Provenance, RailProject, Severity,
    SourceFormat, TrackEdge, TrackNode, Vec3,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const TILE_SIZE: f64 = 2048.0;

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
            "current MSTS/OpenRails route importer converts PAT waypoint topology using 2048 m MSTS tiles; full TDB geometry, track sections, signalling and world scenery are future work",
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
    let path_candidates = msts_pat_candidates(root);
    let consists = files_with_extension(root, "con");

    if path_candidates.is_empty() && consists.is_empty() {
        return Err(ImportError::new(
            "RW200_MSTS_CONTENT_NOT_FOUND",
            "MSTS/OpenRails was detected, but no supported .pat path or .con consist was found",
        ));
    }

    let mut result = ImportResult::new(RailProject::new());

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
