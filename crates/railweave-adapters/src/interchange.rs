use crate::detectors::{decode_text, entries};
use crate::parse_trainz_config;
use railweave_core::{
    Diagnostic, ImportError, ImportResult, Provenance, RailProject, Severity, SourceFormat,
    Station, TrackEdge, TrackNode, Vec3,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Debug)]
struct CsvTrackPoint {
    line: String,
    node_id: u64,
    position: Vec3,
    gauge_mm: Option<u32>,
    speed_limit_kmh: Option<f64>,
    curve_radius_m: Option<f64>,
    gradient_per_mille: Option<f64>,
}

fn provenance(path: &Path, format: SourceFormat, source_id: impl Into<String>) -> Provenance {
    Provenance {
        source_format: format,
        source_path: path.to_path_buf(),
        source_id: Some(source_id.into()),
    }
}

fn distance(a: Vec3, b: Vec3) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt()
}

fn property_number(properties: Option<&Value>, names: &[&str]) -> Option<f64> {
    let object = properties?.as_object()?;
    names.iter().find_map(|name| {
        object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .and_then(|(_, value)| {
                value
                    .as_f64()
                    .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
            })
    })
}

fn add_line(
    project: &mut RailProject,
    path: &Path,
    line_index: usize,
    coordinates: &[Value],
    properties: Option<&Value>,
) -> Result<(), ImportError> {
    if coordinates.len() < 2 {
        return Ok(());
    }
    let gauge_mm = property_number(properties, &["gauge_mm", "gauge"])
        .filter(|value| *value > 0.0 && *value <= u32::MAX as f64)
        .map(|value| value.round() as u32);
    let speed_limit_kmh = property_number(properties, &["speed_limit_kmh", "maxspeed", "speed"])
        .filter(|value| *value > 0.0);
    let mut previous = None;
    let mut next_id = project
        .network
        .nodes
        .iter()
        .map(|node| node.id)
        .chain(project.network.edges.iter().map(|edge| edge.id))
        .max()
        .unwrap_or(0)
        .saturating_add(1);

    for (point_index, coordinate) in coordinates.iter().enumerate() {
        let values = coordinate.as_array().ok_or_else(|| {
            ImportError::new(
                "RW120_GEOJSON_COORDINATE",
                format!("line {line_index} coordinate {point_index} is not an array"),
            )
        })?;
        if values.len() < 2 {
            return Err(ImportError::new(
                "RW120_GEOJSON_COORDINATE",
                format!("line {line_index} coordinate {point_index} has fewer than two values"),
            ));
        }
        let position = Vec3 {
            x: values[0].as_f64().ok_or_else(|| {
                ImportError::new("RW120_GEOJSON_COORDINATE", "non-numeric x coordinate")
            })?,
            z: values[1].as_f64().ok_or_else(|| {
                ImportError::new("RW120_GEOJSON_COORDINATE", "non-numeric y coordinate")
            })?,
            y: values.get(2).and_then(Value::as_f64).unwrap_or(0.0),
        };
        let node_id = next_id;
        next_id += 1;
        project.network.nodes.push(TrackNode {
            id: node_id,
            position,
            provenance: Some(provenance(
                path,
                SourceFormat::GeoJson,
                format!("line:{line_index}:point:{point_index}"),
            )),
        });
        if let Some((previous_id, previous_position)) = previous {
            project.network.edges.push(TrackEdge {
                id: next_id,
                from: previous_id,
                to: node_id,
                gauge_mm,
                electrification: None,
                speed_limit_kmh,
                length_m: Some(distance(previous_position, position)),
                curve_radius_m: None,
                gradient_per_mille: None,
                provenance: Some(provenance(
                    path,
                    SourceFormat::GeoJson,
                    format!("line:{line_index}:segment:{}", point_index - 1),
                )),
            });
            next_id += 1;
        }
        previous = Some((node_id, position));
    }
    Ok(())
}

fn geojson_lines<'a>(value: &'a Value, output: &mut Vec<(&'a [Value], Option<&'a Value>)>) {
    match value.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => {
            if let Some(features) = value.get("features").and_then(Value::as_array) {
                for feature in features {
                    let properties = feature.get("properties");
                    if let Some(geometry) = feature.get("geometry") {
                        geometry_lines(geometry, properties, output);
                    }
                }
            }
        }
        Some("Feature") => {
            if let Some(geometry) = value.get("geometry") {
                geometry_lines(geometry, value.get("properties"), output);
            }
        }
        _ => geometry_lines(value, None, output),
    }
}

fn geometry_lines<'a>(
    geometry: &'a Value,
    properties: Option<&'a Value>,
    output: &mut Vec<(&'a [Value], Option<&'a Value>)>,
) {
    match geometry.get("type").and_then(Value::as_str) {
        Some("LineString") => {
            if let Some(line) = geometry.get("coordinates").and_then(Value::as_array) {
                output.push((line, properties));
            }
        }
        Some("MultiLineString") => {
            if let Some(lines) = geometry.get("coordinates").and_then(Value::as_array) {
                for line in lines.iter().filter_map(Value::as_array) {
                    output.push((line, properties));
                }
            }
        }
        Some("GeometryCollection") => {
            if let Some(geometries) = geometry.get("geometries").and_then(Value::as_array) {
                for child in geometries {
                    geometry_lines(child, properties, output);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn import_geojson(path: &Path) -> Result<ImportResult, ImportError> {
    let text = fs::read_to_string(path).map_err(|error| {
        ImportError::new(
            "RW121_GEOJSON_READ_FAILED",
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    let value: Value = serde_json::from_str(&text).map_err(|error| {
        ImportError::new(
            "RW122_GEOJSON_PARSE_FAILED",
            format!("failed to parse {}: {error}", path.display()),
        )
    })?;
    let mut lines = Vec::new();
    geojson_lines(&value, &mut lines);
    if lines.is_empty() {
        return Err(ImportError::new(
            "RW123_GEOJSON_NO_TRACK",
            "GeoJSON contains no LineString or MultiLineString geometry",
        ));
    }

    let mut project = RailProject::new();
    project.metadata.title = path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_owned);
    project.metadata.description = Some("Imported from GeoJSON railway geometry".to_string());
    for (index, (line, properties)) in lines.iter().enumerate() {
        add_line(&mut project, path, index, line, *properties)?;
    }
    let mut result = ImportResult::new(project);
    result.diagnostics.push(Diagnostic::new(
        Severity::Info,
        "RW124_GEOJSON_IMPORTED",
        format!("imported {} railway line string(s)", lines.len()),
    ));
    if lines.len() > 1 {
        result.diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW125_GEOJSON_MULTIPLE_LINES",
            "multiple line strings were imported as separate graph components; OpenBVE export selects one driveable component",
        ));
    }
    Ok(result)
}

pub(crate) fn import_ir(path: &Path) -> Result<ImportResult, ImportError> {
    let bytes = fs::read(path).map_err(|error| {
        ImportError::new(
            "RW126_IR_READ_FAILED",
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    let mut result: ImportResult = serde_json::from_str(&decode_text(&bytes)).map_err(|error| {
        ImportError::new(
            "RW127_IR_PARSE_FAILED",
            format!(
                "failed to parse RailWeave interchange {}: {error}",
                path.display()
            ),
        )
    })?;
    result.diagnostics.push(Diagnostic::new(
        Severity::Info,
        "RW128_IR_LOADED",
        "loaded versioned RailWeave interchange without simulator-specific loss",
    ));
    Ok(result)
}

fn csv_value<'a>(headers: &[String], row: &'a [&str], names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| {
        headers
            .iter()
            .position(|header| header.eq_ignore_ascii_case(name))
            .and_then(|index| row.get(index).copied())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

pub(crate) fn import_track_csv(path: &Path) -> Result<ImportResult, ImportError> {
    let text = fs::read_to_string(path).map_err(|error| {
        ImportError::new(
            "RW130_CSV_READ_FAILED",
            format!("failed to read {}: {error}", path.display()),
        )
    })?;
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let headers: Vec<String> = lines
        .next()
        .ok_or_else(|| ImportError::new("RW131_CSV_EMPTY", "track CSV is empty"))?
        .split(',')
        .map(|value| value.trim().to_ascii_lowercase())
        .collect();
    for required in ["x", "z"] {
        if !headers.iter().any(|header| header == required) {
            return Err(ImportError::new(
                "RW132_CSV_HEADER",
                format!("track CSV requires {required:?} column"),
            ));
        }
    }

    let mut project = RailProject::new();
    project.metadata.title = path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_owned);
    project.metadata.description =
        Some("Imported through RailWeave track CSV exchange".to_string());
    let mut previous: Option<CsvTrackPoint> = None;
    let mut next_id = 1_u64;
    for (row_index, raw) in lines.enumerate() {
        let row: Vec<&str> = raw.split(',').collect();
        let parse = |names: &[&str], default: Option<f64>| -> Result<Option<f64>, ImportError> {
            match csv_value(&headers, &row, names) {
                Some(value) => value.parse::<f64>().map(Some).map_err(|_| {
                    ImportError::new(
                        "RW133_CSV_NUMBER",
                        format!(
                            "row {} has invalid {} value {value:?}",
                            row_index + 2,
                            names[0]
                        ),
                    )
                }),
                None => Ok(default),
            }
        };
        let position = Vec3 {
            x: parse(&["x", "east", "easting"], None)?.ok_or_else(|| {
                ImportError::new("RW134_CSV_COORDINATE", "track CSV row has no x value")
            })?,
            y: parse(&["y", "elevation", "height"], Some(0.0))?.unwrap_or(0.0),
            z: parse(&["z", "north", "northing"], None)?.ok_or_else(|| {
                ImportError::new("RW134_CSV_COORDINATE", "track CSV row has no z value")
            })?,
        };
        let line_name = csv_value(&headers, &row, &["line", "track", "path"])
            .unwrap_or("main")
            .to_string();
        let gauge_mm = parse(&["gauge_mm", "gauge"], None)?
            .filter(|value| *value > 0.0)
            .map(|value| value.round() as u32);
        let speed_limit_kmh = parse(&["speed_limit_kmh", "speed", "maxspeed"], None)?;
        let curve_radius_m = parse(&["curve_radius_m", "radius"], None)?;
        let gradient_per_mille = parse(&["gradient_per_mille", "gradient"], None)?;
        let node_id = next_id;
        next_id += 1;
        project.network.nodes.push(TrackNode {
            id: node_id,
            position,
            provenance: Some(provenance(
                path,
                SourceFormat::TrackCsv,
                format!("row:{}", row_index + 2),
            )),
        });
        if let Some(station_name) = csv_value(&headers, &row, &["station", "stop"]) {
            project.stations.push(Station {
                name: station_name.to_string(),
                node_id: Some(node_id),
                position_m: None,
                stop_time_s: parse(&["stop_time_s", "dwell"], Some(30.0))?.unwrap_or(30.0),
                provenance: Some(provenance(
                    path,
                    SourceFormat::TrackCsv,
                    format!("row:{}:station", row_index + 2),
                )),
            });
        }
        if let Some(previous) = previous.take() {
            if previous.line == line_name {
                project.network.edges.push(TrackEdge {
                    id: next_id,
                    from: previous.node_id,
                    to: node_id,
                    gauge_mm: previous.gauge_mm,
                    electrification: None,
                    speed_limit_kmh: previous.speed_limit_kmh,
                    length_m: Some(distance(previous.position, position)),
                    curve_radius_m: previous.curve_radius_m,
                    gradient_per_mille: previous.gradient_per_mille,
                    provenance: Some(provenance(
                        path,
                        SourceFormat::TrackCsv,
                        format!("row:{}:segment", row_index + 2),
                    )),
                });
                next_id += 1;
            }
        }
        previous = Some(CsvTrackPoint {
            line: line_name,
            node_id,
            position,
            gauge_mm,
            speed_limit_kmh,
            curve_radius_m,
            gradient_per_mille,
        });
    }
    if project.network.edges.is_empty() {
        return Err(ImportError::new(
            "RW135_CSV_NO_TRACK",
            "track CSV did not produce any connected segments",
        ));
    }
    let mut result = ImportResult::new(project);
    result.diagnostics.push(Diagnostic::new(
        Severity::Info,
        "RW136_CSV_IMPORTED",
        "imported portable track CSV with explicit metric coordinates",
    ));
    Ok(result)
}

pub(crate) fn import_game_bridge(
    root: &Path,
    detected_format: SourceFormat,
) -> Result<ImportResult, ImportError> {
    let mut candidates = entries(root);
    candidates.retain(|path| path.is_file());
    candidates.sort_by_key(|path| {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if name.ends_with(".railweave.json") {
            0
        } else if name.ends_with(".geojson") {
            1
        } else if name.ends_with(".railweave.csv") || name == "railweave-track.csv" {
            2
        } else {
            3
        }
    });
    let bridge = candidates.into_iter().find(|path| {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        name.ends_with(".railweave.json")
            || name.ends_with(".geojson")
            || name.ends_with(".railweave.csv")
            || name == "railweave-track.csv"
    });
    let Some(bridge) = bridge else {
        return Err(ImportError::new(
            "RW003_IMPORT_NEEDS_ADAPTER",
            format!(
                "{detected_format} was detected, but this installation needs either a bundled portable export (.railweave.json, .geojson or .railweave.csv) or `railweave convert --adapter <program>` for that proprietary route revision"
            ),
        ));
    };
    let name = bridge
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut result = if name.ends_with(".railweave.json") {
        import_ir(&bridge)?
    } else if name.ends_with(".geojson") {
        import_geojson(&bridge)?
    } else {
        import_track_csv(&bridge)?
    };
    result.diagnostics.push(Diagnostic::new(
        Severity::Info,
        "RW137_GAME_BRIDGE",
        format!(
            "detected {detected_format} source and imported its portable bridge {}",
            bridge.display()
        ),
    ));
    if detected_format == SourceFormat::Trainz {
        if let Some(config_path) = entries(root).into_iter().find(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case("config.txt"))
                .unwrap_or(false)
        }) {
            if let Ok(text) = fs::read_to_string(&config_path) {
                let parsed = parse_trainz_config(&text);
                if parsed.config.username.is_some() {
                    result.project.metadata.title = parsed.config.username.clone();
                }
                let identity = [
                    parsed.config.kuid.as_deref(),
                    parsed.config.trainz_build.as_deref(),
                    parsed.config.kind.as_deref(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ");
                result.diagnostics.push(Diagnostic::new(
                    Severity::Info,
                    "RW138_TRAINZ_METADATA",
                    format!(
                        "preserved Trainz asset metadata from {}{}",
                        config_path.display(),
                        if identity.is_empty() {
                            String::new()
                        } else {
                            format!(" ({identity})")
                        }
                    ),
                ));
                for diagnostic in parsed.diagnostics {
                    result.diagnostics.push(Diagnostic::new(
                        Severity::Warning,
                        "RW139_TRAINZ_METADATA_LOSS",
                        format!(
                            "{} line {}: {}",
                            config_path.display(),
                            diagnostic.line,
                            diagnostic.message
                        ),
                    ));
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn imports_geojson_line_with_properties() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("railweave-{nonce}.geojson"));
        fs::write(
            &path,
            r#"{"type":"Feature","properties":{"gauge_mm":1520,"speed_limit_kmh":80},"geometry":{"type":"LineString","coordinates":[[0,0,0],[0,100,2],[10,200,4]]}}"#,
        )
        .unwrap();
        let imported = import_geojson(&path).unwrap();
        assert_eq!(imported.project.network.nodes.len(), 3);
        assert_eq!(imported.project.network.edges.len(), 2);
        assert_eq!(imported.project.network.edges[0].gauge_mm, Some(1520));
        assert_eq!(
            imported.project.network.edges[0].speed_limit_kmh,
            Some(80.0)
        );
        fs::remove_file(path).ok();
    }

    #[test]
    fn track_csv_applies_row_state_to_the_following_segment() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("railweave-{nonce}.railweave.csv"));
        fs::write(
            &path,
            "x,y,z,gauge_mm,speed_limit_kmh,station\n0,0,0,1520,60,Origin\n0,1,100,1520,80,\n0,2,200,1520,100,Terminus\n",
        )
        .unwrap();
        let imported = import_track_csv(&path).unwrap();
        assert_eq!(imported.project.network.edges[0].gauge_mm, Some(1520));
        assert_eq!(
            imported.project.network.edges[0].speed_limit_kmh,
            Some(60.0)
        );
        assert_eq!(
            imported.project.network.edges[1].speed_limit_kmh,
            Some(80.0)
        );
        assert_eq!(imported.project.stations.len(), 2);
        fs::remove_file(path).ok();
    }

    #[test]
    fn trainz_bridge_preserves_native_config_identity() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("railweave-trainz-bridge-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("config.txt"),
            "kind map\nusername \"Synthetic Valley\"\nkuid <kuid:123:456>\ntrainz-build 4.6\n",
        )
        .unwrap();
        fs::write(root.join("railweave-track.csv"), "x,z\n0,0\n0,100\n").unwrap();

        let imported = import_game_bridge(&root, SourceFormat::Trainz).unwrap();
        assert_eq!(
            imported.project.metadata.title.as_deref(),
            Some("Synthetic Valley")
        );
        assert!(imported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RW138_TRAINZ_METADATA"));
        fs::remove_dir_all(root).ok();
    }
}
