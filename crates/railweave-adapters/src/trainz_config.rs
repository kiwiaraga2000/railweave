use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrainzConfig {
    pub kind: Option<String>,
    pub username: Option<String>,
    pub kuid: Option<String>,
    pub trainz_build: Option<String>,
    pub map_kuid: Option<String>,
    pub unknown_keys: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainzConfigDiagnostic {
    pub line: usize,
    pub key: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrainzConfigParse {
    pub config: TrainzConfig,
    pub diagnostics: Vec<TrainzConfigDiagnostic>,
}

pub fn parse_trainz_config(input: &str) -> TrainzConfigParse {
    let mut out = TrainzConfigParse::default();
    let mut container_depth = 0usize;
    let mut seen_top_level_keys = BTreeSet::new();
    let lines: Vec<&str> = input.lines().collect();

    for (index, raw_line) in lines.iter().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        // Trainz config.txt is ACS text, not a programming-language source file.
        // Content Manager does not support comment lines, so accepting familiar
        // comment markers here would make an invalid asset look clean.
        if looks_like_comment(line) {
            out.diagnostics.push(TrainzConfigDiagnostic {
                line: line_number,
                key: None,
                message: "Trainz config does not support comment lines".into(),
            });
            continue;
        }

        if line == "}" {
            if container_depth == 0 {
                out.diagnostics.push(TrainzConfigDiagnostic {
                    line: line_number,
                    key: None,
                    message: "unmatched Trainz container closing brace".into(),
                });
            } else {
                container_depth -= 1;
            }
            continue;
        }

        if line == "{" || line.ends_with('{') {
            container_depth += 1;
            continue;
        }

        // Nested Trainz containers describe structured asset metadata. The native
        // adapter does not map their contents yet, but braces and child entries are
        // valid syntax and must not be reported as malformed top-level metadata.
        if container_depth > 0 {
            continue;
        }

        let Some((raw_key, raw_value)) = split_key_value(line) else {
            let opens_container = lines[index + 1..]
                .iter()
                .map(|candidate| candidate.trim())
                .find(|candidate| !candidate.is_empty())
                == Some("{");
            if opens_container {
                continue;
            }
            out.diagnostics.push(TrainzConfigDiagnostic {
                line: line_number,
                key: None,
                message: "unsupported Trainz config line; expected key/value metadata".into(),
            });
            continue;
        };

        let raw_key = raw_key.trim();
        let key = raw_key.to_ascii_lowercase();
        if raw_key.bytes().any(|byte| byte.is_ascii_uppercase()) {
            out.diagnostics.push(TrainzConfigDiagnostic {
                line: line_number,
                key: Some(key.clone()),
                message: "uppercase ASCII is not valid in an ACS Text Format key".into(),
            });
        }

        if !seen_top_level_keys.insert(key.clone()) {
            out.diagnostics.push(TrainzConfigDiagnostic {
                line: line_number,
                key: Some(key),
                message: "duplicate Trainz metadata key in the same scope".into(),
            });
            continue;
        }

        let value = unquote(raw_value.trim()).to_string();
        if value.is_empty() {
            out.diagnostics.push(TrainzConfigDiagnostic {
                line: line_number,
                key: Some(key.clone()),
                message: "empty Trainz metadata value".into(),
            });
        }

        match key.as_str() {
            "kind" => out.config.kind = Some(value),
            "username" => out.config.username = Some(value),
            "kuid" => out.config.kuid = Some(value),
            "trainz-build" => out.config.trainz_build = Some(value),
            "map-kuid" | "map_kuid" => out.config.map_kuid = Some(value),
            _ => {
                out.config.unknown_keys.insert(key.clone(), value);
                out.diagnostics.push(TrainzConfigDiagnostic {
                    line: line_number,
                    key: Some(key),
                    message:
                        "Trainz metadata key is preserved but not mapped by the native adapter yet"
                            .into(),
                });
            }
        }
    }

    if container_depth > 0 {
        out.diagnostics.push(TrainzConfigDiagnostic {
            line: lines.len().max(1),
            key: None,
            message: "unterminated Trainz metadata container".into(),
        });
    }

    out
}

fn looks_like_comment(line: &str) -> bool {
    line.starts_with("//")
        || line.starts_with('#')
        || line.starts_with(';')
        || line.eq_ignore_ascii_case("rem")
        || line
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("rem "))
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.splitn(2, char::is_whitespace);
    let key = parts.next()?;
    let value = parts.next()?.trim();
    Some((key, value))
}

fn unquote(value: &str) -> &str {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_route_metadata_without_assets() {
        let parsed = parse_trainz_config(
            r#"
            kind map
            username "Synthetic Valley"
            kuid <kuid:123:456>
            trainz-build 4.6
            map-kuid <kuid:123:999>
            "#,
        );

        assert_eq!(parsed.config.kind.as_deref(), Some("map"));
        assert_eq!(parsed.config.username.as_deref(), Some("Synthetic Valley"));
        assert_eq!(parsed.config.kuid.as_deref(), Some("<kuid:123:456>"));
        assert_eq!(parsed.config.trainz_build.as_deref(), Some("4.6"));
        assert_eq!(parsed.config.map_kuid.as_deref(), Some("<kuid:123:999>"));
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn preserves_unknown_fields_and_reports_loss_explicitly() {
        let parsed = parse_trainz_config("kind map\ncustom-track-rule keep-me\n");
        assert_eq!(
            parsed
                .config
                .unknown_keys
                .get("custom-track-rule")
                .map(String::as_str),
            Some("keep-me")
        );
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(
            parsed.diagnostics[0].key.as_deref(),
            Some("custom-track-rule")
        );
    }

    #[test]
    fn malformed_lines_do_not_disappear_silently() {
        let parsed = parse_trainz_config("kind map\nbroken_line\nusername Test\n");
        assert_eq!(parsed.config.username.as_deref(), Some("Test"));
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].line, 2);
    }

    #[test]
    fn accepts_nested_trainz_containers_without_false_malformed_diagnostics() {
        let parsed = parse_trainz_config(
            r#"
            kind map
            username "Synthetic Valley"
            kuid <kuid:123:456>
            obsolete-table
            {
                0 <kuid:123:100>
                1 <kuid:123:101>
            }
            trainz-build 4.6
            "#,
        );

        assert_eq!(parsed.config.kind.as_deref(), Some("map"));
        assert_eq!(parsed.config.trainz_build.as_deref(), Some("4.6"));
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn accepts_container_header_separated_by_blank_lines() {
        let parsed = parse_trainz_config(
            r#"
            kind map
            obsolete-table

            {
                0 <kuid:123:100>
            }
            username Test
            "#,
        );

        assert_eq!(parsed.config.username.as_deref(), Some("Test"));
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn reports_comment_like_lines_instead_of_accepting_them() {
        let parsed = parse_trainz_config(
            "kind map\n// not a Trainz comment\n; neither is this\nrem nor this\n# nor this\nusername Test\n",
        );

        assert_eq!(parsed.config.username.as_deref(), Some("Test"));
        assert_eq!(parsed.diagnostics.len(), 4);
        assert!(parsed
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.contains("does not support comment lines")));
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.line)
                .collect::<Vec<_>>(),
            vec![2, 3, 4, 5]
        );
    }

    #[test]
    fn comment_like_line_cannot_bridge_a_container_header_to_its_brace() {
        let parsed = parse_trainz_config(
            "kind map\nobsolete-table\n// invalid separator\n{\n0 <kuid:1:2>\n}\n",
        );

        assert_eq!(parsed.diagnostics.len(), 2);
        assert!(parsed.diagnostics[0].message.contains("expected key/value"));
        assert!(parsed.diagnostics[1]
            .message
            .contains("does not support comment lines"));
    }

    #[test]
    fn duplicate_top_level_keys_are_reported_without_overwriting_the_first_value() {
        let parsed = parse_trainz_config("kind map\nusername First\nkind scenery\nusername Second\n");

        assert_eq!(parsed.config.kind.as_deref(), Some("map"));
        assert_eq!(parsed.config.username.as_deref(), Some("First"));
        assert_eq!(parsed.diagnostics.len(), 2);
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.line)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert!(parsed
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.contains("duplicate Trainz metadata key")));
    }

    #[test]
    fn uppercase_ascii_keys_are_reported_without_hiding_the_metadata() {
        let parsed = parse_trainz_config("Kind map\nUserName Test\n");

        assert_eq!(parsed.config.kind.as_deref(), Some("map"));
        assert_eq!(parsed.config.username.as_deref(), Some("Test"));
        assert_eq!(parsed.diagnostics.len(), 2);
        assert!(parsed
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.contains("uppercase ASCII")));
    }

    #[test]
    fn reports_unbalanced_container_braces() {
        let unterminated = parse_trainz_config("kind map\nobsolete-table\n{\n0 <kuid:1:2>\n");
        assert!(unterminated
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("unterminated")));

        let unmatched = parse_trainz_config("kind map\n}\n");
        assert!(unmatched
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("unmatched")));
    }
}
