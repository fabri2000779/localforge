//! Renders a game's declared config files (`GameConfig::config_files`) into a server's data directory:
//! each entry maps a file key to a `{{VAR}}` template resolved against the server's environment.

use std::collections::HashMap;
use std::path::{Component, Path};

use crate::types::{ConfigFileFormat, FieldType, GameConfig};

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
            ConfigFileFormat::Json => {
                let types = file.variables.iter()
                    .map(|(key, template)| (key.clone(), template_json_type(template, game)))
                    .collect();
                match set_json_values(&current, &values, &types) {
                    Some(text) => text,
                    None => continue,
                }
            }
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

#[derive(Clone, Copy)]
enum JsonScalarType {
    String,
    Number,
    Boolean,
    Inferred,
}

/// Missing keys have no existing JSON type, so use the variable declaration for a single-variable
/// template. Composite templates are text; literal values and undeclared variables retain inference.
fn template_json_type(template: &str, game: &GameConfig) -> JsonScalarType {
    let name = template.strip_prefix("{{")
        .and_then(|s| s.strip_suffix("}}"))
        .filter(|s| !s.contains("{{") && !s.contains("}}"));
    let Some(name) = name else {
        return if template.contains("{{") { JsonScalarType::String } else { JsonScalarType::Inferred };
    };
    match game.variables.iter().find(|v| v.env == name.trim()) {
        Some(variable) => match variable.field_type {
            FieldType::Text | FieldType::Password => JsonScalarType::String,
            FieldType::Number => JsonScalarType::Number,
            FieldType::Select => {
                match variable.options.as_deref().filter(|options| !options.is_empty()) {
                    Some(options) if options.iter().all(|o| matches!(o.value.as_str(), "true" | "false")) => JsonScalarType::Boolean,
                    Some(options) if options.iter().all(|o| json_number(&o.value).is_some()) => JsonScalarType::Number,
                    _ => JsonScalarType::String,
                }
            }
        },
        None => JsonScalarType::Inferred,
    }
}

/// Set dotted-path keys on a JSON object document, preserving existing scalar types. `None` when the
/// file isn't a JSON object or an intermediate path isn't an object.
fn set_json_values(
    text: &str,
    values: &[(String, String)],
    types: &HashMap<String, JsonScalarType>,
) -> Option<String> {
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
        let object = cursor.as_object_mut()?;
        let kind = match object.get(*last) {
            Some(serde_json::Value::String(_)) => JsonScalarType::String,
            Some(serde_json::Value::Number(_)) => JsonScalarType::Number,
            Some(serde_json::Value::Bool(_)) => JsonScalarType::Boolean,
            Some(serde_json::Value::Array(_) | serde_json::Value::Object(_)) => continue,
            _ => types.get(key).copied().unwrap_or(JsonScalarType::Inferred),
        };
        // Invalid scalar input must not turn a numeric/boolean setting into a string or null.
        if let Some(value) = json_scalar(value, kind) {
            object.insert((*last).to_string(), value);
        }
    }
    serde_json::to_string_pretty(&root).ok()
}

fn json_number(value: &str) -> Option<serde_json::Value> {
    value.parse::<i64>().ok().map(serde_json::Value::from)
        .or_else(|| value.parse::<u64>().ok().map(serde_json::Value::from))
        .or_else(|| value.parse::<f64>().ok()
            .and_then(serde_json::Number::from_f64)
            .map(serde_json::Value::Number))
}

fn json_scalar(value: &str, kind: JsonScalarType) -> Option<serde_json::Value> {
    match kind {
        JsonScalarType::String => Some(serde_json::Value::String(value.to_string())),
        JsonScalarType::Number => json_number(value),
        JsonScalarType::Boolean => value.parse::<bool>().ok().map(serde_json::Value::Bool),
        JsonScalarType::Inferred => Some(value.parse::<bool>().ok().map(serde_json::Value::Bool)
            .or_else(|| json_number(value))
            .unwrap_or_else(|| serde_json::Value::String(value.to_string()))),
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
        let out: serde_json::Value = serde_json::from_str(&set_json_values(text, &values, &HashMap::new()).unwrap()).unwrap();
        assert_eq!(out["MaxPlayers"], 32);
        assert_eq!(out["Nested"]["Keep"], true);
        assert_eq!(out["Nested"]["Flag"], false);
        assert_eq!(out["Name"], "Forge");
        assert!(set_json_values("[1,2]", &values, &HashMap::new()).is_none());
    }

    #[test]
    fn json_keeps_existing_strings_numbers_booleans_and_nested_objects() {
        let text = r#"{"Password":"","Name":"Forge","Nested":{"Enabled":true,"Rate":1.5,"Slots":4,"Keep":{"Value":1}}}"#;
        let values = vec![
            ("Password".to_string(), "001234".to_string()),
            ("Name".to_string(), "false".to_string()),
            ("Nested.Enabled".to_string(), "false".to_string()),
            ("Nested.Rate".to_string(), "2.75".to_string()),
            ("Nested.Slots".to_string(), "16".to_string()),
            ("Nested.Keep".to_string(), "accidental scalar".to_string()),
        ];
        let out: serde_json::Value = serde_json::from_str(
            &set_json_values(text, &values, &HashMap::new()).unwrap(),
        ).unwrap();
        assert_eq!(out["Password"], "001234");
        assert_eq!(out["Name"], "false");
        assert_eq!(out["Nested"]["Enabled"], false);
        assert_eq!(out["Nested"]["Rate"], 2.75);
        assert_eq!(out["Nested"]["Slots"], 16);
        assert_eq!(out["Nested"]["Keep"]["Value"], 1);
    }

    #[test]
    fn json_invalid_typed_input_does_not_change_the_existing_type() {
        let text = r#"{"Count":8,"Enabled":true}"#;
        let values = vec![
            ("Count".to_string(), "NaN".to_string()),
            ("Enabled".to_string(), "not a boolean".to_string()),
        ];
        let out: serde_json::Value = serde_json::from_str(
            &set_json_values(text, &values, &HashMap::new()).unwrap(),
        ).unwrap();
        assert_eq!(out, serde_json::from_str::<serde_json::Value>(text).unwrap());
    }

    struct TestDir(std::path::PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("localforge-json-{}-{stamp}-{sequence}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn sotf_config_preserves_defaults_and_text_types_for_existing_and_missing_keys() {
        let game = crate::get_builtin_games().into_iter()
            .find(|g| g.game_type.0 == "sons-of-the-forest").unwrap();
        let dir = TestDir::new();
        let path = dir.0.join(&game.config_files[0].path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let initial = r#"{"MaxPlayers":8,"ServerName":"Dedicated","GameMode":"Normal","Password":"","SaveSlot":1}"#;
        for contents in [initial, "{}"] {
            for overrides in [
                HashMap::new(),
                env(&[("SRV_PW", "001234"), ("SRV_NAME", "123")]),
                env(&[("SRV_PW", "true"), ("SRV_NAME", "false")]),
                env(&[("SRV_PW", "false"), ("SRV_NAME", "true")]),
            ] {
                std::fs::write(&path, contents).unwrap();
                let variables = crate::build_env_vars(&game, 8192, 8766, &overrides);
                assert_eq!(apply_config_files(&dir.0, &game, &variables).unwrap(), vec![game.config_files[0].path.clone()]);
                let actual: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                assert_eq!(actual["Password"], serde_json::Value::String(variables["SRV_PW"].clone()));
                assert_eq!(actual["ServerName"], serde_json::Value::String(variables["SRV_NAME"].clone()));
                assert_eq!(actual["GameMode"], "Normal");
                assert_eq!(actual["MaxPlayers"], 8);
                assert_eq!(actual["SaveSlot"], 1);
                assert!(apply_config_files(&dir.0, &game, &variables).unwrap().is_empty());
            }
        }
    }

    #[test]
    fn json_missing_keys_use_declared_number_and_boolean_select_types() {
        let mut game = crate::get_builtin_games().into_iter()
            .find(|g| g.game_type.0 == "sons-of-the-forest").unwrap();
        game.config_files[0].variables = env(&[
            ("Nested.Enabled", "{{SKIP_TESTS}}"),
            ("Nested.Port", "{{SERVER_PORT}}"),
            ("Nested.Composite", "{{SRV_NAME}}{{SRV_PW}}"),
        ]);
        let dir = TestDir::new();
        let path = dir.0.join(&game.config_files[0].path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{}").unwrap();
        let variables = crate::build_env_vars(&game, 8192, 8766, &env(&[
            ("SRV_NAME", "00"), ("SRV_PW", "1234"), ("SKIP_TESTS", "false"),
        ]));
        apply_config_files(&dir.0, &game, &variables).unwrap();
        let actual: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(actual["Nested"]["Enabled"], false);
        assert_eq!(actual["Nested"]["Port"], 8766);
        assert_eq!(actual["Nested"]["Composite"], "001234");
    }
}
