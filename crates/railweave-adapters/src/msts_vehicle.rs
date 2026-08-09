use crate::detectors::decode_text;
use railweave_core::{
    AssetKind, Diagnostic, ImportResult, Provenance, RollingStockVehicle, Severity, SourceFormat,
};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
enum UnitKind {
    Mass,
    Distance,
    Force,
    Power,
    Speed,
}

#[derive(Debug, Default)]
struct ParsedVehicle {
    name: Option<String>,
    vehicle_type: Option<String>,
    mass_kg: Option<f64>,
    width_m: Option<f64>,
    height_m: Option<f64>,
    length_m: Option<f64>,
    axle_count: Option<u32>,
    wheel_count: Option<f64>,
    brake_system_type: Option<String>,
    brake_equipment_type: Option<String>,
    max_brake_force_n: Option<f64>,
    max_power_w: Option<f64>,
    max_tractive_force_n: Option<f64>,
    max_continuous_force_n: Option<f64>,
    max_velocity_mps: Option<f64>,
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

fn block_values<'a>(tokens: &'a [String], name: &str) -> Option<&'a [String]> {
    let (open, close) = find_block(tokens, name)?;
    Some(&tokens[open + 1..close])
}

fn first_value(tokens: &[String], name: &str) -> Option<String> {
    block_values(tokens, name)?.first().cloned()
}

fn split_quantity(raw: &str) -> Result<(f64, String), String> {
    let raw = raw.split('#').next().unwrap_or(raw).trim();
    if raw.is_empty() {
        return Err("empty quantity".to_string());
    }

    let bytes = raw.as_bytes();
    let mut index = 0usize;
    let mut saw_digit = false;
    while index < bytes.len() {
        let ch = bytes[index] as char;
        if ch.is_ascii_digit() {
            saw_digit = true;
            index += 1;
            continue;
        }
        if matches!(ch, '+' | '-' | '.' | ',') {
            index += 1;
            continue;
        }
        if matches!(ch, 'e' | 'E') && saw_digit {
            index += 1;
            if index < bytes.len() && matches!(bytes[index] as char, '+' | '-') {
                index += 1;
            }
            continue;
        }
        break;
    }

    if !saw_digit || index == 0 {
        return Err(format!("invalid numeric value {raw:?}"));
    }
    let number = raw[..index].replace(',', ".");
    let value: f64 = number
        .parse()
        .map_err(|_| format!("invalid numeric value {raw:?}"))?;
    if !value.is_finite() {
        return Err(format!("non-finite numeric value {raw:?}"));
    }
    Ok((value, raw[index..].trim().to_ascii_lowercase()))
}

fn quantity(raw: &str, kind: UnitKind) -> Result<f64, String> {
    let (value, suffix) = split_quantity(raw)?;
    let scale = match kind {
        UnitKind::Mass => match suffix.as_str() {
            "" | "kg" => 1.0,
            "lb" => 0.453_592_37,
            "t" => 1_000.0,
            "t-uk" => 1_016.05,
            "t-us" => 907.184_74,
            _ => return Err(format!("unsupported mass unit {suffix:?} in {raw:?}")),
        },
        UnitKind::Distance => match suffix.as_str() {
            "" | "m" => 1.0,
            "cm" => 0.01,
            "mm" => 0.001,
            "km" => 1_000.0,
            "ft" => 0.3048,
            "in" => 0.0254,
            "in/2" => 0.0127,
            _ => return Err(format!("unsupported distance unit {suffix:?} in {raw:?}")),
        },
        UnitKind::Force => match suffix.as_str() {
            "" | "n" => 1.0,
            "kn" => 1_000.0,
            "lbf" | "lb" => 4.448_221_62,
            _ => return Err(format!("unsupported force unit {suffix:?} in {raw:?}")),
        },
        UnitKind::Power => match suffix.as_str() {
            "" | "w" => 1.0,
            "kw" => 1_000.0,
            "mw" => 1_000_000.0,
            "hp" => 745.699_872,
            _ => return Err(format!("unsupported power unit {suffix:?} in {raw:?}")),
        },
        UnitKind::Speed => match suffix.as_str() {
            "" | "m/s" | "mps" => 1.0,
            "km/h" | "kmh" | "kph" => 1.0 / 3.6,
            "mph" => 0.447_04,
            _ => return Err(format!("unsupported speed unit {suffix:?} in {raw:?}")),
        },
    };
    Ok(value * scale)
}

fn parse_quantity_field(
    tokens: &[String],
    field: &str,
    kind: UnitKind,
    warnings: &mut Vec<String>,
) -> Option<f64> {
    let raw = first_value(tokens, field)?;
    match quantity(&raw, kind) {
        Ok(value) => Some(value),
        Err(error) => {
            warnings.push(format!("{field}: {error}"));
            None
        }
    }
}

fn parse_vehicle(text: &str) -> Result<(ParsedVehicle, Vec<String>), String> {
    let tokens = stf_tokens(text);
    let (wagon_open, wagon_close) = find_block(&tokens, "wagon")
        .ok_or_else(|| "vehicle file has no Wagon block".to_string())?;
    let wagon = &tokens[wagon_open + 1..wagon_close];
    let mut warnings = Vec::new();

    let mass_kg = parse_quantity_field(wagon, "mass", UnitKind::Mass, &mut warnings);
    let max_brake_force_n =
        parse_quantity_field(wagon, "maxbrakeforce", UnitKind::Force, &mut warnings);
    let max_power_w = parse_quantity_field(wagon, "maxpower", UnitKind::Power, &mut warnings);
    let max_tractive_force_n =
        parse_quantity_field(wagon, "maxforce", UnitKind::Force, &mut warnings);
    let max_continuous_force_n =
        parse_quantity_field(wagon, "maxcontinuousforce", UnitKind::Force, &mut warnings);
    let max_velocity_mps =
        parse_quantity_field(wagon, "maxvelocity", UnitKind::Speed, &mut warnings);

    let (width_m, height_m, length_m) = if let Some(size) = block_values(wagon, "size") {
        if size.len() < 3 {
            warnings.push("Size: expected width, height and length".to_string());
            (None, None, None)
        } else {
            let mut parsed = [None, None, None];
            for (index, raw) in size.iter().take(3).enumerate() {
                match quantity(raw, UnitKind::Distance) {
                    Ok(value) => parsed[index] = Some(value),
                    Err(error) => warnings.push(format!("Size[{}]: {error}", index + 1)),
                }
            }
            (parsed[0], parsed[1], parsed[2])
        }
    } else {
        (None, None, None)
    };

    let axle_count = first_value(wagon, "ortsnumberaxles").and_then(|raw| match raw.parse() {
        Ok(value) => Some(value),
        Err(_) => {
            warnings.push(format!("ORTSNumberAxles: invalid integer {raw:?}"));
            None
        }
    });
    let wheel_count = first_value(wagon, "numwheels").and_then(|raw| match raw.parse::<f64>() {
        Ok(value) if value.is_finite() => Some(value),
        _ => {
            warnings.push(format!("NumWheels: invalid number {raw:?}"));
            None
        }
    });

    Ok((
        ParsedVehicle {
            name: first_value(wagon, "name"),
            vehicle_type: first_value(wagon, "type"),
            mass_kg,
            width_m,
            height_m,
            length_m,
            axle_count,
            wheel_count,
            brake_system_type: first_value(wagon, "brakesystemtype"),
            brake_equipment_type: first_value(wagon, "brakeequipmenttype"),
            max_brake_force_n,
            max_power_w,
            max_tractive_force_n,
            max_continuous_force_n,
            max_velocity_mps,
        },
        warnings,
    ))
}

fn vehicle_provenance(path: &Path, source_id: impl Into<String>) -> Provenance {
    Provenance {
        source_format: SourceFormat::MstsOpenRails,
        source_path: path.to_path_buf(),
        source_id: Some(source_id.into()),
    }
}

pub(crate) fn enrich_vehicle_metadata(result: &mut ImportResult) {
    let mut parsed_count = 0usize;
    let mut seen = HashSet::new();
    let assets = result.project.assets.clone();

    for asset in assets {
        if asset.kind != AssetKind::RollingStock
            || asset.provenance.source_format != SourceFormat::MstsOpenRails
            || !seen.insert(asset.id)
        {
            continue;
        }
        let path = &asset.provenance.source_path;
        let supported_extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| {
                extension.eq_ignore_ascii_case("eng") || extension.eq_ignore_ascii_case("wag")
            })
            .unwrap_or(false);
        if !supported_extension || !path.exists() {
            continue;
        }

        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                result.diagnostics.push(
                    Diagnostic::new(
                        Severity::Warning,
                        "RW228_MSTS_VEHICLE_READ_FAILED",
                        format!("failed to read {}: {error}", path.display()),
                    )
                    .with_provenance(vehicle_provenance(path, "Wagon")),
                );
                continue;
            }
        };
        let (vehicle, warnings) = match parse_vehicle(&decode_text(&bytes)) {
            Ok(parsed) => parsed,
            Err(error) => {
                result.diagnostics.push(
                    Diagnostic::new(
                        Severity::Warning,
                        "RW227_MSTS_VEHICLE_PARSE_FAILED",
                        format!("failed to parse {}: {error}", path.display()),
                    )
                    .with_provenance(vehicle_provenance(path, "Wagon")),
                );
                continue;
            }
        };

        for warning in warnings {
            result.diagnostics.push(
                Diagnostic::new(
                    Severity::Warning,
                    "RW229_MSTS_VEHICLE_VALUE_UNSUPPORTED",
                    format!("{}: {warning}", path.display()),
                )
                .with_provenance(vehicle_provenance(path, "Wagon")),
            );
        }

        result.project.vehicles.push(RollingStockVehicle {
            asset_id: asset.id,
            name: vehicle.name,
            vehicle_type: vehicle.vehicle_type,
            mass_kg: vehicle.mass_kg,
            width_m: vehicle.width_m,
            height_m: vehicle.height_m,
            length_m: vehicle.length_m,
            axle_count: vehicle.axle_count,
            wheel_count: vehicle.wheel_count,
            brake_system_type: vehicle.brake_system_type,
            brake_equipment_type: vehicle.brake_equipment_type,
            max_brake_force_n: vehicle.max_brake_force_n,
            max_power_w: vehicle.max_power_w,
            max_tractive_force_n: vehicle.max_tractive_force_n,
            max_continuous_force_n: vehicle.max_continuous_force_n,
            max_velocity_mps: vehicle.max_velocity_mps,
        });
        parsed_count += 1;
    }

    if parsed_count > 0 {
        result.diagnostics.push(Diagnostic::new(
            Severity::Info,
            "RW230_MSTS_VEHICLE_METADATA",
            format!(
                "parsed basic physical/brake metadata for {parsed_count} MSTS/OpenRails ENG/WAG vehicle file(s)"
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use railweave_core::{AssetRef, ImportResult, RailProject};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("railweave-vehicle-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn parses_metric_vehicle_metadata() {
        let (vehicle, warnings) = parse_vehicle(
            r#"Wagon (
  Motor
  Name ( "Motor car" )
  Type ( Engine )
  Mass ( 48t )
  Size ( 2.8m 4.0m 20m )
  ORTSNumberAxles ( 4 )
  NumWheels ( 8 )
  BrakeSystemType ( "Air_single_pipe" )
  BrakeEquipmentType ( "Handbrake,Triple_valve" )
  MaxBrakeForce ( 120kN )
)"#,
        )
        .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(vehicle.name.as_deref(), Some("Motor car"));
        assert_eq!(vehicle.vehicle_type.as_deref(), Some("Engine"));
        assert_eq!(vehicle.mass_kg, Some(48_000.0));
        assert_eq!(vehicle.length_m, Some(20.0));
        assert_eq!(vehicle.axle_count, Some(4));
        assert_eq!(vehicle.wheel_count, Some(8.0));
        assert_eq!(vehicle.max_brake_force_n, Some(120_000.0));
    }

    #[test]
    fn parses_openrails_compatible_imperial_units() {
        let (vehicle, warnings) = parse_vehicle(
            r#"Wagon (
  Car
  Mass ( 100000lb )
  Size ( 9ft 13ft 65ft )
  MaxBrakeForce ( 30000lbf )
)"#,
        )
        .unwrap();
        assert!(warnings.is_empty());
        assert!((vehicle.mass_kg.unwrap() - 45_359.237).abs() < 0.001);
        assert!((vehicle.length_m.unwrap() - 19.812).abs() < 0.001);
        assert!((vehicle.max_brake_force_n.unwrap() - 133_446.648_6).abs() < 0.001);
    }

    #[test]
    fn enriches_existing_rolling_stock_asset() {
        let root = fixture();
        let path = root.join("Motor.eng");
        fs::write(
            &path,
            "Wagon ( Motor Name ( \"EMU motor\" ) Mass ( 50t ) Size ( 3m 4m 21m ) MaxBrakeForce ( 100kN ) )",
        )
        .unwrap();
        let mut project = RailProject::new();
        project.assets.push(AssetRef {
            id: 42,
            kind: AssetKind::RollingStock,
            name: Some("Motor".to_string()),
            provenance: Provenance {
                source_format: SourceFormat::MstsOpenRails,
                source_path: path,
                source_id: None,
            },
        });
        let mut result = ImportResult::new(project);
        enrich_vehicle_metadata(&mut result);
        assert_eq!(result.project.vehicles.len(), 1);
        assert_eq!(result.project.vehicles[0].asset_id, 42);
        assert_eq!(result.project.vehicles[0].mass_kg, Some(50_000.0));
        assert_eq!(result.project.vehicles[0].length_m, Some(21.0));
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RW230_MSTS_VEHICLE_METADATA"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn unsupported_units_are_reported_without_guessing() {
        let (vehicle, warnings) = parse_vehicle("Wagon ( Car Mass ( 12stone ) )").unwrap();
        assert_eq!(vehicle.mass_kg, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unsupported mass unit"));
    }
}
