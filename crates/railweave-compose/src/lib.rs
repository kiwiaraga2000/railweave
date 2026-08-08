use railweave_adapters::import_path;
use railweave_core::{Diagnostic, ImportResult, Severity, IR_SCHEMA_VERSION};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub inputs: BTreeMap<String, InputSpec>,
    pub compose: ComposeSpec,
}

#[derive(Debug, Deserialize)]
pub struct InputSpec {
    pub source: Option<PathBuf>,
    pub ir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct ComposeSpec {
    pub network: String,
    pub metadata: Option<String>,
    #[serde(default)]
    pub assets: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ComposeError {
    pub code: &'static str,
    pub message: String,
}

impl ComposeError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ComposeError {}

fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn load_input(base: &Path, name: &str, spec: &InputSpec) -> Result<ImportResult, ComposeError> {
    match (&spec.source, &spec.ir) {
        (Some(source), None) => import_path(&resolve(base, source)).map_err(|error| {
            ComposeError::new(
                "RW301_SOURCE_IMPORT_FAILED",
                format!("input {name:?} could not be imported: {error}"),
            )
        }),
        (None, Some(ir)) => {
            let path = resolve(base, ir);
            let text = fs::read_to_string(&path).map_err(|error| {
                ComposeError::new(
                    "RW302_IR_READ_FAILED",
                    format!(
                        "failed to read input {name:?} at {}: {error}",
                        path.display()
                    ),
                )
            })?;
            serde_json::from_str(&text).map_err(|error| {
                ComposeError::new(
                    "RW303_IR_PARSE_FAILED",
                    format!(
                        "failed to parse input {name:?} at {}: {error}",
                        path.display()
                    ),
                )
            })
        }
        (Some(_), Some(_)) => Err(ComposeError::new(
            "RW304_AMBIGUOUS_INPUT",
            format!("input {name:?} must specify exactly one of source or ir"),
        )),
        (None, None) => Err(ComposeError::new(
            "RW305_EMPTY_INPUT",
            format!("input {name:?} must specify source or ir"),
        )),
    }
}

fn require_input<'a>(
    inputs: &'a BTreeMap<String, ImportResult>,
    name: &str,
) -> Result<&'a ImportResult, ComposeError> {
    inputs.get(name).ok_or_else(|| {
        ComposeError::new(
            "RW306_UNKNOWN_INPUT",
            format!("composition references unknown input {name:?}"),
        )
    })
}

fn max_entity_id(result: &ImportResult) -> u64 {
    result
        .project
        .network
        .nodes
        .iter()
        .map(|node| node.id)
        .chain(result.project.network.edges.iter().map(|edge| edge.id))
        .chain(result.project.assets.iter().map(|asset| asset.id))
        .max()
        .unwrap_or(0)
}

pub fn compose_manifest(path: &Path) -> Result<ImportResult, ComposeError> {
    let text = fs::read_to_string(path).map_err(|error| {
        ComposeError::new(
            "RW307_MANIFEST_READ_FAILED",
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    let manifest: Manifest = toml::from_str(&text).map_err(|error| {
        ComposeError::new(
            "RW308_MANIFEST_PARSE_FAILED",
            format!("failed to parse {}: {error}", path.display()),
        )
    })?;

    if manifest.version != 1 {
        return Err(ComposeError::new(
            "RW309_MANIFEST_VERSION",
            format!("unsupported manifest version {}", manifest.version),
        ));
    }

    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut inputs = BTreeMap::new();
    for (name, spec) in &manifest.inputs {
        let input = load_input(base, name, spec)?;
        if input.project.schema_version != IR_SCHEMA_VERSION {
            return Err(ComposeError::new(
                "RW310_IR_VERSION",
                format!(
                    "input {name:?} uses IR schema {}, expected {}",
                    input.project.schema_version, IR_SCHEMA_VERSION
                ),
            ));
        }
        inputs.insert(name.clone(), input);
    }

    let network_input = require_input(&inputs, &manifest.compose.network)?;
    let metadata_name = manifest
        .compose
        .metadata
        .as_deref()
        .unwrap_or(&manifest.compose.network);
    let metadata_input = require_input(&inputs, metadata_name)?;

    let mut output = ImportResult::new(network_input.project.clone());
    output.project.metadata = metadata_input.project.metadata.clone();
    output.project.assets.clear();
    output.project.consists.clear();

    let asset_sources: Vec<&str> = if manifest.compose.assets.is_empty() {
        vec![manifest.compose.network.as_str()]
    } else {
        manifest.compose.assets.iter().map(String::as_str).collect()
    };

    let mut next_id = max_entity_id(&output).saturating_add(1);
    for source_name in &asset_sources {
        let source = require_input(&inputs, source_name)?;
        let mut asset_id_map = HashMap::new();
        for asset in &source.project.assets {
            let mut asset = asset.clone();
            let source_id = asset.id;
            asset.id = next_id;
            next_id = next_id.saturating_add(1);
            asset_id_map.insert(source_id, asset.id);
            output.project.assets.push(asset);
        }

        for consist in &source.project.consists {
            let mut consist = consist.clone();
            for member in &mut consist.members {
                let Some(remapped) = asset_id_map.get(&member.asset_id).copied() else {
                    return Err(ComposeError::new(
                        "RW311_CONSIST_MEMBER_ASSET",
                        format!(
                            "input {source_name:?} consist {:?} references missing asset {}",
                            consist.name, member.asset_id
                        ),
                    ));
                };
                member.asset_id = remapped;
            }
            output.project.consists.push(consist);
        }
    }

    for input in inputs.values() {
        output.diagnostics.extend(input.diagnostics.clone());
    }
    output.diagnostics.push(Diagnostic::new(
        Severity::Info,
        "RW300_COMPOSED",
        format!(
            "composed network from {:?}, metadata from {:?}, assets/consists from [{}]",
            manifest.compose.network,
            metadata_name,
            asset_sources.join(", ")
        ),
    ));

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use railweave_core::{
        AssetKind, AssetRef, ConsistMember, ImportResult, Provenance, RailProject,
        RollingStockConsist, RollingStockRole, SourceFormat, TrackNode, Vec3,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("railweave-compose-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn composes_network_assets_and_consists_from_different_inputs() {
        let root = fixture();

        let mut route = RailProject::new();
        route.metadata.title = Some("Route".to_string());
        route.network.nodes.push(TrackNode {
            id: 1,
            position: Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            provenance: None,
        });
        fs::write(
            root.join("route.json"),
            serde_json::to_string_pretty(&ImportResult::new(route)).unwrap(),
        )
        .unwrap();

        let mut stock = RailProject::new();
        stock.assets.push(AssetRef {
            id: 1,
            kind: AssetKind::RollingStock,
            name: Some("ED4M-head".to_string()),
            provenance: Provenance {
                source_format: SourceFormat::MstsOpenRails,
                source_path: PathBuf::from("ED4M-head.eng"),
                source_id: None,
            },
        });
        stock.assets.push(AssetRef {
            id: 2,
            kind: AssetKind::RollingStock,
            name: Some("ED4M-trailer".to_string()),
            provenance: Provenance {
                source_format: SourceFormat::MstsOpenRails,
                source_path: PathBuf::from("ED4M-trailer.wag"),
                source_id: None,
            },
        });
        stock.consists.push(RollingStockConsist {
            name: Some("ED4M".to_string()),
            members: vec![
                ConsistMember {
                    asset_id: 1,
                    role: RollingStockRole::Engine,
                    flipped: false,
                    source_uid: Some(10),
                },
                ConsistMember {
                    asset_id: 2,
                    role: RollingStockRole::Wagon,
                    flipped: true,
                    source_uid: Some(11),
                },
            ],
            provenance: Provenance {
                source_format: SourceFormat::MstsOpenRails,
                source_path: PathBuf::from("ED4M.con"),
                source_id: Some("TrainCfg".to_string()),
            },
        });
        fs::write(
            root.join("stock.json"),
            serde_json::to_string_pretty(&ImportResult::new(stock)).unwrap(),
        )
        .unwrap();

        fs::write(
            root.join("railweave.toml"),
            r#"version = 1

[inputs.route]
ir = "route.json"

[inputs.stock]
ir = "stock.json"

[compose]
network = "route"
assets = ["route", "stock"]
"#,
        )
        .unwrap();

        let composed = compose_manifest(&root.join("railweave.toml")).unwrap();
        assert_eq!(composed.project.network.nodes.len(), 1);
        assert_eq!(composed.project.assets.len(), 2);
        assert_eq!(composed.project.consists.len(), 1);
        assert_eq!(composed.project.consists[0].members.len(), 2);
        assert!(composed.project.assets[0].id > 1);
        assert_eq!(
            composed.project.consists[0].members[0].asset_id,
            composed.project.assets[0].id
        );
        assert_eq!(
            composed.project.consists[0].members[1].asset_id,
            composed.project.assets[1].id
        );
        assert_eq!(
            composed.project.consists[0].members[1].role,
            RollingStockRole::Wagon
        );
        assert!(composed.project.consists[0].members[1].flipped);
        fs::remove_dir_all(root).ok();
    }
}
