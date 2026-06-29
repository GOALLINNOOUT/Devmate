use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use regex::Regex;

use crate::{
    cli::{EnvArgs, EnvCommand},
    fswalk,
    models::{EnvReference, EnvReport},
    output,
};

pub fn run(args: EnvArgs) -> Result<()> {
    let command = args.command.unwrap_or(EnvCommand::Inspect {
        path: PathBuf::from("."),
        file: PathBuf::from(".env"),
        example: None,
        json: false,
    });
    match command {
        EnvCommand::Inspect {
            path,
            file,
            example,
            json,
        } => {
            let env_file = resolve_env_file(&path, &file);
            let example_file = example.map(|value| resolve_env_file(&path, &value));
            let report = inspect(&path, &env_file, example_file.as_deref())?;
            if json {
                output::print_json(&report)?;
            } else {
                render(&report);
            }
        }
    }
    Ok(())
}

pub fn inspect(project_root: &Path, file: &Path, example: Option<&Path>) -> Result<EnvReport> {
    let env = if file.exists() {
        parse_env(file)?
    } else {
        ParsedEnv::default()
    };
    let references = scan_references(project_root)?;
    let referenced_keys = references.keys().cloned().collect::<BTreeSet<_>>();
    let example_vars = if let Some(example) = example.filter(|path| path.exists()) {
        Some(parse_env(example)?)
    } else {
        None
    };

    let env_keys = env.values.keys().cloned().collect::<BTreeSet<_>>();
    let example_keys = example_vars
        .as_ref()
        .map(|parsed| parsed.values.keys().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();

    let missing_from_env = referenced_keys
        .difference(&env_keys)
        .cloned()
        .collect::<Vec<String>>();
    let unused_in_env = env_keys
        .difference(&referenced_keys)
        .cloned()
        .collect::<Vec<String>>();
    let missing_from_example = example_keys
        .difference(&env_keys)
        .cloned()
        .collect::<Vec<String>>();
    let extra_in_env = env_keys
        .difference(&example_keys)
        .filter(|_| example_vars.is_some())
        .cloned()
        .collect::<Vec<String>>();
    let referenced_variables = references
        .into_iter()
        .map(|(name, files)| EnvReference { name, files })
        .collect();

    Ok(EnvReport {
        file: file.to_path_buf(),
        example: example.filter(|path| path.exists()).map(Path::to_path_buf),
        variables: env.values.len(),
        duplicates: env.duplicates,
        empty: env.empty,
        malformed: env.malformed,
        referenced_variables,
        missing_from_env,
        unused_in_env,
        missing_from_example,
        extra_in_env,
    })
}

#[derive(Debug, Default)]
struct ParsedEnv {
    values: BTreeMap<String, String>,
    duplicates: Vec<String>,
    empty: Vec<String>,
    malformed: Vec<String>,
}

fn parse_env(path: &Path) -> Result<ParsedEnv> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut parsed = ParsedEnv::default();
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            parsed.malformed.push(format!("{}:{}", index + 1, raw_line));
            continue;
        };
        let key = raw_key
            .trim()
            .strip_prefix("export ")
            .unwrap_or(raw_key.trim());
        if !valid_key(key) {
            parsed.malformed.push(format!("{}:{}", index + 1, raw_line));
            continue;
        }
        if parsed.values.contains_key(key) {
            parsed.duplicates.push(key.to_string());
        }
        let value = raw_value.trim().trim_matches('"').trim_matches('\'');
        if value.is_empty() {
            parsed.empty.push(key.to_string());
        }
        parsed.values.insert(key.to_string(), value.to_string());
    }
    parsed.duplicates.sort();
    parsed.duplicates.dedup();
    parsed.empty.sort();
    parsed.empty.dedup();
    Ok(parsed)
}

fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|char| char == '_' || char.is_ascii_alphanumeric())
}

fn resolve_env_file(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn scan_references(root: &Path) -> Result<BTreeMap<String, Vec<PathBuf>>> {
    fswalk::ensure_dir(root)?;
    let patterns = [
        r#"process\.env\.([A-Za-z_][A-Za-z0-9_]*)"#,
        r#"import\.meta\.env\.([A-Za-z_][A-Za-z0-9_]*)"#,
        r#"Deno\.env\.get\(["']([A-Za-z_][A-Za-z0-9_]*)["']\)"#,
        r#"std::env::var\(["']([A-Za-z_][A-Za-z0-9_]*)["']\)"#,
        r#"env!\(["']([A-Za-z_][A-Za-z0-9_]*)["']\)"#,
        r#"os\.environ\[['"]([A-Za-z_][A-Za-z0-9_]*)['"]\]"#,
        r#"os\.getenv\(["']([A-Za-z_][A-Za-z0-9_]*)["']\)"#,
    ];
    let expressions = patterns
        .iter()
        .map(|pattern| Regex::new(pattern))
        .collect::<Result<Vec<_>, _>>()?;
    let mut references = BTreeMap::<String, BTreeSet<PathBuf>>::new();
    for path in fswalk::walk(root)?
        .into_iter()
        .filter(|path| path.is_file())
    {
        if crate::commands::files::is_binary_like(&path) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for expression in &expressions {
            for captures in expression.captures_iter(&text) {
                if let Some(name) = captures.get(1) {
                    references
                        .entry(name.as_str().to_string())
                        .or_default()
                        .insert(fswalk::relative(root, &path));
                }
            }
        }
    }
    Ok(references
        .into_iter()
        .map(|(name, files)| (name, files.into_iter().collect()))
        .collect())
}

fn render(report: &EnvReport) {
    anstream::println!("Env file: {}", report.file.display());
    if let Some(example) = &report.example {
        anstream::println!("Example: {}", example.display());
    }
    anstream::println!("Variables: {}", report.variables);
    let rows = vec![
        vec!["Duplicates".to_string(), report.duplicates.join(", ")],
        vec!["Empty".to_string(), report.empty.join(", ")],
        vec!["Malformed".to_string(), report.malformed.join(", ")],
        vec![
            "Referenced but missing".to_string(),
            report.missing_from_env.join(", "),
        ],
        vec![
            "Defined but unused".to_string(),
            report.unused_in_env.join(", "),
        ],
        vec![
            "Missing from example".to_string(),
            report.missing_from_example.join(", "),
        ],
        vec!["Extra in env".to_string(), report.extra_in_env.join(", ")],
    ];
    anstream::println!("{}", output::table(&["Check", "Values"], rows));
    if !report.referenced_variables.is_empty() {
        let rows = report
            .referenced_variables
            .iter()
            .map(|reference| {
                vec![
                    reference.name.clone(),
                    reference
                        .files
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                ]
            })
            .collect();
        anstream::println!("{}", output::table(&["Referenced variable", "Files"], rows));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_env_keys() {
        assert!(valid_key("DATABASE_URL"));
        assert!(valid_key("_TOKEN1"));
        assert!(!valid_key("1BAD"));
        assert!(!valid_key("BAD-NAME"));
    }
}
