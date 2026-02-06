use crate::error::{ApsError, Result};
use serde_yaml::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn validate_cursor_hooks(hooks_dir: &Path, strict: bool) -> Result<Vec<String>> {
    validate_hooks(hooks_dir, strict)
}

fn validate_hooks(hooks_dir: &Path, strict: bool) -> Result<Vec<String>> {
    let mut warnings = Vec::new();

    let hooks_root = hooks_root_dir(hooks_dir);
    let config_path = hooks_root.join("hooks.json");
    if !config_path.exists() {
        warn_or_error(
            &mut warnings,
            strict,
            ApsError::MissingHooksConfig {
                path: config_path.clone(),
            },
        )?;
        return Ok(warnings);
    }

    let config_value = match read_hooks_config(&config_path) {
        Ok(value) => value,
        Err(err) => {
            warn_or_error(&mut warnings, strict, err)?;
            return Ok(warnings);
        }
    };

    let hooks_section = match get_hooks_section(&config_value) {
        Some(section) => section,
        None => {
            warn_or_error(
                &mut warnings,
                strict,
                ApsError::MissingHooksSection {
                    path: config_path.clone(),
                },
            )?;
            return Ok(warnings);
        }
    };

    let commands = collect_hook_commands(hooks_section);
    let referenced_scripts = collect_hook_script_paths(&commands);

    for rel_path in referenced_scripts {
        let script_path = hooks_root.join(rel_path);
        if !script_path.is_file() {
            warn_or_error(
                &mut warnings,
                strict,
                ApsError::HookScriptNotFound { path: script_path },
            )?;
        }
    }

    Ok(warnings)
}

fn hooks_root_dir(hooks_dir: &Path) -> PathBuf {
    match hooks_dir.file_name().and_then(|name| name.to_str()) {
        Some("hooks") | Some("scripts") => hooks_dir.parent().unwrap_or(hooks_dir).to_path_buf(),
        _ => hooks_dir.to_path_buf(),
    }
}

fn read_hooks_config(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ApsError::io(e, "Failed to read hooks config"))?;

    serde_yaml::from_str(&content).map_err(|e| ApsError::InvalidHooksConfig {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn get_hooks_section(config: &Value) -> Option<&Value> {
    let map = match config {
        Value::Mapping(map) => map,
        _ => return None,
    };

    map.get(Value::String("hooks".to_string()))
}

fn collect_hook_commands(section: &Value) -> Vec<String> {
    let mut commands = Vec::new();
    collect_command_values(section, &mut commands);
    commands
}

fn collect_command_values(value: &Value, commands: &mut Vec<String>) {
    match value {
        Value::Mapping(map) => {
            for (key, val) in map {
                if matches!(key, Value::String(k) if k == "command") {
                    if let Value::String(command) = val {
                        commands.push(command.clone());
                        continue;
                    }
                }
                collect_command_values(val, commands);
            }
        }
        Value::Sequence(seq) => {
            for val in seq {
                collect_command_values(val, commands);
            }
        }
        _ => {}
    }
}

fn collect_hook_script_paths(commands: &[String]) -> HashSet<PathBuf> {
    let mut scripts = HashSet::new();

    for command in commands {
        for token in command.split_whitespace() {
            if let Some(rel_path) = extract_relative_path(token) {
                scripts.insert(PathBuf::from(rel_path));
            }
        }
    }

    scripts
}

fn extract_relative_path(token: &str) -> Option<String> {
    let token = trim_token(token);
    if token.is_empty() {
        return None;
    }

    let markers = [".cursor/", ".cursor\\"];

    for marker in markers {
        if let Some(position) = token.find(marker) {
            let mut rel = &token[position + marker.len()..];
            rel = trim_token(rel);
            if !rel.is_empty() {
                return Some(rel.to_string());
            }
        }
    }

    let trimmed = token
        .strip_prefix("./")
        .or_else(|| token.strip_prefix(".\\"))
        .unwrap_or(token);
    let rel_prefixes = ["hooks/", "scripts/", "hooks\\", "scripts\\"];

    for prefix in rel_prefixes {
        if trimmed.starts_with(prefix) {
            let rel = trim_token(trimmed);
            if !rel.is_empty() {
                return Some(rel.to_string());
            }
        }
    }

    None
}

fn trim_token(token: &str) -> &str {
    token.trim_matches(|c: char| matches!(c, '"' | '\'' | ';' | ')' | '(' | ','))
}

fn warn_or_error(warnings: &mut Vec<String>, strict: bool, error: ApsError) -> Result<()> {
    if strict {
        return Err(error);
    }

    warnings.push(error.to_string());
    Ok(())
}
