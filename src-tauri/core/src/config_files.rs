//! Renders a game's declared config files (`GameConfig::config_files`) into a server's data directory:
//! each entry maps a file key to a `{{VAR}}` template resolved against the server's environment.

use std::collections::HashMap;
use std::path::{Component, Path};

use crate::types::{ConfigFileFormat, GameConfig};

/// Substitute every `{{VAR}}` in `template` with its value from `env`; unknown variables become "".
pub fn render_template(template: &str, env: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("}}") {
            Some(len) => {
                let key = rest[start + 2..start + 2 + len].trim();
                out.push_str(env.get(key).map(String::as_str).unwrap_or(""));
                rest = &rest[start + 2 + len + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Write the game's config files under `data_dir`. Files the game hasn't created yet are skipped (the
/// installer or the first start creates them and the next apply fills them in). Returns the relative
/// paths that changed.
pub fn apply_config_files(
    data_dir: &Path,
    game: &GameConfig,
    env: &HashMap<String, String>,
) -> std::io::Result<Vec<String>> {
    let mut written = Vec::new();
    for file in &game.config_files {
        if file.variables.is_empty() {
            continue;
        }
        let rel = Path::new(&file.path);
        // Never write outside the server directory, whatever a custom game definition says.
        if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir)) {
            continue;
        }
        let path = data_dir.join(rel);
        if !path.is_file() {
            continue;
        }
        let mut values: Vec<(String, String)> = file
            .variables
            .iter()
            .map(|(key, template)| (key.clone(), render_template(template, env)))
            .collect();
        values.sort();
        let current = std::fs::read_to_string(&path)?;
        let next = match file.format {
            ConfigFileFormat::Properties | ConfigFileFormat::Ini => set_key_values(&current, &values, '='),
            ConfigFileFormat::Yaml => set_key_values(&current, &values, ':'),
            ConfigFileFormat::Json => match set_json_values(&current, &values) {
                Some(text) => text,
                None => continue,
            },
        };
        if next != current {
            std::fs::write(&path, next)?;
            written.push(file.path.clone());
        }
    }
    Ok(written)
}

fn separator(sep: char) -> &'static str {
    if sep == ':' { ": " } else { "=" }
}

/// Line-based `key<sep>value` editing. Existing keys are replaced in place (comments, blank lines and
/// `[sections]` are kept) and Unreal-style `Name=(A=1,B=2)` tuples are edited inside the parentheses.
/// A key may be section-qualified as `[Section]Key`: it then only matches inside that section and is
/// added right under its header (the section is created at the end when missing). Unqualified keys
/// that appear nowhere go into the file's single tuple when it has one, else at the end.
fn set_key_values(text: &str, values: &[(String, String)], sep: char) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let targets: Vec<(Option<&str>, &str, &str)> = values
        .iter()
        .map(|(key, value)| {
            let (section, key) = split_section(key);
            (section, key, value.as_str())
        })
        .collect();
    let mut seen = vec![false; targets.len()];

    let mut current: Option<String> = None;
    for line in lines.iter_mut() {
        let trimmed = line.trim_start();
        if let Some(header) = trimmed.strip_prefix('[') {
            current = header.split(']').next().map(str::to_string);
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(pos) = trimmed.find(sep) {
            let key = trimmed[..pos].trim();
            let hit = targets
                .iter()
                .position(|(section, k, _)| *k == key && in_section(*section, current.as_deref()));
            if let Some(i) = hit {
                let indent = &line[..line.len() - trimmed.len()];
                *line = format!("{indent}{key}{}{}", separator(sep), targets[i].2);
                seen[i] = true;
                continue;
            }
        }
        for (i, (section, key, value)) in targets.iter().enumerate() {
            if seen[i] || !in_section(*section, current.as_deref()) {
                continue;
            }
            if let Some(updated) = replace_tuple_field(line, key, value) {
                *line = updated;
                seen[i] = true;
            }
        }
    }

    // Unqualified keys first: tuple edits and appends never shift earlier line indexes.
    let tuple_line = single_tuple_line(&lines);
    for (i, (section, key, value)) in targets.iter().enumerate() {
        if seen[i] || section.is_some() {
            continue;
        }
        match tuple_line {
            Some(t) if sep == '=' => lines[t] = insert_tuple_field(&lines[t], key, value),
            _ => lines.push(format!("{key}{}{value}", separator(sep))),
        }
    }
    for (i, (section, key, value)) in targets.iter().enumerate() {
        let Some(section) = section else { continue };
        if seen[i] {
            continue;
        }
        let entry = format!("{key}{}{value}", separator(sep));
        match section_header_index(&lines, section) {
            Some(header) => lines.insert(header + 1, entry),
            None => {
                if !lines.last().is_none_or(|l| l.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push(format!("[{section}]"));
                lines.push(entry);
            }
        }
    }

    let mut out = lines.join(eol);
    if text.ends_with('\n') || text.is_empty() {
        out.push_str(eol);
    }
    out
}

/// `[Section]Key` → (Some(section), key); anything else is unqualified.
fn split_section(key: &str) -> (Option<&str>, &str) {
    if let Some(rest) = key.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return (Some(&rest[..end]), &rest[end + 1..]);
        }
    }
    (None, key)
}

fn in_section(wanted: Option<&str>, current: Option<&str>) -> bool {
    wanted.is_none() || wanted == current
}

fn section_header_index(lines: &[String], section: &str) -> Option<usize> {
    lines.iter().position(|l| {
        let t = l.trim();
        t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) == Some(section)
    })
}

/// The only `Name=(...)` line in the file, when there is exactly one — missing keys belong inside it.
fn single_tuple_line(lines: &[String]) -> Option<usize> {
    let mut found = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.contains("=(") && t.ends_with(')') && !t.starts_with('#') && !t.starts_with(';') {
            if found.is_some() {
                return None;
            }
            found = Some(i);
        }
    }
    found
}

/// Add `key=value` before the tuple's closing parenthesis.
fn insert_tuple_field(line: &str, key: &str, value: &str) -> String {
    let close = line.rfind(')').unwrap_or(line.len());
    let body = &line[..close];
    let comma = if body.trim_end().ends_with('(') { "" } else { "," };
    format!("{body}{comma}{key}={value}{}", &line[close..])
}

/// `(key=` or `,key=` inside a tuple, value running to the next `,` or `)`.
fn replace_tuple_field(line: &str, key: &str, value: &str) -> Option<String> {
    for prefix in ['(', ','] {
        let needle = format!("{prefix}{key}=");
        if let Some(start) = line.find(&needle) {
            let vstart = start + needle.len();
            let vend = line[vstart..]
                .find([',', ')'])
                .map(|n| vstart + n)
                .unwrap_or(line.len());
            return Some(format!("{}{}{}", &line[..vstart], value, &line[vend..]));
        }
    }
    None
}

/// Set dotted-path keys on a JSON object document; `None` when the file isn't a JSON object.
fn set_json_values(text: &str, values: &[(String, String)]) -> Option<String> {
    let mut root: serde_json::Value = serde_json::from_str(text).ok()?;
    if !root.is_object() {
        return None;
    }
    for (key, value) in values {
        let parts: Vec<&str> = key.split('.').collect();
        let (last, parents) = parts.split_last()?;
        let mut cursor = &mut root;
        for part in parents {
            cursor = cursor
                .as_object_mut()?
                .entry((*part).to_string())
                .or_insert_with(|| serde_json::Value::Object(Default::default()));
        }
        cursor.as_object_mut()?.insert((*last).to_string(), json_scalar(value));
    }
    serde_json::to_string_pretty(&root).ok()
}

/// Booleans and numbers keep their JSON type; everything else is a string.
fn json_scalar(value: &str) -> serde_json::Value {
    match value {
        "true" => serde_json::Value::Bool(true),
        "false" => serde_json::Value::Bool(false),
        _ => value
            .parse::<i64>()
            .map(serde_json::Value::from)
            .or_else(|_| value.parse::<f64>().map(serde_json::Value::from))
            .unwrap_or_else(|_| serde_json::Value::String(value.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn renders_templates_and_drops_unknown_vars() {
        let e = env(&[("MAX", "20"), ("NAME", "Forge")]);
        assert_eq!(render_template("{{NAME}} x {{MAX}} {{NOPE}}", &e), "Forge x 20 ");
        assert_eq!(render_template("plain", &e), "plain");
        assert_eq!(render_template("{{unterminated", &e), "{{unterminated");
    }

    #[test]
    fn properties_replace_in_place_and_append_missing() {
        let text = "# comment\nmax-players=20\ndifficulty=easy\nserver-port=25565\n";
        let values = vec![
            ("difficulty".to_string(), "hard".to_string()),
            ("motd".to_string(), "A=B".to_string()),
        ];
        let out = set_key_values(text, &values, '=');
        assert_eq!(out, "# comment\nmax-players=20\ndifficulty=hard\nserver-port=25565\nmotd=A=B\n");
    }

    #[test]
    fn ini_tuple_fields_are_edited_inside_the_parentheses() {
        let text = "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(Difficulty=None,RCONEnabled=False,RCONPort=25575)\n";
        let values = vec![("RCONEnabled".to_string(), "True".to_string())];
        let out = set_key_values(text, &values, '=');
        assert_eq!(out, "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(Difficulty=None,RCONEnabled=True,RCONPort=25575)\n");
    }

    #[test]
    fn ini_missing_tuple_field_is_inserted_into_the_tuple() {
        let text = "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(Difficulty=None,RCONPort=25575)\n";
        let values = vec![("RCONEnabled".to_string(), "True".to_string())];
        let out = set_key_values(text, &values, '=');
        assert_eq!(out, "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(Difficulty=None,RCONPort=25575,RCONEnabled=True)\n");
    }

    #[test]
    fn ini_section_qualified_keys_stay_in_their_section() {
        let text = "[A]\nTimeout=1\n\n[/Script/Net.Driver]\nTimeout=1\n";
        let values = vec![
            ("[/Script/Net.Driver]Timeout".to_string(), "300".to_string()),
            ("[/Script/Net.Driver]Initial".to_string(), "60".to_string()),
            ("[Missing]Flag".to_string(), "on".to_string()),
        ];
        let out = set_key_values(text, &values, '=');
        assert_eq!(
            out,
            "[A]\nTimeout=1\n\n[/Script/Net.Driver]\nInitial=60\nTimeout=300\n\n[Missing]\nFlag=on\n"
        );
    }

    #[test]
    fn json_sets_typed_values_and_nested_paths() {
        let text = r#"{"MaxPlayers": 10, "Nested": {"Keep": true}}"#;
        let values = vec![
            ("MaxPlayers".to_string(), "32".to_string()),
            ("Nested.Flag".to_string(), "false".to_string()),
            ("Name".to_string(), "Forge".to_string()),
        ];
        let out: serde_json::Value = serde_json::from_str(&set_json_values(text, &values).unwrap()).unwrap();
        assert_eq!(out["MaxPlayers"], 32);
        assert_eq!(out["Nested"]["Keep"], true);
        assert_eq!(out["Nested"]["Flag"], false);
        assert_eq!(out["Name"], "Forge");
        assert!(set_json_values("[1,2]", &values).is_none());
    }
}
