use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Result;

use crate::{
    cli::DoctorArgs,
    models::{DoctorReport, ToolImportance, ToolStatus},
    output,
};

pub fn run(args: DoctorArgs) -> Result<()> {
    let report = check(&args.path);
    if args.json {
        output::print_json(&report)?;
    } else {
        render(&report);
    }
    Ok(())
}

pub fn check(root: &Path) -> DoctorReport {
    let specs = tool_specs(root);

    DoctorReport {
        tools: specs
            .iter()
            .map(|spec| inspect_tool(spec.name, spec.importance, spec.executable, spec.args))
            .collect(),
    }
}

struct ToolSpec {
    name: &'static str,
    importance: ToolImportance,
    executable: &'static str,
    args: &'static [&'static str],
}

impl ToolSpec {
    const fn new(
        name: &'static str,
        importance: ToolImportance,
        executable: &'static str,
        args: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            importance,
            executable,
            args,
        }
    }
}

fn tool_specs(root: &Path) -> Vec<ToolSpec> {
    let mut specs = BTreeMap::<&'static str, ToolSpec>::new();
    add_baseline_specs(&mut specs);
    add_project_specs(root, &mut specs);
    specs.into_values().collect()
}

fn add_tool(specs: &mut BTreeMap<&'static str, ToolSpec>, spec: ToolSpec) {
    specs
        .entry(spec.name)
        .and_modify(|existing| {
            if importance_rank(spec.importance) < importance_rank(existing.importance) {
                existing.importance = spec.importance;
            }
        })
        .or_insert(spec);
}

fn add_baseline_specs(specs: &mut BTreeMap<&'static str, ToolSpec>) {
    use ToolImportance::{Optional, Recommended};

    for spec in [
        ToolSpec::new("Git", Recommended, "git", &["--version"]),
        ToolSpec::new("VS Code", Optional, "code", &["--version"]),
        ToolSpec::new("GitHub CLI", Optional, "gh", &["--version"]),
        ToolSpec::new("Cursor", Optional, "cursor", &["--version"]),
        ToolSpec::new("Claude Code", Optional, "claude", &["--version"]),
    ] {
        add_tool(specs, spec);
    }
}

fn add_project_specs(root: &Path, specs: &mut BTreeMap<&'static str, ToolSpec>) {
    use ToolImportance::{Optional, Recommended, Required};

    if root.join("Cargo.toml").exists() {
        add_tool(
            specs,
            ToolSpec::new("Rust", Required, "rustc", &["--version"]),
        );
        add_tool(
            specs,
            ToolSpec::new("Cargo", Required, "cargo", &["--version"]),
        );
        add_tool(
            specs,
            ToolSpec::new("rustfmt", Recommended, "rustfmt", &["--version"]),
        );
        add_tool(
            specs,
            ToolSpec::new("Clippy", Recommended, "cargo", &["clippy", "--version"]),
        );
    }

    if root.join("package.json").exists() {
        add_tool(
            specs,
            ToolSpec::new("Node", Required, "node", &["--version"]),
        );
        add_tool(
            specs,
            ToolSpec::new("npm", Recommended, "npm", &["--version"]),
        );
        if root.join("pnpm-lock.yaml").exists() {
            add_tool(
                specs,
                ToolSpec::new("pnpm", Required, "pnpm", &["--version"]),
            );
        } else {
            add_tool(
                specs,
                ToolSpec::new("pnpm", Optional, "pnpm", &["--version"]),
            );
        }
        if root.join("yarn.lock").exists() {
            add_tool(
                specs,
                ToolSpec::new("Yarn", Required, "yarn", &["--version"]),
            );
        }
        if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
            add_tool(specs, ToolSpec::new("Bun", Required, "bun", &["--version"]));
        } else {
            add_tool(specs, ToolSpec::new("Bun", Optional, "bun", &["--version"]));
        }
    }

    if root.join("pyproject.toml").exists()
        || root.join("requirements.txt").exists()
        || root.join("setup.py").exists()
    {
        add_tool(
            specs,
            ToolSpec::new("Python", Required, "python", &["--version"]),
        );
        add_tool(
            specs,
            ToolSpec::new("pip", Recommended, "pip", &["--version"]),
        );
    }

    if root.join("go.mod").exists() {
        add_tool(specs, ToolSpec::new("Go", Required, "go", &["version"]));
    }

    if has_docker_files(root) {
        add_tool(
            specs,
            ToolSpec::new("Docker", Required, "docker", &["--version"]),
        );
        add_tool(
            specs,
            ToolSpec::new(
                "Docker Compose",
                Recommended,
                "docker",
                &["compose", "version"],
            ),
        );
    }

    let manifest_text = combined_manifest_text(root);
    if contains_any(&manifest_text, &["mongodb", "mongoose"]) {
        add_tool(
            specs,
            ToolSpec::new("MongoDB", Recommended, "mongod", &["--version"]),
        );
    }
    if contains_any(&manifest_text, &["postgres", "postgresql", "pg"]) {
        add_tool(
            specs,
            ToolSpec::new("PostgreSQL", Recommended, "psql", &["--version"]),
        );
    }
    if contains_any(&manifest_text, &["redis"]) {
        add_tool(
            specs,
            ToolSpec::new("Redis", Recommended, "redis-server", &["--version"]),
        );
    }
}

fn has_docker_files(root: &Path) -> bool {
    [
        "Dockerfile",
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
    ]
    .iter()
    .any(|name| root.join(name).exists())
}

fn combined_manifest_text(root: &Path) -> String {
    [
        "package.json",
        "Cargo.toml",
        "requirements.txt",
        "pyproject.toml",
        "go.mod",
    ]
    .iter()
    .filter_map(|name| fs::read_to_string(root.join(name)).ok())
    .collect::<Vec<_>>()
    .join("\n")
    .to_ascii_lowercase()
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn importance_rank(importance: ToolImportance) -> u8 {
    match importance {
        ToolImportance::Required => 0,
        ToolImportance::Recommended => 1,
        ToolImportance::Optional => 2,
    }
}

fn inspect_tool(
    name: &str,
    importance: ToolImportance,
    executable: &str,
    args: &[&str],
) -> ToolStatus {
    let path = which::which(executable).ok();
    let version = path.as_ref().and_then(|resolved| version(resolved, args));
    ToolStatus {
        name: name.to_string(),
        importance,
        installed: path.is_some(),
        version,
        path,
    }
}

fn version(executable: &PathBuf, args: &[&str]) -> Option<String> {
    let output = Command::new(executable).args(args).output().ok()?;
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).to_string()
    } else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    text.lines().next().map(str::trim).map(str::to_string)
}

fn render(report: &DoctorReport) {
    let rows = report
        .tools
        .iter()
        .map(|tool| {
            vec![
                tool.name.clone(),
                importance_label(tool.importance).to_string(),
                output::status(tool.installed).to_string(),
                tool.version.clone().unwrap_or_else(|| "-".to_string()),
                tool.path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();
    anstream::println!(
        "{}",
        output::table(&["Tool", "Importance", "Status", "Version", "Path"], rows)
    );
}

fn importance_label(importance: ToolImportance) -> &'static str {
    match importance {
        ToolImportance::Required => "required",
        ToolImportance::Recommended => "recommended",
        ToolImportance::Optional => "optional",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn tool(report: &DoctorReport, name: &str) -> ToolImportance {
        report
            .tools
            .iter()
            .find(|tool| tool.name == name)
            .map(|tool| tool.importance)
            .unwrap()
    }

    #[test]
    fn doctor_marks_rust_tools_as_project_required() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n",
        )
        .unwrap();

        let report = check(dir.path());

        assert_eq!(tool(&report, "Rust"), ToolImportance::Required);
        assert_eq!(tool(&report, "Cargo"), ToolImportance::Required);
        assert_eq!(tool(&report, "Clippy"), ToolImportance::Recommended);
    }

    #[test]
    fn doctor_detects_node_lockfile_package_manager() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();

        let report = check(dir.path());

        assert_eq!(tool(&report, "Node"), ToolImportance::Required);
        assert_eq!(tool(&report, "pnpm"), ToolImportance::Required);
    }

    #[test]
    fn doctor_detects_python_go_and_docker() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "redis==1\n").unwrap();
        fs::write(dir.path().join("go.mod"), "module sample\n").unwrap();
        fs::write(dir.path().join("Dockerfile"), "FROM scratch\n").unwrap();

        let report = check(dir.path());

        assert_eq!(tool(&report, "Python"), ToolImportance::Required);
        assert_eq!(tool(&report, "Go"), ToolImportance::Required);
        assert_eq!(tool(&report, "Docker"), ToolImportance::Required);
        assert_eq!(tool(&report, "Redis"), ToolImportance::Recommended);
    }
}
