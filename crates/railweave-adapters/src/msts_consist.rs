use crate::detectors::{decode_text, entries};
use railweave_core::{
    AssetKind, AssetRef, Diagnostic, ImportResult, Provenance, Severity, SourceFormat,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConsistMember {
    name: String,
    folder: String,
    is_engine: bool,
    flipped: bool,
    uid: Option<i32>,
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

fn parse_member(tokens: &[String], is_engine: bool) -> Option<ConsistMember> {
    let data_name = if is_engine { "enginedata" } else { "wagondata" };
    let (data_open, data_close) = find_block(tokens, data_name)?;
    if data_close <= data_open + 2 {
        return None;
    }
    let name = tokens.get(data_open + 1)?.clone();
    let folder = tokens.get(data_open + 2)?.clone();
    let flipped = find_block(tokens, "flip").is_some();
    let uid = find_block(tokens, "uid")
        .and_then(|(open, _)| tokens.get(open + 1))
        .and_then(|value| value.parse().ok());

    Some(ConsistMember {
        name,
        folder,
        is_engine,
        flipped,
        uid,
    })
}

fn parse_members(text: &str) -> Vec<ConsistMember> {
    let tokens = stf_tokens(text);
    let mut members = Vec::new();
    let mut cursor = 0usize;

    while cursor + 1 < tokens.len() {
        let is_engine = tokens[cursor].eq_ignore_ascii_case("engine");
        let is_wagon = tokens[cursor].eq_ignore_ascii_case("wagon");
        if (is_engine || is_wagon) && tokens.get(cursor + 1).map(String::as_str) == Some("(") {
            let open = cursor + 1;
            let Some(close) = matching_paren(&tokens, open) else {
                break;
            };
            if let Some(member) = parse_member(&tokens[open + 1..close], is_engine) {
                members.push(member);
            }
            cursor = close + 1;
            continue;
        }
        cursor += 1;
    }

    members
}

fn con_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = entries(root)
        .into_iter()
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(|extension| extension.eq_ignore_ascii_case("con"))
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

fn child_ignore_ascii_case(parent: &Path, name: &str) -> Option<PathBuf> {
    fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| {
            entry
                .file_name()
                .to_str()
                .filter(|candidate| candidate.eq_ignore_ascii_case(name))
                .map(|_| entry.path())
        })
}

fn trainset_root(con_file: &Path) -> Option<PathBuf> {
    for ancestor in con_file.ancestors() {
        if ancestor
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.eq_ignore_ascii_case("trains"))
            .unwrap_or(false)
        {
            return child_ignore_ascii_case(ancestor, "trainset");
        }
    }
    None
}

fn member_path(con_file: &Path, member: &ConsistMember) -> PathBuf {
    let extension = if member.is_engine { "eng" } else { "wag" };
    let file_name = format!("{}.{}", member.name, extension);
    if let Some(trainset) = trainset_root(con_file) {
        return trainset.join(&member.folder).join(file_name);
    }
    con_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&member.folder)
        .join(file_name)
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

pub(crate) fn enrich_consists(root: &Path, result: &mut ImportResult) {
    let files = con_files(root);
    if files.is_empty() {
        return;
    }

    let mut next_id = max_entity_id(result).saturating_add(1);
    let mut parsed_members = 0usize;
    let mut unresolved = 0usize;
    let mut consist_count = 0usize;
    let mut seen_member_keys = HashSet::new();

    for con_file in files {
        let bytes = match fs::read(&con_file) {
            Ok(bytes) => bytes,
            Err(error) => {
                result.diagnostics.push(
                    Diagnostic::new(
                        Severity::Warning,
                        "RW224_MSTS_CONSIST_READ_FAILED",
                        format!("failed to read {}: {error}", con_file.display()),
                    )
                    .with_provenance(Provenance {
                        source_format: SourceFormat::MstsOpenRails,
                        source_path: con_file.clone(),
                        source_id: None,
                    }),
                );
                continue;
            }
        };
        let members = parse_members(&decode_text(&bytes));
        if members.is_empty() {
            continue;
        }
        consist_count += 1;
        let consist_name = con_file
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("consist");

        for (index, member) in members.iter().enumerate() {
            parsed_members += 1;
            let path = member_path(&con_file, member);
            if !path.exists() {
                unresolved += 1;
            }

            let key = format!(
                "{}:{}:{}:{}:{}",
                con_file.display(),
                index,
                member.folder,
                member.name,
                member.is_engine
            );
            if !seen_member_keys.insert(key) {
                continue;
            }

            let role = if member.is_engine { "engine" } else { "wagon" };
            let uid = member
                .uid
                .map(|uid| uid.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            result.project.assets.push(AssetRef {
                id: next_id,
                kind: AssetKind::RollingStock,
                name: Some(format!("{}/{}", member.folder, member.name)),
                provenance: Provenance {
                    source_format: SourceFormat::MstsOpenRails,
                    source_path: path,
                    source_id: Some(format!(
                        "consist={consist_name}:member={index}:role={role}:uid={uid}:flipped={}",
                        member.flipped
                    )),
                },
            });
            next_id = next_id.saturating_add(1);
        }
    }

    if parsed_members > 0 {
        result.diagnostics.push(Diagnostic::new(
            Severity::Info,
            "RW225_MSTS_CONSIST_MEMBERS",
            format!(
                "resolved {parsed_members} ordered member reference(s) from {consist_count} MSTS consist file(s); member order, engine/wagon role and flip state are preserved in provenance"
            ),
        ));
    }
    if unresolved > 0 {
        result.diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "RW226_MSTS_CONSIST_MEMBER_MISSING",
            format!(
                "{unresolved} consist member file reference(s) could not be found in the detected TRAINS/TRAINSET layout"
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use railweave_core::{ImportResult, RailProject};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "railweave-consist-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn parses_engine_wagon_order_and_flip() {
        let members = parse_members(
            r#"Train (
  TrainCfg (
    Demo
    Engine ( UiD ( 1 ) EngineData ( Motor EMU ) )
    Wagon ( Flip ( ) WagonData ( Trailer EMU ) UiD ( 2 ) )
  )
)"#,
        );
        assert_eq!(members.len(), 2);
        assert!(members[0].is_engine);
        assert_eq!(members[0].name, "Motor");
        assert!(!members[0].flipped);
        assert!(!members[1].is_engine);
        assert_eq!(members[1].name, "Trailer");
        assert!(members[1].flipped);
        assert_eq!(members[1].uid, Some(2));
    }

    #[test]
    fn resolves_standard_trainset_layout() {
        let root = fixture();
        let consists = root.join("TRAINS").join("CONSISTS");
        let trainset = root.join("TRAINS").join("TRAINSET").join("EMU");
        fs::create_dir_all(&consists).unwrap();
        fs::create_dir_all(&trainset).unwrap();
        fs::write(
            consists.join("demo.con"),
            r#"Train ( TrainCfg ( Demo
Engine ( UiD ( 1 ) EngineData ( Motor EMU ) )
Wagon ( Flip ( ) WagonData ( Trailer EMU ) UiD ( 2 ) )
) )"#,
        )
        .unwrap();
        fs::write(trainset.join("Motor.eng"), "Wagon ( Motor )").unwrap();
        fs::write(trainset.join("Trailer.wag"), "Wagon ( Trailer )").unwrap();

        let mut result = ImportResult::new(RailProject::new());
        enrich_consists(&root, &mut result);
        assert_eq!(result.project.assets.len(), 2);
        assert!(result.project.assets[0]
            .provenance
            .source_path
            .ends_with("TRAINS/TRAINSET/EMU/Motor.eng"));
        assert!(result.project.assets[1]
            .provenance
            .source_id
            .as_deref()
            .unwrap_or_default()
            .contains("member=1:role=wagon:uid=2:flipped=true"));
        assert!(!result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RW226_MSTS_CONSIST_MEMBER_MISSING"));
        fs::remove_dir_all(root).ok();
    }
}
