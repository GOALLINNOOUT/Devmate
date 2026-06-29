use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::{
    cli::{JsonArgs, JsonCommand},
    errors::DevMateError,
    fswalk, output,
};

pub fn run(args: JsonArgs) -> Result<()> {
    match args.command {
        JsonCommand::Validate { file } => {
            parse_json_file(&file)?;
            anstream::println!("valid JSON: {}", file.display());
        }
        JsonCommand::Format { file, output: out } => {
            let value = parse_json_file(&file)?;
            let formatted = serde_json::to_string_pretty(&value)?;
            write_or_print(out.as_deref(), &formatted)?;
        }
        JsonCommand::Minify { file, output: out } => {
            let value = parse_json_file(&file)?;
            let minified = serde_json::to_string(&value)?;
            write_or_print(out.as_deref(), &minified)?;
        }
        JsonCommand::Diff { left, right } => {
            let left_value = parse_json_file(&left)?;
            let right_value = parse_json_file(&right)?;
            let changes = diff_values("", &left_value, &right_value);
            if changes.is_empty() {
                anstream::println!("JSON files are equivalent");
            } else {
                let rows = changes
                    .into_iter()
                    .map(|change| vec![change.path, change.left, change.right])
                    .collect();
                anstream::println!("{}", output::table(&["Path", "Left", "Right"], rows));
            }
        }
    }
    Ok(())
}

pub fn parse_json_file(path: &Path) -> Result<Value> {
    fswalk::ensure_file(path)?;
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read JSON file {}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| {
        DevMateError::JsonParse {
            path: path.to_path_buf(),
            line: error.line(),
            column: error.column(),
            message: error.to_string(),
        }
        .into()
    })
}

fn write_or_print(output_path: Option<&Path>, text: &str) -> Result<()> {
    if let Some(path) = output_path {
        fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))?;
    } else {
        anstream::println!("{text}");
    }
    Ok(())
}

#[derive(Debug)]
struct JsonChange {
    path: String,
    left: String,
    right: String,
}

fn diff_values(path: &str, left: &Value, right: &Value) -> Vec<JsonChange> {
    if left == right {
        return Vec::new();
    }

    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            let mut changes = Vec::new();
            let mut keys = left_map.keys().chain(right_map.keys()).collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child = if path.is_empty() {
                    key.to_string()
                } else {
                    format!("{path}.{key}")
                };
                match (left_map.get(key), right_map.get(key)) {
                    (Some(left_value), Some(right_value)) => {
                        changes.extend(diff_values(&child, left_value, right_value));
                    }
                    (Some(value), None) => changes.push(JsonChange {
                        path: child,
                        left: summarize(value),
                        right: "[missing]".to_string(),
                    }),
                    (None, Some(value)) => changes.push(JsonChange {
                        path: child,
                        left: "[missing]".to_string(),
                        right: summarize(value),
                    }),
                    (None, None) => {}
                }
            }
            changes
        }
        (Value::Array(left_items), Value::Array(right_items)) => {
            let mut changes = Vec::new();
            let max_len = left_items.len().max(right_items.len());
            for index in 0..max_len {
                let child = format!("{path}[{index}]");
                match (left_items.get(index), right_items.get(index)) {
                    (Some(left_value), Some(right_value)) => {
                        changes.extend(diff_values(&child, left_value, right_value));
                    }
                    (Some(value), None) => changes.push(JsonChange {
                        path: child,
                        left: summarize(value),
                        right: "[missing]".to_string(),
                    }),
                    (None, Some(value)) => changes.push(JsonChange {
                        path: child,
                        left: "[missing]".to_string(),
                        right: summarize(value),
                    }),
                    (None, None) => {}
                }
            }
            changes
        }
        _ => vec![JsonChange {
            path: if path.is_empty() {
                "$".to_string()
            } else {
                path.to_string()
            },
            left: summarize(left),
            right: summarize(right),
        }],
    }
}

fn summarize(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_reports_nested_changes() {
        let left = serde_json::json!({"a": 1, "b": {"c": true}});
        let right = serde_json::json!({"a": 2, "b": {"d": true}});
        let changes = diff_values("", &left, &right);
        let paths = changes
            .into_iter()
            .map(|change| change.path)
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["a", "b.c", "b.d"]);
    }
}
