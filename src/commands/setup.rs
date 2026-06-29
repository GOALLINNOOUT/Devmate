use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::{
    cli::SetupArgs,
    commands::{analyze, doctor},
    models::{ToolImportance, ToolStatus},
    output,
};

pub fn run(args: SetupArgs) -> Result<()> {
    let report = check(&args.path)?;
    if args.json {
        output::print_json(&report)?;
    } else {
        render(&report);
    }
    Ok(())
}

pub fn check(root: &Path) -> Result<SetupReport> {
    let project_types = analyze::detect_project_types(root)?;
    let doctor = doctor::check(root);
    let required_missing = missing_tools(&doctor.tools, ToolImportance::Required);
    let recommended_missing = missing_tools(&doctor.tools, ToolImportance::Recommended);
    let optional_missing = missing_tools(&doctor.tools, ToolImportance::Optional);
    Ok(SetupReport {
        root: root.to_path_buf(),
        project_types,
        required_missing,
        recommended_missing,
        optional_missing,
        next_commands: next_commands(),
        install_help: install_help(),
    })
}

#[derive(Debug, Serialize)]
pub struct SetupReport {
    pub root: PathBuf,
    pub project_types: Vec<String>,
    pub required_missing: Vec<String>,
    pub recommended_missing: Vec<String>,
    pub optional_missing: Vec<String>,
    pub next_commands: Vec<String>,
    pub install_help: Vec<String>,
}

fn missing_tools(tools: &[ToolStatus], importance: ToolImportance) -> Vec<String> {
    tools
        .iter()
        .filter(|tool| tool.importance == importance && !tool.installed)
        .map(|tool| tool.name.clone())
        .collect()
}

fn next_commands() -> Vec<String> {
    [
        "devmate doctor",
        "devmate analyze",
        "devmate system",
        "devmate files stats",
    ]
    .iter()
    .map(|command| (*command).to_string())
    .collect()
}

fn install_help() -> Vec<String> {
    [
        "winget upgrade ADELA.Devmate",
        "cargo install devmate --force",
        "cargo binstall devmate --force",
    ]
    .iter()
    .map(|command| (*command).to_string())
    .collect()
}

fn render(report: &SetupReport) {
    anstream::println!("DevMate setup");
    anstream::println!("Project: {}", report.root.display());
    anstream::println!("Detected: {}", report.project_types.join(", "));
    anstream::println!();

    let rows = vec![
        vec![
            "Required missing".to_string(),
            display_list(&report.required_missing),
        ],
        vec![
            "Recommended missing".to_string(),
            display_list(&report.recommended_missing),
        ],
        vec![
            "Optional missing".to_string(),
            display_list(&report.optional_missing),
        ],
    ];
    anstream::println!("{}", output::table(&["Check", "Result"], rows));

    anstream::println!("Try these first:");
    for command in &report.next_commands {
        anstream::println!("  {command}");
    }

    anstream::println!();
    anstream::println!("Update DevMate:");
    for command in &report.install_help {
        anstream::println!("  {command}");
    }
}

fn display_list(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn setup_detects_project_and_next_commands() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname='sample'\n").unwrap();

        let report = check(dir.path()).unwrap();

        assert!(report.project_types.contains(&"Rust".to_string()));
        assert!(report.next_commands.contains(&"devmate doctor".to_string()));
    }
}
