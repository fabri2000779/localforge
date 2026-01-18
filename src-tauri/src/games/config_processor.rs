// Config file processor - handles reading/writing config files with variable substitution
// TODO: Integrate with server startup to process config files
#![allow(dead_code)]

use crate::games::{ConfigFile, ConfigFileFormat};
use std::collections::HashMap;
use std::path::Path;

/// Apply variable substitutions to a config file
/// Returns Ok(true) if file was modified, Ok(false) if file doesn't exist
pub fn apply_config_variables(
    base_path: &Path,
    config_file: &ConfigFile,
    variables: &HashMap<String, String>,
) -> Result<bool, String> {
    let file_path = base_path.join(&config_file.path);
    
    if !file_path.exists() {
        tracing::debug!("Config file doesn't exist yet: {:?}", file_path);
        return Ok(false);
    }
    
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read config file {:?}: {}", file_path, e))?;
    
    let new_content = match config_file.format {
        ConfigFileFormat::Properties => {
            apply_properties_variables(&content, &config_file.variables, variables)?
        }
        ConfigFileFormat::Ini => {
            apply_ini_variables(&content, &config_file.variables, variables)?
        }
        ConfigFileFormat::Json => {
            apply_json_variables(&content, &config_file.variables, variables)?
        }
        ConfigFileFormat::Yaml => {
            apply_yaml_variables(&content, &config_file.variables, variables)?
        }
    };
    
    if new_content != content {
        std::fs::write(&file_path, &new_content)
            .map_err(|e| format!("Failed to write config file {:?}: {}", file_path, e))?;
        tracing::info!("Updated config file: {:?}", file_path);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Apply variables to a Properties file (key=value format, no sections)
fn apply_properties_variables(
    content: &str,
    mappings: &HashMap<String, String>,
    variables: &HashMap<String, String>,
) -> Result<String, String> {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    
    for (config_key, var_template) in mappings {
        // Extract variable name from {{VAR_NAME}}
        let var_name = var_template
            .trim_start_matches("{{")
            .trim_end_matches("}}")
            .to_string();
        
        if let Some(value) = variables.get(&var_name) {
            // Find and replace the line with this key
            for line in &mut lines {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.is_empty() {
                    continue;
                }
                
                if let Some(eq_pos) = trimmed.find('=') {
                    let key = trimmed[..eq_pos].trim();
                    if key == config_key {
                        *line = format!("{}={}", config_key, value);
                        break;
                    }
                }
            }
        }
    }
    
    Ok(lines.join("\n"))
}

/// Apply variables to an INI file (with [section] headers)
fn apply_ini_variables(
    content: &str,
    mappings: &HashMap<String, String>,
    variables: &HashMap<String, String>,
) -> Result<String, String> {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    
    for (config_key, var_template) in mappings {
        // Extract variable name from {{VAR_NAME}}
        let var_name = var_template
            .trim_start_matches("{{")
            .trim_end_matches("}}")
            .to_string();
        
        if let Some(value) = variables.get(&var_name) {
            // config_key can be either "key" or "section/key"
            let (target_section, target_key) = if config_key.contains('/') {
                let parts: Vec<&str> = config_key.splitn(2, '/').collect();
                (Some(parts[0]), parts[1])
            } else {
                (None, config_key.as_str())
            };
            
            let mut current_section: Option<String> = None;
            let mut found = false;
            
            for line in &mut lines {
                let trimmed = line.trim();
                
                // Track current section
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    current_section = Some(trimmed[1..trimmed.len()-1].to_string());
                    continue;
                }
                
                // Skip comments and empty lines
                if trimmed.starts_with('#') || trimmed.starts_with(';') || trimmed.is_empty() {
                    continue;
                }
                
                // Check if this line matches our key (and section if specified)
                if let Some(eq_pos) = trimmed.find('=') {
                    let key = trimmed[..eq_pos].trim();
                    
                    let section_matches = match (&target_section, &current_section) {
                        (Some(ts), Some(cs)) => ts == cs,
                        (None, _) => true, // No section specified, match any
                        _ => false,
                    };
                    
                    if section_matches && key == target_key {
                        *line = format!("{}={}", target_key, value);
                        found = true;
                        break;
                    }
                }
            }
            
            if !found {
                tracing::debug!("INI key not found: {} (will be added on first run)", config_key);
            }
        }
    }
    
    Ok(lines.join("\n"))
}

/// Apply variables to a JSON file
fn apply_json_variables(
    content: &str,
    mappings: &HashMap<String, String>,
    variables: &HashMap<String, String>,
) -> Result<String, String> {
    let mut json: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| format!("Invalid JSON: {}", e))?;
    
    for (config_key, var_template) in mappings {
        let var_name = var_template
            .trim_start_matches("{{")
            .trim_end_matches("}}")
            .to_string();
        
        if let Some(value) = variables.get(&var_name) {
            // Support nested keys with dot notation: "server.maxPlayers"
            let keys: Vec<&str> = config_key.split('.').collect();
            set_json_value(&mut json, &keys, value);
        }
    }
    
    serde_json::to_string_pretty(&json)
        .map_err(|e| format!("Failed to serialize JSON: {}", e))
}

fn set_json_value(json: &mut serde_json::Value, keys: &[&str], value: &str) {
    if keys.is_empty() {
        return;
    }
    
    if keys.len() == 1 {
        if let serde_json::Value::Object(map) = json {
            // Try to preserve the original type
            if let Some(existing) = map.get(keys[0]) {
                let new_value = match existing {
                    serde_json::Value::Number(_) => {
                        if let Ok(n) = value.parse::<i64>() {
                            serde_json::Value::Number(n.into())
                        } else if let Ok(n) = value.parse::<f64>() {
                            serde_json::Number::from_f64(n)
                                .map(serde_json::Value::Number)
                                .unwrap_or_else(|| serde_json::Value::String(value.to_string()))
                        } else {
                            serde_json::Value::String(value.to_string())
                        }
                    }
                    serde_json::Value::Bool(_) => {
                        serde_json::Value::Bool(value == "true" || value == "1")
                    }
                    _ => serde_json::Value::String(value.to_string()),
                };
                map.insert(keys[0].to_string(), new_value);
            } else {
                map.insert(keys[0].to_string(), serde_json::Value::String(value.to_string()));
            }
        }
    } else {
        if let serde_json::Value::Object(map) = json {
            if let Some(nested) = map.get_mut(keys[0]) {
                set_json_value(nested, &keys[1..], value);
            }
        }
    }
}

/// Apply variables to a YAML file
fn apply_yaml_variables(
    content: &str,
    mappings: &HashMap<String, String>,
    variables: &HashMap<String, String>,
) -> Result<String, String> {
    // For YAML, we do simple line-based replacement to preserve formatting
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    
    for (config_key, var_template) in mappings {
        let var_name = var_template
            .trim_start_matches("{{")
            .trim_end_matches("}}")
            .to_string();
        
        if let Some(value) = variables.get(&var_name) {
            // Simple key: value matching
            for line in &mut lines {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.is_empty() {
                    continue;
                }
                
                if let Some(colon_pos) = trimmed.find(':') {
                    let key = trimmed[..colon_pos].trim();
                    if key == config_key {
                        // Preserve indentation
                        let indent = line.len() - line.trim_start().len();
                        let indent_str: String = line.chars().take(indent).collect();
                        *line = format!("{}{}: {}", indent_str, config_key, value);
                        break;
                    }
                }
            }
        }
    }
    
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_properties_replacement() {
        let content = "# Comment\nmax-players=20\ndifficulty=normal\n";
        let mut mappings = HashMap::new();
        mappings.insert("max-players".to_string(), "{{MAX_PLAYERS}}".to_string());
        
        let mut variables = HashMap::new();
        variables.insert("MAX_PLAYERS".to_string(), "50".to_string());
        
        let result = apply_properties_variables(content, &mappings, &variables).unwrap();
        assert!(result.contains("max-players=50"));
    }
    
    #[test]
    fn test_ini_replacement() {
        let content = "[Server]\nMaxPlayers=20\nViewDistance=12\n";
        let mut mappings = HashMap::new();
        mappings.insert("Server/MaxPlayers".to_string(), "{{HT_MAXPLAYERS}}".to_string());
        
        let mut variables = HashMap::new();
        variables.insert("HT_MAXPLAYERS".to_string(), "100".to_string());
        
        let result = apply_ini_variables(content, &mappings, &variables).unwrap();
        assert!(result.contains("MaxPlayers=100"));
    }
    
    #[test]
    fn test_json_replacement() {
        let content = r#"{"MaxPlayers": 20, "ViewDistance": 12}"#;
        let mut mappings = HashMap::new();
        mappings.insert("MaxPlayers".to_string(), "{{HT_MAXPLAYERS}}".to_string());
        
        let mut variables = HashMap::new();
        variables.insert("HT_MAXPLAYERS".to_string(), "100".to_string());
        
        let result = apply_json_variables(content, &mappings, &variables).unwrap();
        assert!(result.contains("100"));
    }
}
