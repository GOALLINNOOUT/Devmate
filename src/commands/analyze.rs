use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use git2::{Repository, Sort};
use rayon::prelude::*;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    cli::AnalyzeArgs,
    commands::{files, git},
    errors::DevMateError,
    fswalk,
    models::{
        AnalyzeArchitecture, AnalyzeComplexity, AnalyzeConfigUsed, AnalyzeFileReport, AnalyzeGit,
        AnalyzeHotspot, AnalyzeImportEdge, AnalyzeIssue, AnalyzeLanguage, AnalyzeRecommendation,
        AnalyzeReport, AnalyzeSymbol, Dependency, FileEntry, HealthScoreItem, LineStats,
    },
    output,
};

const ASSET_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "pdf"];
const BUILTIN_IGNORES: [&str; 7] = [
    ".next",
    "dist",
    "build",
    "vendor",
    "coverage",
    "node_modules",
    "target",
];
const DISPLAY_LIMIT: usize = 10;

pub fn run(args: AnalyzeArgs) -> Result<()> {
    let config = AnalyzeConfig::load(args.config.as_deref(), args.large_file_bytes)?;
    let target = analyze_target(&args.path, &config)?;
    if args.json {
        match &target {
            TargetReport::Project(report) => output::print_json(report.as_ref())?,
            TargetReport::File(report) => output::print_json(report.as_ref())?,
        }
    } else {
        match &target {
            TargetReport::Project(report) => render_project(report.as_ref(), args.details),
            TargetReport::File(report) => render_file(report.as_ref()),
        }
    }
    Ok(())
}

enum TargetReport {
    Project(Box<AnalyzeReport>),
    File(Box<AnalyzeFileReport>),
}

pub fn analyze(root: &Path, large_file_bytes: u64) -> Result<AnalyzeReport> {
    let config = AnalyzeConfig::default().with_large_file_bytes(large_file_bytes);
    analyze_project(root, &config)
}

fn analyze_target(target: &Path, config: &AnalyzeConfig) -> Result<TargetReport> {
    if !target.exists() {
        return Err(DevMateError::MissingPath(target.to_path_buf()).into());
    }
    if target.is_file() {
        Ok(TargetReport::File(Box::new(analyze_file_target(
            target, config,
        )?)))
    } else if target.is_dir() {
        Ok(TargetReport::Project(Box::new(analyze_project(
            target, config,
        )?)))
    } else {
        Err(anyhow::anyhow!(
            "unsupported analyze target: {}",
            target.display()
        ))
    }
}

fn analyze_project(root: &Path, config: &AnalyzeConfig) -> Result<AnalyzeReport> {
    fswalk::ensure_dir(root)?;
    let paths = walk_project(root, config)?;
    let file_paths = paths
        .iter()
        .filter(|path| path.is_file() && !files::is_binary_like(path))
        .cloned()
        .collect::<Vec<_>>();
    let analyses = file_paths
        .par_iter()
        .filter_map(|path| analyze_file_inner(root, path, config).ok())
        .collect::<Vec<_>>();

    let mut stats = LineStats {
        files: 0,
        folders: 0,
        lines: 0,
        comments: 0,
        blanks: 0,
    };
    let mut file_types = BTreeMap::<String, usize>::new();
    let mut largest_files = Vec::new();
    let mut large_files = Vec::new();
    let mut language_map = BTreeMap::<String, AnalyzeLanguage>::new();
    let mut todo_count = 0;
    let mut logging_count = 0;
    let mut all_import_edges = Vec::new();

    for path in &paths {
        if path.is_dir() {
            stats.folders += 1;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        stats.files += 1;
        *file_types.entry(fswalk::extension(path)).or_default() += 1;
        let bytes = fs::metadata(path)?.len();
        let entry = FileEntry {
            path: fswalk::relative(root, path),
            bytes,
        };
        largest_files.push(entry.clone());
        if bytes >= config.large_file_bytes {
            large_files.push(entry);
        }
    }

    for file in &analyses {
        stats.lines += file.stats.lines;
        stats.comments += file.stats.comments;
        stats.blanks += file.stats.blanks;
        todo_count += file.todo_count;
        logging_count += file.logging_count;
        let language = language_map
            .entry(file.language.clone())
            .or_insert_with(|| AnalyzeLanguage {
                name: file.language.clone(),
                files: 0,
                lines: 0,
                bytes: 0,
            });
        language.files += 1;
        language.lines += file.stats.lines;
        language.bytes += file.bytes;
        for import in &file.imports {
            all_import_edges.push(AnalyzeImportEdge {
                from: fswalk::relative(root, &file.path),
                to: import.clone(),
            });
        }
    }

    largest_files.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
    largest_files.truncate(DISPLAY_LIMIT);
    large_files.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));

    let project_types = detect_project_types(root)?;
    let dependencies = collect_dependencies(root)?;
    let frameworks = detect_frameworks(root, &dependencies)?;
    let duplicate_assets = duplicate_assets(root)?;
    let duplicate_code = duplicate_code_blocks(&analyses, root);
    let architecture = architecture(all_import_edges, &analyses);
    let git_info = git_intelligence(root).ok();
    let hotspots = project_hotspots(&analyses, git_info.as_ref());
    let mut issues = project_issues(ProjectIssueContext {
        root,
        project_types: &project_types,
        dependencies: &dependencies,
        todo_count,
        logging_count,
        large_files: &large_files,
        duplicate_assets: &duplicate_assets,
        duplicate_code: &duplicate_code,
        architecture: &architecture,
        hotspots: &hotspots,
    });
    for file in &analyses {
        issues.extend(file.issues.clone());
    }
    issues.sort_by_key(|issue| priority_rank(&issue.priority));
    let health_breakdown = health_breakdown(root, &issues, git_info.as_ref(), &analyses);
    let health_score = score_from_breakdown(&health_breakdown);
    let risk_level = risk_level(health_score).to_string();
    let recommendations = recommendations(&issues);
    let warnings = warnings_from_issues(&issues);
    let mut languages = language_map.into_values().collect::<Vec<_>>();
    languages.sort_by_key(|language| std::cmp::Reverse(language.lines));

    Ok(AnalyzeReport {
        root: root.to_path_buf(),
        target_kind: "project".to_string(),
        project_name: project_name(root),
        project_types,
        stats,
        file_types,
        languages,
        frameworks,
        dependencies,
        largest_files,
        todo_count,
        logging_count,
        duplicate_assets,
        large_files,
        health_score,
        risk_level,
        health_breakdown,
        warnings,
        issues,
        recommendations,
        git: git_info,
        architecture,
        hotspots,
        config_used: config.used(),
    })
}

fn analyze_file_target(path: &Path, config: &AnalyzeConfig) -> Result<AnalyzeFileReport> {
    fswalk::ensure_file(path)?;
    analyze_file_inner(
        path.parent().unwrap_or_else(|| Path::new(".")),
        path,
        config,
    )
}

fn analyze_file_inner(
    root: &Path,
    path: &Path,
    config: &AnalyzeConfig,
) -> Result<AnalyzeFileReport> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read source file {}", path.display()))?;
    let language = detect_language_for_file(path, &text);
    let stats = line_stats(&text);
    let bytes = fs::metadata(path)?.len();
    let analyzer = LanguageHeuristics::for_language(&language);
    let imports = analyzer.extract_imports(&text);
    let exports = analyzer.extract_exports(&text);
    let symbols = analyzer.extract_symbols(&text);
    let max_nesting_depth = max_nesting_depth(&text);
    let large_functions = symbols
        .iter()
        .filter(|symbol| symbol.kind == "function" && symbol.lines > config.max_function_lines)
        .cloned()
        .collect::<Vec<_>>();
    let complexity = AnalyzeComplexity {
        functions: symbols
            .iter()
            .filter(|symbol| symbol.kind == "function")
            .count(),
        classes: symbols
            .iter()
            .filter(|symbol| symbol.kind == "class")
            .count(),
        interfaces: symbols
            .iter()
            .filter(|symbol| symbol.kind == "interface" || symbol.kind == "type")
            .count(),
        enums: symbols
            .iter()
            .filter(|symbol| symbol.kind == "enum")
            .count(),
        traits: symbols
            .iter()
            .filter(|symbol| symbol.kind == "trait")
            .count(),
        imports: imports.len(),
        exports: exports.len(),
        max_nesting_depth,
        large_functions,
    };
    let todo_count = if config.warn_todo {
        todo_marker_count(&language, &text)
    } else {
        0
    };
    let logging_count = if config.warn_console_log {
        analyzer.debug_logging_count(&text)
    } else {
        0
    };
    let issues = file_issues(
        root,
        path,
        &stats,
        todo_count,
        logging_count,
        &complexity,
        config,
    );
    let health_breakdown = health_breakdown_for_file(&issues);
    let risk_score = score_from_breakdown(&health_breakdown);
    let risk_level = risk_level(risk_score).to_string();
    let recommendations = recommendations(&issues);

    Ok(AnalyzeFileReport {
        target_kind: "file".to_string(),
        path: path.to_path_buf(),
        language,
        bytes,
        stats,
        symbols,
        imports,
        exports,
        complexity,
        todo_count,
        logging_count,
        issues,
        risk_score,
        risk_level,
        recommendations,
        config_used: config.used(),
    })
}

#[derive(Clone, Debug, Deserialize)]
struct AnalyzeConfig {
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(default = "default_max_file_lines")]
    max_file_lines: usize,
    #[serde(default = "default_max_function_lines")]
    max_function_lines: usize,
    #[serde(default = "default_max_nesting_depth")]
    max_nesting_depth: usize,
    #[serde(default = "default_true")]
    warn_console_log: bool,
    #[serde(default = "default_true")]
    warn_todo: bool,
    #[serde(default = "default_health_fail_below")]
    health_fail_below: u8,
    #[serde(skip)]
    large_file_bytes: u64,
}

impl Default for AnalyzeConfig {
    fn default() -> Self {
        Self {
            ignore: Vec::new(),
            max_file_lines: default_max_file_lines(),
            max_function_lines: default_max_function_lines(),
            max_nesting_depth: default_max_nesting_depth(),
            warn_console_log: true,
            warn_todo: true,
            health_fail_below: default_health_fail_below(),
            large_file_bytes: 512 * 1024,
        }
    }
}

impl AnalyzeConfig {
    fn load(path: Option<&Path>, large_file_bytes: u64) -> Result<Self> {
        let path = path.map(PathBuf::from).or_else(|| {
            let default = PathBuf::from("devmate.toml");
            default.exists().then_some(default)
        });
        let mut config = if let Some(path) = path {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            toml::from_str::<AnalyzeConfig>(&text)
                .with_context(|| format!("failed to parse {}", path.display()))?
        } else {
            AnalyzeConfig::default()
        };
        config.large_file_bytes = large_file_bytes;
        Ok(config)
    }

    fn with_large_file_bytes(mut self, large_file_bytes: u64) -> Self {
        self.large_file_bytes = large_file_bytes;
        self
    }

    fn used(&self) -> AnalyzeConfigUsed {
        AnalyzeConfigUsed {
            ignore: self.ignore.clone(),
            max_file_lines: self.max_file_lines,
            max_function_lines: self.max_function_lines,
            max_nesting_depth: self.max_nesting_depth,
            warn_console_log: self.warn_console_log,
            warn_todo: self.warn_todo,
            health_fail_below: self.health_fail_below,
        }
    }
}

fn default_max_file_lines() -> usize {
    500
}

fn default_max_function_lines() -> usize {
    80
}

fn default_max_nesting_depth() -> usize {
    5
}

fn default_health_fail_below() -> u8 {
    75
}

fn default_true() -> bool {
    true
}

fn walk_project(root: &Path, config: &AnalyzeConfig) -> Result<Vec<PathBuf>> {
    let mut paths = fswalk::walk(root)?;
    paths.retain(|path| {
        !path.components().any(|component| {
            let value = component.as_os_str().to_string_lossy();
            BUILTIN_IGNORES.contains(&value.as_ref())
                || config.ignore.iter().any(|ignore| ignore == value.as_ref())
        })
    });
    Ok(paths)
}

pub fn detect_project_types(root: &Path) -> Result<Vec<String>> {
    let mut types = Vec::<String>::new();
    add_manifest_project_types(root, &mut types)?;
    add_extension_project_types(root, &mut types)?;
    dedupe(&mut types);
    if types.is_empty() {
        types.push("Unknown".to_string());
    }
    Ok(types)
}

fn add_manifest_project_types(root: &Path, types: &mut Vec<String>) -> Result<()> {
    let package_json = root.join("package.json");
    add_if_exists(root, "Cargo.toml", types, "Rust");
    add_if_exists(root, "go.mod", types, "Go");
    if root.join("pyproject.toml").exists()
        || root.join("requirements.txt").exists()
        || root.join("setup.py").exists()
    {
        types.push("Python".to_string());
    }
    if package_json.exists() {
        types.push("Node".to_string());
        let package = read_json_object(&package_json)?;
        let deps = package_dependencies(&package);
        for (dep, label) in node_framework_map() {
            if deps.contains_key(*dep) {
                types.push(label.to_string());
            }
        }
        if root.join("tsconfig.json").exists() || deps.contains_key("typescript") {
            types.push("TypeScript".to_string());
        }
        if deps.contains_key("mongodb") || deps.contains_key("mongoose") {
            types.push("MongoDB".to_string());
        }
        if deps.contains_key("pg") || deps.contains_key("postgres") {
            types.push("PostgreSQL".to_string());
        }
        if deps.contains_key("redis") {
            types.push("Redis".to_string());
        }
    }
    for (file, labels) in [
        ("deno.json", vec!["Deno", "TypeScript"]),
        ("deno.jsonc", vec!["Deno", "TypeScript"]),
        ("pom.xml", vec!["Java", "Maven"]),
        ("composer.json", vec!["PHP", "Composer"]),
        ("pubspec.yaml", vec!["Dart"]),
        ("mix.exs", vec!["Elixir"]),
        ("Package.swift", vec!["Swift"]),
    ] {
        if root.join(file).exists() {
            types.extend(labels.into_iter().map(str::to_string));
        }
    }
    if has_any(
        root,
        &["Dockerfile", "docker-compose.yml", "docker-compose.yaml"],
    ) {
        types.push("Docker".to_string());
    }
    if has_any(
        root,
        &[
            "compose.yml",
            "compose.yaml",
            "docker-compose.yml",
            "docker-compose.yaml",
        ],
    ) {
        types.push("Docker Compose".to_string());
    }
    if has_any(root, &["main.tf", "variables.tf", "outputs.tf"]) {
        types.push("Terraform".to_string());
    }
    if has_any(
        root,
        &["build.gradle", "build.gradle.kts", "settings.gradle"],
    ) {
        types.push("Gradle".to_string());
        if file_contains(&root.join("build.gradle"), "kotlin")
            || file_contains(&root.join("build.gradle.kts"), "kotlin")
        {
            types.push("Kotlin".to_string());
        } else {
            types.push("Java".to_string());
        }
    }
    if root.join("Gemfile").exists() || root.join("gems.rb").exists() {
        types.push("Ruby".to_string());
        types.push("Bundler".to_string());
    }
    if glob_exists(root, "csproj") {
        types.push("C#".to_string());
        types.push(".NET".to_string());
    }
    if glob_exists(root, "sln") {
        types.push(".NET".to_string());
    }
    Ok(())
}

fn add_extension_project_types(root: &Path, types: &mut Vec<String>) -> Result<()> {
    let paths = fswalk::walk(root)?;
    let mut extensions = BTreeMap::<String, usize>::new();
    for path in paths.iter().filter(|path| path.is_file()) {
        *extensions.entry(fswalk::extension(path)).or_default() += 1;
    }
    for (extension, label) in language_extension_map() {
        if extensions.get(*extension).copied().unwrap_or_default() > 0 {
            types.push(label.to_string());
        }
    }
    if paths.iter().any(|path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| {
                name.eq_ignore_ascii_case("Dockerfile") || name.ends_with(".dockerfile")
            })
    }) {
        types.push("Docker".to_string());
    }
    if paths.iter().any(|path| {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        (name.ends_with(".yaml") || name.ends_with(".yml"))
            && file_contains(path, "apiversion:")
            && file_contains(path, "kind:")
    }) {
        types.push("Kubernetes".to_string());
    }
    Ok(())
}

fn language_extension_map() -> &'static [(&'static str, &'static str)] {
    &[
        ("rs", "Rust"),
        ("go", "Go"),
        ("py", "Python"),
        ("js", "JavaScript"),
        ("jsx", "React"),
        ("ts", "TypeScript"),
        ("tsx", "React"),
        ("java", "Java"),
        ("kt", "Kotlin"),
        ("kts", "Kotlin"),
        ("scala", "Scala"),
        ("c", "C"),
        ("h", "C"),
        ("cpp", "C++"),
        ("cc", "C++"),
        ("cxx", "C++"),
        ("hpp", "C++"),
        ("cs", "C#"),
        ("php", "PHP"),
        ("rb", "Ruby"),
        ("swift", "Swift"),
        ("dart", "Dart"),
        ("ex", "Elixir"),
        ("exs", "Elixir"),
        ("erl", "Erlang"),
        ("hrl", "Erlang"),
        ("hs", "Haskell"),
        ("lua", "Lua"),
        ("r", "R"),
        ("jl", "Julia"),
        ("zig", "Zig"),
        ("nim", "Nim"),
        ("sql", "SQL"),
        ("tf", "Terraform"),
        ("tfvars", "Terraform"),
        ("yaml", "YAML"),
        ("yml", "YAML"),
    ]
}

fn detect_language_for_file(path: &Path, text: &str) -> String {
    let extension = fswalk::extension(path);
    match extension.as_str() {
        "rs" => "Rust",
        "go" => "Go",
        "py" => "Python",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "cs" => "C#",
        "php" => "PHP",
        "rb" => "Ruby",
        "swift" => "Swift",
        "dart" => "Dart",
        "scala" => "Scala",
        "ex" | "exs" => "Elixir",
        "lua" => "Lua",
        "zig" => "Zig",
        "hs" => "Haskell",
        "r" => "R",
        _ if text.starts_with("#!") && text.contains("python") => "Python",
        _ => "Unknown",
    }
    .to_string()
}

struct LanguageHeuristics {
    language: String,
}

impl LanguageHeuristics {
    fn for_language(language: &str) -> Self {
        Self {
            language: language.to_string(),
        }
    }

    fn extract_imports(&self, text: &str) -> Vec<String> {
        let patterns = match self.language.as_str() {
            "Rust" => vec![r"(?m)^use\s+([^;]+);", r"(?m)^mod\s+([A-Za-z0-9_]+)\s*;"],
            "JavaScript" | "TypeScript" => vec![
                r#"(?m)^\s*import\s+.*?\s+from\s+["']([^"']+)["']"#,
                r#"(?m)^\s*import\s+["']([^"']+)["']"#,
                r#"require\(["']([^"']+)["']\)"#,
            ],
            "Python" => vec![
                r"(?m)^\s*import\s+([A-Za-z0-9_., ]+)",
                r"(?m)^\s*from\s+([A-Za-z0-9_.]+)\s+import",
            ],
            "Go" => vec![r#"(?m)^\s*import\s+"([^"]+)""#, r#"(?m)^\s*"([^"]+)""#],
            "Java" | "Kotlin" | "Scala" => vec![r"(?m)^\s*import\s+([^;]+);?"],
            "C" | "C++" => vec![r#"(?m)^\s*#include\s+[<"]([^>"]+)[>"]"#],
            "C#" => vec![r"(?m)^\s*using\s+([^;]+);"],
            "PHP" => vec![
                r"(?m)^\s*use\s+([^;]+);",
                r#"require(?:_once)?\s*["']([^"']+)["']"#,
            ],
            "Ruby" => vec![r#"(?m)^\s*require\s+["']([^"']+)["']"#],
            "Swift" => vec![r"(?m)^\s*import\s+([A-Za-z0-9_]+)"],
            "Dart" => vec![r#"(?m)^\s*import\s+['"]([^'"]+)['"]"#],
            "Elixir" => vec![
                r"(?m)^\s*alias\s+([A-Za-z0-9_.]+)",
                r"(?m)^\s*import\s+([A-Za-z0-9_.]+)",
            ],
            "Lua" => vec![r#"require\s*\(?["']([^"']+)["']"#],
            "Zig" => vec![r#"@import\(["']([^"']+)["']\)"#],
            "Haskell" => vec![r"(?m)^\s*import\s+(?:qualified\s+)?([A-Za-z0-9_.]+)"],
            "R" => vec![r#"library\(([^)]+)\)"#, r#"require\(([^)]+)\)"#],
            _ => Vec::new(),
        };
        capture_all(text, &patterns)
    }

    fn extract_exports(&self, text: &str) -> Vec<String> {
        let patterns = match self.language.as_str() {
            "JavaScript" | "TypeScript" => vec![
                r"(?m)^\s*export\s+(?:default\s+)?(?:class|function|const|let|var|interface|type|enum)\s+([A-Za-z0-9_]+)",
                r"(?m)^\s*export\s*\{([^}]+)\}",
            ],
            "Rust" => vec![r"(?m)^\s*pub\s+(?:fn|struct|enum|trait|mod)\s+([A-Za-z0-9_]+)"],
            _ => Vec::new(),
        };
        capture_all(text, &patterns)
    }

    fn extract_symbols(&self, text: &str) -> Vec<AnalyzeSymbol> {
        let patterns = symbol_patterns(&self.language);
        let mut symbols = Vec::new();
        for (kind, pattern) in patterns {
            let Ok(regex) = Regex::new(pattern) else {
                continue;
            };
            for capture in regex.captures_iter(text) {
                let uses_named_captures = capture.name("name").is_some();
                if let Some(name) = capture.name("name").or_else(|| capture.get(1)) {
                    let line = text[..name.start()].matches('\n').count() + 1;
                    let visibility = if uses_named_captures {
                        capture.name("visibility")
                    } else {
                        capture.get(2)
                    }
                    .map(|value| value.as_str().trim().to_string())
                    .filter(|value| !value.is_empty());
                    symbols.push(AnalyzeSymbol {
                        kind: kind.to_string(),
                        name: name.as_str().to_string(),
                        line,
                        visibility,
                        lines: estimate_symbol_lines(text, line),
                    });
                }
            }
        }
        symbols.sort_by_key(|symbol| symbol.line);
        symbols
    }

    fn debug_logging_count(&self, text: &str) -> usize {
        let patterns = self.debug_logging_patterns();
        text.lines()
            .map(strip_quoted_literals)
            .map(|line| {
                patterns
                    .iter()
                    .filter(|pattern| debug_pattern_matches(&line, pattern))
                    .count()
            })
            .sum()
    }

    fn debug_logging_patterns(&self) -> Vec<&'static str> {
        match self.language.as_str() {
            "Rust" => vec!["println!", "dbg!", "eprintln!"],
            "JavaScript" | "TypeScript" => vec!["console.log", "console.debug", "console.warn"],
            "Python" => vec!["print(", "logging.debug"],
            "Java" | "Kotlin" | "Scala" => vec!["System.out.println", "println("],
            "C#" => vec!["Console.WriteLine", "Debug.WriteLine"],
            "PHP" => vec!["var_dump", "print_r", "dd(", "dump("],
            "Ruby" => vec!["puts ", "p "],
            "Swift" => vec!["print(", "NSLog"],
            _ => vec!["print(", "debug", "log.debug"],
        }
    }
}

fn symbol_patterns(language: &str) -> Vec<(&'static str, &'static str)> {
    match language {
        "Rust" => vec![
            (
                "function",
                r"(?m)^\s*(?P<visibility>pub(?:\([^)]*\))?\s+)?fn\s+(?P<name>[A-Za-z0-9_]+)",
            ),
            (
                "class",
                r"(?m)^\s*(?P<visibility>pub(?:\([^)]*\))?\s+)?struct\s+(?P<name>[A-Za-z0-9_]+)",
            ),
            (
                "enum",
                r"(?m)^\s*(?P<visibility>pub(?:\([^)]*\))?\s+)?enum\s+(?P<name>[A-Za-z0-9_]+)",
            ),
            (
                "trait",
                r"(?m)^\s*(?P<visibility>pub(?:\([^)]*\))?\s+)?trait\s+(?P<name>[A-Za-z0-9_]+)",
            ),
        ],
        "JavaScript" | "TypeScript" => vec![
            (
                "function",
                r"(?m)^\s*(?:export\s+)?(?:async\s+)?function\s+([A-Za-z0-9_]+)",
            ),
            (
                "function",
                r"(?m)^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z0-9_]+)\s*=\s*(?:async\s*)?\(",
            ),
            ("class", r"(?m)^\s*(?:export\s+)?class\s+([A-Za-z0-9_]+)"),
            (
                "interface",
                r"(?m)^\s*(?:export\s+)?interface\s+([A-Za-z0-9_]+)",
            ),
            ("type", r"(?m)^\s*(?:export\s+)?type\s+([A-Za-z0-9_]+)"),
            ("enum", r"(?m)^\s*(?:export\s+)?enum\s+([A-Za-z0-9_]+)"),
        ],
        "Python" => vec![
            ("function", r"(?m)^\s*def\s+([A-Za-z0-9_]+)"),
            ("class", r"(?m)^\s*class\s+([A-Za-z0-9_]+)"),
        ],
        "Go" => vec![
            (
                "function",
                r"(?m)^\s*func\s+(?:\([^)]*\)\s*)?([A-Za-z0-9_]+)",
            ),
            ("class", r"(?m)^\s*type\s+([A-Za-z0-9_]+)\s+struct"),
            ("interface", r"(?m)^\s*type\s+([A-Za-z0-9_]+)\s+interface"),
        ],
        "Java" | "Kotlin" | "C#" | "PHP" | "Swift" | "Dart" | "Scala" => vec![
            (
                "class",
                r"(?m)^\s*(?:public|private|protected|internal|open|final|abstract|\s)*class\s+([A-Za-z0-9_]+)",
            ),
            (
                "interface",
                r"(?m)^\s*(?:public|private|protected|internal|\s)*interface\s+([A-Za-z0-9_]+)",
            ),
            (
                "enum",
                r"(?m)^\s*(?:public|private|protected|internal|\s)*enum\s+([A-Za-z0-9_]+)",
            ),
            (
                "function",
                r"(?m)^\s*(?:public|private|protected|internal|static|fun|func|\s)+\s*([A-Za-z0-9_]+)\s*\(",
            ),
        ],
        "Ruby" => vec![
            ("function", r"(?m)^\s*def\s+([A-Za-z0-9_!?=]+)"),
            ("class", r"(?m)^\s*class\s+([A-Za-z0-9_:]+)"),
        ],
        "Elixir" => vec![
            ("function", r"(?m)^\s*defp?\s+([A-Za-z0-9_!?]+)"),
            ("class", r"(?m)^\s*defmodule\s+([A-Za-z0-9_.]+)"),
        ],
        "Lua" => vec![("function", r"(?m)^\s*function\s+([A-Za-z0-9_.:]+)")],
        "Zig" => vec![("function", r"(?m)^\s*(?:pub\s+)?fn\s+([A-Za-z0-9_]+)")],
        "Haskell" => vec![("function", r"(?m)^([a-z][A-Za-z0-9_']*)\s+.*=")],
        "R" => vec![("function", r"(?m)^([A-Za-z0-9_.]+)\s*<-\s*function")],
        _ => Vec::new(),
    }
}

fn todo_marker_count(language: &str, text: &str) -> usize {
    let comment_text = comment_text_for_markers(language, text);
    Regex::new(r"(?i)\b(?:todo|fixme)\b")
        .map(|regex| regex.find_iter(&comment_text).count())
        .unwrap_or_default()
}

fn comment_text_for_markers(language: &str, text: &str) -> String {
    match language {
        "Rust" | "JavaScript" | "TypeScript" | "Java" | "Kotlin" | "Scala" | "C" | "C++" | "C#"
        | "PHP" | "Swift" | "Dart" | "Go" => c_style_comments(text),
        "Python" | "Ruby" | "R" => line_comments(text, &["#"]),
        "Lua" | "SQL" => line_comments(text, &["--"]),
        _ => line_comments(text, &["//", "#", "--"]),
    }
}

fn c_style_comments(text: &str) -> String {
    let mut comments = String::new();
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut index = 0;
    let mut quote = None;
    let mut escaped = false;

    while index < chars.len() {
        let (byte_index, ch) = chars[index];
        let next = chars.get(index + 1).map(|(_, next)| *next);
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            index += 1;
            continue;
        }

        if matches!(ch, '"' | '\'') {
            quote = Some(ch);
            index += 1;
            continue;
        }

        if ch == '/' && next == Some('/') {
            let rest = &text[byte_index..];
            let end = rest.find('\n').unwrap_or(rest.len());
            comments.push_str(&rest[..end]);
            comments.push('\n');
            index += rest[..end].chars().count();
            continue;
        }

        if ch == '/' && next == Some('*') {
            let rest = &text[byte_index..];
            let end = rest.find("*/").map(|end| end + 2).unwrap_or(rest.len());
            comments.push_str(&rest[..end]);
            comments.push('\n');
            index += rest[..end].chars().count();
            continue;
        }

        index += 1;
    }

    comments
}

fn line_comments(text: &str, markers: &[&str]) -> String {
    let mut comments = String::new();
    for line in text.lines() {
        let Some(index) = markers.iter().filter_map(|marker| line.find(marker)).min() else {
            continue;
        };
        comments.push_str(&line[index..]);
        comments.push('\n');
    }
    comments
}

fn strip_quoted_literals(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut quote = None;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            output.push(' ');
        } else if matches!(ch, '"' | '\'') {
            quote = Some(ch);
            output.push(' ');
        } else {
            output.push(ch);
        }
    }
    output
}

fn debug_pattern_matches(line: &str, pattern: &str) -> bool {
    let Some(index) = line.find(pattern) else {
        return false;
    };
    if pattern.ends_with('!') {
        let previous = line[..index].chars().next_back();
        if matches!(
            previous,
            Some(':') | Some('_') | Some('a'..='z') | Some('A'..='Z') | Some('0'..='9')
        ) {
            return false;
        }
    }
    true
}

fn capture_all(text: &str, patterns: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    for pattern in patterns {
        let Ok(regex) = Regex::new(pattern) else {
            continue;
        };
        for capture in regex.captures_iter(text) {
            if let Some(value) = capture.get(1) {
                values.push(value.as_str().trim().to_string());
            }
        }
    }
    dedupe(&mut values);
    values
}

fn estimate_symbol_lines(text: &str, start_line: usize) -> usize {
    let lines = text
        .lines()
        .skip(start_line.saturating_sub(1))
        .collect::<Vec<_>>();
    let mut depth = 0_i32;
    let mut seen_body = false;
    for (idx, line) in lines.iter().enumerate() {
        let code = strip_quoted_literals(line);
        depth += code.matches('{').count() as i32;
        if code.contains('{') {
            seen_body = true;
        }
        depth -= code.matches('}').count() as i32;
        if seen_body && depth <= 0 && idx > 0 {
            return idx + 1;
        }
        if !seen_body && idx > 0 && line.trim().is_empty() {
            return idx;
        }
    }
    lines.len().min(200)
}

fn max_nesting_depth(text: &str) -> usize {
    let mut depth = 0_usize;
    let mut max = 0_usize;
    for line in text.lines() {
        let code = strip_quoted_literals(line);
        for ch in code.chars() {
            if ch == '{' {
                depth += 1;
                max = max.max(depth);
            } else if ch == '}' {
                depth = depth.saturating_sub(1);
            }
        }
    }
    max
}

fn line_stats(text: &str) -> LineStats {
    let mut stats = LineStats {
        files: 1,
        folders: 0,
        lines: 0,
        comments: 0,
        blanks: 0,
    };
    files::add_line_counts(text, &mut stats);
    stats.files = 1;
    stats
}

fn collect_dependencies(root: &Path) -> Result<Vec<Dependency>> {
    let mut deps = Vec::new();
    let package_json = root.join("package.json");
    if package_json.exists() {
        let package = read_json_object(&package_json)?;
        for (name, version) in package_dependencies(&package) {
            deps.push(Dependency {
                name,
                version: Some(version),
                source: "package.json".to_string(),
            });
        }
    }
    let cargo = root.join("Cargo.toml");
    if cargo.exists() {
        deps.extend(parse_simple_manifest(
            &cargo,
            "Cargo.toml",
            &[
                "[dependencies]",
                "[dev-dependencies]",
                "[build-dependencies]",
            ],
        )?);
    }
    for (path, parser) in [
        (
            root.join("pyproject.toml"),
            parse_pyproject as fn(&Path) -> Result<Vec<Dependency>>,
        ),
        (root.join("requirements.txt"), parse_requirements),
        (root.join("go.mod"), parse_go_mod),
        (root.join("composer.json"), parse_composer),
        (root.join("pubspec.yaml"), parse_yaml_dependencies),
        (root.join("deno.json"), parse_deno),
        (root.join("deno.jsonc"), parse_deno),
    ] {
        if path.exists() {
            deps.extend(parser(&path)?);
        }
    }
    for file in ["Gemfile", "gems.rb"] {
        let path = root.join(file);
        if path.exists() {
            deps.extend(parse_gemfile(&path)?);
        }
    }
    for (file, pattern) in [
        ("mix.exs", r#"\{:\s*([A-Za-z0-9_]+)\s*,\s*"([^"]+)""#),
        ("Package.swift", r#"\.package\([^)]*url:\s*"([^"]+)""#),
    ] {
        let path = root.join(file);
        if path.exists() {
            deps.extend(parse_regex_dependencies(&path, file, pattern)?);
        }
    }
    for file in ["pom.xml", "build.gradle", "build.gradle.kts"] {
        let path = root.join(file);
        if path.exists() {
            deps.extend(match file {
                "pom.xml" => parse_pom(&path)?,
                _ => parse_gradle(&path)?,
            });
        }
    }
    for path in fswalk::walk(root)?.into_iter().filter(|path| {
        matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("csproj" | "tf")
        )
    }) {
        if path.extension().and_then(|value| value.to_str()) == Some("csproj") {
            deps.extend(parse_csproj(&path)?);
        } else {
            deps.extend(parse_terraform(&path)?);
        }
    }
    dedupe_dependencies(&mut deps);
    deps.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(deps)
}

fn detect_frameworks(root: &Path, deps: &[Dependency]) -> Result<Vec<String>> {
    let mut frameworks = Vec::new();
    let dep_names = deps
        .iter()
        .map(|dep| dep.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    for (needle, framework) in framework_map() {
        if dep_names.iter().any(|name| name.contains(needle)) {
            frameworks.push(framework.to_string());
        }
    }
    if file_contains(&root.join("Cargo.toml"), "tokio") {
        frameworks.push("Tokio".to_string());
    }
    if file_contains(&root.join("pom.xml"), "spring-boot") {
        frameworks.push("Spring Boot".to_string());
    }
    dedupe(&mut frameworks);
    Ok(frameworks)
}

fn framework_map() -> &'static [(&'static str, &'static str)] {
    &[
        ("react", "React"),
        ("next", "Next.js"),
        ("express", "Express"),
        ("@nestjs", "NestJS"),
        ("vite", "Vite"),
        ("vue", "Vue"),
        ("nuxt", "Nuxt"),
        ("@angular", "Angular"),
        ("astro", "Astro"),
        ("svelte", "Svelte"),
        ("django", "Django"),
        ("flask", "Flask"),
        ("fastapi", "FastAPI"),
        ("axum", "Axum"),
        ("actix", "Actix"),
        ("rocket", "Rocket"),
        ("tauri", "Tauri"),
        ("bevy", "Bevy"),
        ("gin-gonic", "Gin"),
        ("gofiber", "Fiber"),
        ("labstack/echo", "Echo"),
        ("spring", "Spring Boot"),
        ("laravel", "Laravel"),
        ("symfony", "Symfony"),
    ]
}

fn node_framework_map() -> &'static [(&'static str, &'static str)] {
    &[
        ("react", "React"),
        ("next", "Next.js"),
        ("express", "Express"),
        ("@nestjs/core", "NestJS"),
        ("vite", "Vite"),
        ("vue", "Vue"),
        ("nuxt", "Nuxt"),
        ("@angular/core", "Angular"),
        ("astro", "Astro"),
        ("svelte", "Svelte"),
        ("tailwindcss", "Tailwind"),
        ("prisma", "Prisma"),
        ("@prisma/client", "Prisma"),
    ]
}

fn file_issues(
    root: &Path,
    path: &Path,
    stats: &LineStats,
    todo_count: usize,
    logging_count: usize,
    complexity: &AnalyzeComplexity,
    config: &AnalyzeConfig,
) -> Vec<AnalyzeIssue> {
    let relative = fswalk::relative(root, path);
    let mut issues = Vec::new();
    if stats.lines > config.max_file_lines {
        issues.push(issue(
            "Large file",
            "Large files are harder to review, test, and safely refactor.",
            "Split this file by responsibility and move reusable code into focused modules.",
            vec![relative.clone()],
            "High",
            "legacy",
            "Medium",
        ));
    }
    if todo_count > 0 {
        issues.push(issue(
            format!("{todo_count} TODO/FIXME markers").as_str(),
            "Unresolved markers often become hidden technical debt.",
            "Convert markers into tracked work items or resolve them before release.",
            vec![relative.clone()],
            "Medium",
            "technical-debt",
            "Small",
        ));
    }
    if logging_count > 0 {
        issues.push(issue(
            format!("{logging_count} debug logging calls").as_str(),
            "Debug output can leak data and make production logs noisy.",
            "Remove ad-hoc debug logging or replace it with structured level-based logging.",
            vec![relative.clone()],
            "Medium",
            "quality",
            "Small",
        ));
    }
    if complexity.max_nesting_depth > config.max_nesting_depth {
        issues.push(issue(
            "Deep nesting",
            "Deeply nested code is harder to reason about and more likely to hide edge-case bugs.",
            "Use early returns, smaller helper functions, or clearer control flow.",
            vec![relative.clone()],
            "High",
            "complexity",
            "Medium",
        ));
    }
    for symbol in &complexity.large_functions {
        issues.push(issue(
            format!("Large function `{}`", symbol.name).as_str(),
            "Large functions tend to mix responsibilities and are difficult to test thoroughly.",
            "Extract smaller functions around one responsibility each.",
            vec![relative.clone()],
            "High",
            "legacy",
            "Medium",
        ));
    }
    issues
}

struct ProjectIssueContext<'a> {
    root: &'a Path,
    project_types: &'a [String],
    dependencies: &'a [Dependency],
    todo_count: usize,
    logging_count: usize,
    large_files: &'a [FileEntry],
    duplicate_assets: &'a [Vec<PathBuf>],
    duplicate_code: &'a [Vec<PathBuf>],
    architecture: &'a AnalyzeArchitecture,
    hotspots: &'a [AnalyzeHotspot],
}

fn project_issues(ctx: ProjectIssueContext<'_>) -> Vec<AnalyzeIssue> {
    let mut issues = Vec::new();
    if ctx.project_types.iter().any(|kind| kind == "Unknown") {
        issues.push(issue(
            "Unknown project type",
            "DevMate could not infer a language or ecosystem.",
            "Add a recognizable manifest or source files.",
            vec![ctx.root.to_path_buf()],
            "Medium",
            "detection",
            "Small",
        ));
    }
    if ctx.dependencies.is_empty() {
        issues.push(issue(
            "No dependencies detected",
            "Missing dependency data can make audits incomplete.",
            "Check whether supported manifest files are committed.",
            vec![ctx.root.to_path_buf()],
            "Low",
            "dependencies",
            "Small",
        ));
    }
    if !ctx.root.join("README.md").exists() && !ctx.root.join("README").exists() {
        issues.push(issue(
            "Missing README",
            "A missing README slows onboarding and support.",
            "Add a README with setup, usage, and troubleshooting instructions.",
            vec![ctx.root.to_path_buf()],
            "Medium",
            "documentation",
            "Small",
        ));
    }
    if ctx.todo_count > 10 {
        issues.push(issue(
            "High TODO/FIXME count",
            "Many unresolved markers indicate accumulated technical debt.",
            "Triage markers and convert important ones into tracked tasks.",
            vec![ctx.root.to_path_buf()],
            "Medium",
            "technical-debt",
            "Small",
        ));
    }
    if ctx.logging_count > 10 {
        issues.push(issue(
            "High debug logging count",
            "Debug output can leak data and obscure useful production logs.",
            "Remove debug logs or gate them behind structured logging levels.",
            vec![ctx.root.to_path_buf()],
            "Medium",
            "quality",
            "Small",
        ));
    }
    if !ctx.large_files.is_empty() {
        issues.push(issue(
            "Large files detected",
            "Large files are often hotspots for bugs and merge conflicts.",
            "Split the largest files into smaller modules.",
            ctx.large_files
                .iter()
                .take(5)
                .map(|entry| entry.path.clone())
                .collect(),
            "High",
            "legacy",
            "Medium",
        ));
    }
    if !ctx.duplicate_assets.is_empty() {
        issues.push(issue(
            "Duplicate assets detected",
            "Duplicate assets increase repository size and maintenance work.",
            "Keep one canonical asset and update references.",
            ctx.duplicate_assets
                .iter()
                .flat_map(|group| group.iter().cloned())
                .take(5)
                .collect(),
            "Low",
            "assets",
            "Small",
        ));
    }
    if !ctx.duplicate_code.is_empty() {
        issues.push(issue(
            "Duplicate code blocks detected",
            "Repeated code multiplies bug-fix and change effort.",
            "Extract shared behavior into a common helper or component.",
            ctx.duplicate_code
                .iter()
                .flat_map(|group| group.iter().cloned())
                .take(5)
                .collect(),
            "High",
            "duplication",
            "Medium",
        ));
    }
    if !ctx.architecture.circular_dependencies.is_empty() {
        issues.push(issue(
            "Circular dependencies detected",
            "Cycles make modules harder to test, reuse, and refactor.",
            "Move shared contracts into a lower-level module and remove bidirectional imports.",
            ctx.architecture
                .circular_dependencies
                .iter()
                .flat_map(|group| group.iter().cloned())
                .take(5)
                .collect(),
            "High",
            "architecture",
            "Medium",
        ));
    }
    if ctx.hotspots.iter().any(|hotspot| hotspot.score >= 5) {
        issues.push(issue(
            "High-churn hotspots",
            "Frequently changed files with complexity are more likely to produce regressions.",
            "Prioritize tests and refactoring around the highest-churn files.",
            ctx.hotspots
                .iter()
                .take(5)
                .map(|hotspot| hotspot.path.clone())
                .collect(),
            "High",
            "git",
            "Medium",
        ));
    }
    issues
}

fn issue(
    problem: &str,
    why_it_matters: &str,
    suggested_fix: &str,
    affected_files: Vec<PathBuf>,
    priority: &str,
    category: &str,
    estimated_effort: &str,
) -> AnalyzeIssue {
    AnalyzeIssue {
        problem: problem.to_string(),
        why_it_matters: why_it_matters.to_string(),
        suggested_fix: suggested_fix.to_string(),
        affected_files,
        priority: priority.to_string(),
        category: category.to_string(),
        estimated_effort: estimated_effort.to_string(),
    }
}

fn health_breakdown(
    root: &Path,
    issues: &[AnalyzeIssue],
    git_info: Option<&AnalyzeGit>,
    analyses: &[AnalyzeFileReport],
) -> Vec<HealthScoreItem> {
    let mut items = vec![HealthScoreItem {
        label: "Base score".to_string(),
        points: 100,
    }];
    let mut grouped = BTreeMap::<String, i32>::new();
    for issue in issues {
        let points = match issue.priority.as_str() {
            "High" => -8,
            "Medium" => -5,
            _ => -3,
        };
        *grouped.entry(issue.problem.clone()).or_default() += points;
    }
    items.extend(
        grouped
            .into_iter()
            .map(|(label, points)| HealthScoreItem { label, points }),
    );
    if root.join("README.md").exists() || root.join("README").exists() {
        items.push(HealthScoreItem {
            label: "README present".to_string(),
            points: 3,
        });
    }
    if analyses.iter().any(|file| {
        file.path.components().any(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("tests")
        }) || file.path.to_string_lossy().contains(".test.")
            || file.path.to_string_lossy().contains("_test.")
    }) {
        items.push(HealthScoreItem {
            label: "Tests detected".to_string(),
            points: 5,
        });
    }
    if git_info.is_some_and(|git| git.clean) {
        items.push(HealthScoreItem {
            label: "Git status clean".to_string(),
            points: 2,
        });
    }
    items
}

fn health_breakdown_for_file(issues: &[AnalyzeIssue]) -> Vec<HealthScoreItem> {
    let mut items = vec![HealthScoreItem {
        label: "Base score".to_string(),
        points: 100,
    }];
    for issue in issues {
        let points = match issue.priority.as_str() {
            "High" => -10,
            "Medium" => -6,
            _ => -3,
        };
        items.push(HealthScoreItem {
            label: issue.problem.clone(),
            points,
        });
    }
    items
}

fn score_from_breakdown(items: &[HealthScoreItem]) -> u8 {
    items
        .iter()
        .map(|item| item.points)
        .sum::<i32>()
        .clamp(0, 100) as u8
}

fn risk_level(score: u8) -> &'static str {
    match score {
        85..=100 => "Low",
        70..=84 => "Medium",
        50..=69 => "High",
        _ => "Critical",
    }
}

fn recommendations(issues: &[AnalyzeIssue]) -> Vec<AnalyzeRecommendation> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for issue in issues {
        if seen.insert(issue.suggested_fix.clone()) {
            output.push(AnalyzeRecommendation {
                priority: issue.priority.clone(),
                action: issue.suggested_fix.clone(),
                reason: issue.why_it_matters.clone(),
                estimated_effort: issue.estimated_effort.clone(),
            });
        }
        if output.len() >= DISPLAY_LIMIT {
            break;
        }
    }
    output
}

fn warnings_from_issues(issues: &[AnalyzeIssue]) -> Vec<String> {
    issues
        .iter()
        .take(DISPLAY_LIMIT)
        .map(|issue| issue.problem.clone())
        .collect()
}

fn architecture(
    edges: Vec<AnalyzeImportEdge>,
    analyses: &[AnalyzeFileReport],
) -> AnalyzeArchitecture {
    if analyses.len() > 2000 {
        return AnalyzeArchitecture {
            local_imports: Vec::new(),
            circular_dependencies: Vec::new(),
            skipped_reason: Some("Skipped architecture graph for very large project".to_string()),
        };
    }
    let cycles = circular_dependencies(&edges);
    AnalyzeArchitecture {
        local_imports: edges,
        circular_dependencies: cycles,
        skipped_reason: None,
    }
}

fn circular_dependencies(edges: &[AnalyzeImportEdge]) -> Vec<Vec<PathBuf>> {
    let mut cycles = Vec::new();
    let mut lookup = HashSet::new();
    for edge in edges {
        lookup.insert((edge.from.display().to_string(), normalize_import(&edge.to)));
    }
    for edge in edges {
        let from = edge.from.display().to_string();
        let to = normalize_import(&edge.to);
        if lookup.contains(&(to.clone(), normalize_import(&from))) {
            cycles.push(vec![PathBuf::from(from), PathBuf::from(to)]);
        }
        if cycles.len() >= DISPLAY_LIMIT {
            break;
        }
    }
    cycles
}

fn normalize_import(value: &str) -> String {
    value
        .trim_start_matches("./")
        .trim_start_matches("../")
        .replace('\\', "/")
}

fn duplicate_code_blocks(analyses: &[AnalyzeFileReport], root: &Path) -> Vec<Vec<PathBuf>> {
    let mut by_block = HashMap::<String, Vec<PathBuf>>::new();
    for file in analyses.iter().take(1500) {
        let Ok(text) = fs::read_to_string(&file.path) else {
            continue;
        };
        let normalized = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//") && !line.starts_with('#'))
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        for window in normalized.windows(8) {
            let joined = window.join("\n");
            if joined.len() > 120 {
                by_block
                    .entry(joined)
                    .or_default()
                    .push(fswalk::relative(root, &file.path));
            }
        }
    }
    let mut groups = by_block
        .into_values()
        .filter_map(|mut group| {
            group.sort();
            group.dedup();
            (group.len() > 1).then_some(group)
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| std::cmp::Reverse(group.len()));
    groups.truncate(DISPLAY_LIMIT);
    groups
}

fn duplicate_assets(root: &Path) -> Result<Vec<Vec<PathBuf>>> {
    let mut by_name_and_size: HashMap<(String, u64), Vec<PathBuf>> = HashMap::new();
    for path in files::files_only(root)? {
        let extension = fswalk::extension(&path);
        if !ASSET_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let bytes = fs::metadata(&path)?.len();
        by_name_and_size
            .entry((file_name.to_ascii_lowercase(), bytes))
            .or_default()
            .push(fswalk::relative(root, &path));
    }
    let mut groups = by_name_and_size
        .into_values()
        .filter(|paths| paths.len() > 1)
        .collect::<Vec<_>>();
    groups.sort_by_key(|paths| std::cmp::Reverse(paths.len()));
    Ok(groups)
}

fn git_intelligence(root: &Path) -> Result<AnalyzeGit> {
    let summary = git::summary(root, 30)?;
    let repo = Repository::discover(root)?;
    let since = Utc::now() - Duration::days(30);
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(Sort::TIME)?;
    let mut commits_30_days = 0;
    let mut churn = BTreeMap::<PathBuf, usize>::new();
    for oid in revwalk.take(300) {
        let commit = repo.find_commit(oid?)?;
        let Some(time) = chrono::DateTime::<Utc>::from_timestamp(commit.time().seconds(), 0) else {
            continue;
        };
        if time < since {
            break;
        }
        commits_30_days += 1;
        if commit.parent_count() == 0 {
            continue;
        }
        let parent = commit.parent(0)?;
        let tree = commit.tree()?;
        let parent_tree = parent.tree()?;
        let diff = repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)?;
        for delta in diff.deltas() {
            if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
                *churn.entry(path.to_path_buf()).or_default() += 1;
            }
        }
    }
    let mut most_modified_files = churn
        .into_iter()
        .map(|(path, score)| AnalyzeHotspot { path, score })
        .collect::<Vec<_>>();
    most_modified_files.sort_by_key(|item| std::cmp::Reverse(item.score));
    most_modified_files.truncate(DISPLAY_LIMIT);
    Ok(AnalyzeGit {
        branch: summary.branch,
        clean: summary.clean,
        commits_30_days,
        most_modified_files,
        contributors: summary.contributors,
        ahead: summary.ahead,
        behind: summary.behind,
    })
}

fn project_hotspots(
    analyses: &[AnalyzeFileReport],
    git_info: Option<&AnalyzeGit>,
) -> Vec<AnalyzeHotspot> {
    let churn = git_info
        .map(|git| {
            git.most_modified_files
                .iter()
                .map(|item| {
                    (
                        normalize_import(&item.path.display().to_string()),
                        item.score,
                    )
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let mut hotspots = analyses
        .iter()
        .map(|file| {
            let path = normalize_import(&file.path.display().to_string());
            let complexity =
                file.complexity.max_nesting_depth + file.complexity.large_functions.len();
            let score = complexity + churn.get(&path).copied().unwrap_or_default();
            AnalyzeHotspot {
                path: file.path.clone(),
                score,
            }
        })
        .filter(|item| item.score > 0)
        .collect::<Vec<_>>();
    hotspots.sort_by_key(|item| std::cmp::Reverse(item.score));
    hotspots.truncate(DISPLAY_LIMIT);
    hotspots
}

fn project_name(root: &Path) -> String {
    if let Ok(package) = read_json_object(&root.join("package.json")) {
        if let Some(name) = package.get("name").and_then(Value::as_str) {
            return name.to_string();
        }
    }
    if let Ok(text) = fs::read_to_string(root.join("Cargo.toml")) {
        if let Some(name) = manifest_name(&text) {
            return name;
        }
    }
    root.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("project")
        .to_string()
}

fn manifest_name(text: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|line| {
        line.strip_prefix("name")
            .and_then(|line| line.split_once('='))
            .map(|(_, value)| value.trim().trim_matches('"').to_string())
    })
}

fn render_project(report: &AnalyzeReport, details: bool) {
    anstream::println!(
        "Project: {} ({})",
        report.project_name,
        report.root.display()
    );
    anstream::println!("Detected: {}", report.project_types.join(", "));
    if !report.frameworks.is_empty() {
        anstream::println!("Frameworks: {}", report.frameworks.join(", "));
    }
    anstream::println!(
        "Health score: {}/100  Risk: {}",
        report.health_score,
        report.risk_level
    );
    for item in report
        .health_breakdown
        .iter()
        .filter(|item| item.points != 100)
        .take(6)
    {
        anstream::println!("{:+} {}", item.points, item.label);
    }
    anstream::println!(
        "Files: {}  Folders: {}  LOC: {}  Comments: {}  Blanks: {}",
        report.stats.files,
        report.stats.folders,
        report.stats.lines,
        report.stats.comments,
        report.stats.blanks
    );

    let file_types = report
        .file_types
        .iter()
        .filter(|(extension, _)| !extension.is_empty())
        .map(|(extension, count)| vec![extension.clone(), count.to_string()])
        .take(DISPLAY_LIMIT)
        .collect();
    anstream::println!("{}", output::table(&["File type", "Files"], file_types));

    let languages = report
        .languages
        .iter()
        .take(DISPLAY_LIMIT)
        .map(|language| {
            vec![
                language.name.clone(),
                language.files.to_string(),
                language.lines.to_string(),
                output::bytes(language.bytes),
            ]
        })
        .collect();
    anstream::println!(
        "{}",
        output::table(&["Language", "Files", "LOC", "Size"], languages)
    );

    let largest = report
        .largest_files
        .iter()
        .take(DISPLAY_LIMIT)
        .map(|entry| vec![entry.path.display().to_string(), output::bytes(entry.bytes)])
        .collect();
    anstream::println!("{}", output::table(&["Largest files", "Size"], largest));
    print_limit_notice("largest files", report.largest_files.len());

    let deps = report
        .dependencies
        .iter()
        .take(DISPLAY_LIMIT)
        .map(|dep| {
            vec![
                dep.name.clone(),
                dep.version.clone().unwrap_or_else(|| "-".to_string()),
                dep.source.clone(),
            ]
        })
        .collect();
    anstream::println!(
        "{}",
        output::table(&["Dependency", "Version", "Source"], deps)
    );
    print_limit_notice("dependencies", report.dependencies.len());

    if !report.warnings.is_empty() {
        anstream::println!("Warnings:");
        for warning in report.warnings.iter().take(DISPLAY_LIMIT) {
            anstream::println!("  - {warning}");
        }
    }
    if !report.recommendations.is_empty() {
        anstream::println!("Recommendations:");
        for recommendation in report.recommendations.iter().take(5) {
            anstream::println!(
                "  - [{}] {} ({})",
                recommendation.priority,
                recommendation.action,
                recommendation.estimated_effort
            );
        }
    }
    if details {
        render_details(report);
    }
}

fn render_details(report: &AnalyzeReport) {
    anstream::println!();
    anstream::println!("Detailed audit");
    let issues = report
        .issues
        .iter()
        .take(20)
        .map(|issue| {
            vec![
                issue.priority.clone(),
                issue.category.clone(),
                issue.problem.clone(),
                issue.estimated_effort.clone(),
            ]
        })
        .collect();
    anstream::println!(
        "{}",
        output::table(&["Priority", "Category", "Problem", "Effort"], issues)
    );

    if let Some(git) = &report.git {
        anstream::println!(
            "Git: {}  Status: {}  30-day commits: {}  Ahead/behind: {}/{}",
            git.branch,
            if git.clean { "clean" } else { "dirty" },
            git.commits_30_days,
            git.ahead,
            git.behind
        );
        let rows = git
            .most_modified_files
            .iter()
            .map(|item| vec![item.path.display().to_string(), item.score.to_string()])
            .collect();
        anstream::println!("{}", output::table(&["Most modified", "Changes"], rows));
    }

    let imports = report
        .architecture
        .local_imports
        .iter()
        .take(DISPLAY_LIMIT)
        .map(|edge| vec![edge.from.display().to_string(), edge.to.clone()])
        .collect();
    anstream::println!("{}", output::table(&["From", "Imports"], imports));

    if !report.architecture.circular_dependencies.is_empty() {
        anstream::println!("Circular dependencies:");
        for cycle in &report.architecture.circular_dependencies {
            anstream::println!(
                "  - {}",
                cycle
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            );
        }
    }
}

fn render_file(report: &AnalyzeFileReport) {
    anstream::println!("File: {}", report.path.display());
    anstream::println!("Language: {}  Risk: {}", report.language, report.risk_level);
    anstream::println!(
        "Size: {}  LOC: {}  Comments: {}  Blanks: {}",
        output::bytes(report.bytes),
        report.stats.lines,
        report.stats.comments,
        report.stats.blanks
    );
    anstream::println!(
        "Functions: {}  Classes: {}  Interfaces: {}  Enums: {}  Traits: {}  Nesting: {}",
        report.complexity.functions,
        report.complexity.classes,
        report.complexity.interfaces,
        report.complexity.enums,
        report.complexity.traits,
        report.complexity.max_nesting_depth
    );
    let symbols = report
        .symbols
        .iter()
        .take(DISPLAY_LIMIT)
        .map(|symbol| {
            vec![
                symbol.kind.clone(),
                symbol.name.clone(),
                symbol.line.to_string(),
                symbol.lines.to_string(),
            ]
        })
        .collect();
    anstream::println!(
        "{}",
        output::table(&["Kind", "Name", "Line", "Lines"], symbols)
    );
    if !report.imports.is_empty() {
        anstream::println!(
            "Imports: {}",
            report
                .imports
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !report.issues.is_empty() {
        anstream::println!("Issues:");
        for issue in &report.issues {
            anstream::println!("  - [{}] {}", issue.priority, issue.problem);
            anstream::println!("    Fix: {}", issue.suggested_fix);
        }
    }
}

fn print_limit_notice(label: &str, total: usize) {
    if total > DISPLAY_LIMIT {
        anstream::println!("Showing {DISPLAY_LIMIT} of {total} {label}");
    }
}

fn priority_rank(priority: &str) -> usize {
    match priority {
        "High" => 0,
        "Medium" => 1,
        _ => 2,
    }
}

fn add_if_exists(root: &Path, file: &str, types: &mut Vec<String>, label: &str) {
    if root.join(file).exists() {
        types.push(label.to_string());
    }
}

fn parse_simple_manifest(path: &Path, source: &str, sections: &[&str]) -> Result<Vec<Dependency>> {
    let text = fs::read_to_string(path)?;
    let mut active = false;
    let mut deps = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            active = sections.contains(&line);
            continue;
        }
        if active && !line.is_empty() && !line.starts_with('#') {
            if let Some((name, raw_version)) = line.split_once('=') {
                deps.push(Dependency {
                    name: name.trim().to_string(),
                    version: Some(raw_version.trim().trim_matches('"').to_string()),
                    source: source.to_string(),
                });
            }
        }
    }
    Ok(deps)
}

fn parse_requirements(path: &Path) -> Result<Vec<Dependency>> {
    let text = fs::read_to_string(path)?;
    let mut deps = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        let (name, version) = split_dependency_version(line, &["==", ">=", "<=", "~=", ">", "<"]);
        deps.push(Dependency {
            name,
            version,
            source: "requirements.txt".to_string(),
        });
    }
    Ok(deps)
}

fn parse_pyproject(path: &Path) -> Result<Vec<Dependency>> {
    let text = fs::read_to_string(path)?;
    let mut deps = Vec::new();
    let dependency_array = Regex::new(r#"(?s)dependencies\s*=\s*\[(.*?)\]"#)?;
    let quoted_value = Regex::new(r#"["']([^"']+)["']"#)?;
    for array in dependency_array.captures_iter(&text) {
        if let Some(values) = array.get(1) {
            for value in quoted_value.captures_iter(values.as_str()) {
                let (name, version) =
                    split_dependency_version(&value[1], &["==", ">=", "<=", "~=", ">", "<"]);
                deps.push(Dependency {
                    name,
                    version,
                    source: "pyproject.toml".to_string(),
                });
            }
        }
    }
    for line in text.lines().map(str::trim) {
        if let Some((name, version)) = line.split_once('=') {
            let name = name.trim();
            if !name.starts_with('[') && name != "python" && !name.is_empty() && line.contains('"')
            {
                deps.push(Dependency {
                    name: name.to_string(),
                    version: Some(version.trim().trim_matches('"').to_string()),
                    source: "pyproject.toml".to_string(),
                });
            }
        }
    }
    Ok(deps)
}

fn parse_go_mod(path: &Path) -> Result<Vec<Dependency>> {
    let text = fs::read_to_string(path)?;
    let mut deps = Vec::new();
    let mut in_block = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with("require (") {
            in_block = true;
            continue;
        }
        if in_block && line.starts_with(')') {
            in_block = false;
            continue;
        }
        let line = line.split("//").next().unwrap_or("").trim();
        let line = if in_block {
            line
        } else if line.starts_with("require ") && !line.contains('(') {
            line.trim_start_matches("require ").trim()
        } else {
            ""
        };
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if let Some(name) = parts.first() {
            deps.push(Dependency {
                name: (*name).to_string(),
                version: parts.get(1).map(|value| (*value).to_string()),
                source: "go.mod".to_string(),
            });
        }
    }
    Ok(deps)
}

fn parse_composer(path: &Path) -> Result<Vec<Dependency>> {
    let package = read_json_object(path)?;
    let mut deps = Vec::new();
    for section in ["require", "require-dev"] {
        if let Some(map) = package.get(section).and_then(Value::as_object) {
            for (name, version) in map {
                deps.push(Dependency {
                    name: name.clone(),
                    version: version.as_str().map(str::to_string),
                    source: "composer.json".to_string(),
                });
            }
        }
    }
    Ok(deps)
}

fn parse_deno(path: &Path) -> Result<Vec<Dependency>> {
    let package = read_json_object(path)?;
    let mut deps = Vec::new();
    for section in ["imports", "tasks"] {
        if let Some(map) = package.get(section).and_then(Value::as_object) {
            for (name, version) in map {
                deps.push(Dependency {
                    name: name.clone(),
                    version: version.as_str().map(str::to_string),
                    source: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                });
            }
        }
    }
    Ok(deps)
}

fn parse_yaml_dependencies(path: &Path) -> Result<Vec<Dependency>> {
    let text = fs::read_to_string(path)?;
    let mut deps = Vec::new();
    let mut active = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if matches!(trimmed, "dependencies:" | "dev_dependencies:") {
            active = true;
            continue;
        }
        if active && !line.starts_with(' ') && !trimmed.is_empty() {
            active = false;
        }
        if active && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some((name, version)) = trimmed.split_once(':') {
                deps.push(Dependency {
                    name: name.trim().to_string(),
                    version: Some(version.trim().trim_matches('"').to_string())
                        .filter(|v| !v.is_empty()),
                    source: "pubspec.yaml".to_string(),
                });
            }
        }
    }
    Ok(deps)
}

fn parse_gemfile(path: &Path) -> Result<Vec<Dependency>> {
    parse_regex_dependencies(
        path,
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
        r#"gem\s+["']([^"']+)["'](?:\s*,\s*["']([^"']+)["'])?"#,
    )
}

fn parse_pom(path: &Path) -> Result<Vec<Dependency>> {
    let text = fs::read_to_string(path)?;
    let regex = Regex::new(
        r#"(?s)<dependency>.*?<groupId>([^<]+)</groupId>.*?<artifactId>([^<]+)</artifactId>(?:.*?<version>([^<]+)</version>)?.*?</dependency>"#,
    )?;
    Ok(regex
        .captures_iter(&text)
        .map(|capture| Dependency {
            name: format!("{}:{}", &capture[1], &capture[2]),
            version: capture.get(3).map(|value| value.as_str().to_string()),
            source: "pom.xml".to_string(),
        })
        .collect())
}

fn parse_gradle(path: &Path) -> Result<Vec<Dependency>> {
    parse_regex_dependencies(
        path,
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
        r#"(?:implementation|api|compileOnly|runtimeOnly|testImplementation)\(?\s*["']([^:"']+:[^:"']+):?([^"']*)["']"#,
    )
}

fn parse_csproj(path: &Path) -> Result<Vec<Dependency>> {
    parse_regex_dependencies(
        path,
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
        r#"<PackageReference\s+Include="([^"]+)"(?:\s+Version="([^"]+)")?"#,
    )
}

fn parse_terraform(path: &Path) -> Result<Vec<Dependency>> {
    parse_regex_dependencies(
        path,
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
        r#"source\s*=\s*"([^"]+)""#,
    )
}

fn parse_regex_dependencies(path: &Path, source: &str, pattern: &str) -> Result<Vec<Dependency>> {
    let text = fs::read_to_string(path)?;
    let regex = Regex::new(pattern)?;
    Ok(regex
        .captures_iter(&text)
        .map(|capture| Dependency {
            name: capture
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or("")
                .to_string(),
            version: capture.get(2).map(|value| value.as_str().to_string()),
            source: source.to_string(),
        })
        .filter(|dep| !dep.name.is_empty())
        .collect())
}

fn split_dependency_version(value: &str, operators: &[&str]) -> (String, Option<String>) {
    for operator in operators {
        if let Some((name, version)) = value.split_once(operator) {
            return (
                name.trim().to_string(),
                Some(format!("{operator}{}", version.trim())),
            );
        }
    }
    (value.trim().to_string(), None)
}

fn read_json_object(path: &Path) -> Result<Value> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

fn package_dependencies(package: &Value) -> BTreeMap<String, String> {
    let mut deps = BTreeMap::new();
    for section in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(map) = package.get(section).and_then(Value::as_object) {
            for (name, version) in map {
                deps.insert(name.clone(), version.as_str().unwrap_or("*").to_string());
            }
        }
    }
    deps
}

fn has_any(root: &Path, files: &[&str]) -> bool {
    files.iter().any(|file| root.join(file).exists())
}

fn glob_exists(root: &Path, extension: &str) -> bool {
    fswalk::walk(root).is_ok_and(|paths| {
        paths.iter().any(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        })
    })
}

fn file_contains(path: &Path, needle: &str) -> bool {
    fs::read_to_string(path)
        .map(|text| {
            text.to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        })
        .unwrap_or(false)
}

fn dedupe(values: &mut Vec<String>) {
    let mut seen = BTreeMap::new();
    values.retain(|value| seen.insert(value.to_ascii_lowercase(), ()).is_none());
}

fn dedupe_dependencies(deps: &mut Vec<Dependency>) {
    let mut seen = BTreeMap::new();
    deps.retain(|dep| {
        seen.insert(
            (
                dep.source.to_ascii_lowercase(),
                dep.name.to_ascii_lowercase(),
                dep.version.clone().unwrap_or_default(),
            ),
            (),
        )
        .is_none()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn health_score_stays_in_bounds() {
        let items = vec![
            HealthScoreItem {
                label: "Base".to_string(),
                points: 100,
            },
            HealthScoreItem {
                label: "a".to_string(),
                points: -999,
            },
        ];
        assert_eq!(score_from_breakdown(&items), 0);
        assert_eq!(
            score_from_breakdown(&[HealthScoreItem {
                label: "Base".to_string(),
                points: 100
            }]),
            100
        );
    }

    #[test]
    fn target_detection_supports_directory_and_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("main.rs");
        fs::write(&file, "fn main() {}\n").unwrap();
        let config = AnalyzeConfig::default();
        assert!(matches!(
            analyze_target(dir.path(), &config).unwrap(),
            TargetReport::Project(_)
        ));
        assert!(matches!(
            analyze_target(&file, &config).unwrap(),
            TargetReport::File(_)
        ));
    }

    #[test]
    fn config_loads_custom_thresholds() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("devmate.toml");
        fs::write(
            &config_path,
            "ignore = [\"dist\"]\nmax_file_lines = 12\nwarn_console_log = false\nhealth_fail_below = 80\n",
        )
        .unwrap();
        let config = AnalyzeConfig::load(Some(&config_path), 123).unwrap();
        assert_eq!(config.max_file_lines, 12);
        assert!(!config.warn_console_log);
        assert_eq!(config.large_file_bytes, 123);
    }

    #[test]
    fn detects_representative_project_types() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"react":"1","next":"1","typescript":"1","tailwindcss":"1"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        fs::write(dir.path().join("Dockerfile"), "FROM scratch\n").unwrap();
        fs::write(dir.path().join("main.tf"), "resource \"x\" \"y\" {}\n").unwrap();

        let types = detect_project_types(dir.path()).unwrap();
        for expected in [
            "Node",
            "TypeScript",
            "React",
            "Next.js",
            "Tailwind",
            "Docker",
            "Terraform",
        ] {
            assert!(
                types.contains(&expected.to_string()),
                "{expected} missing from {types:?}"
            );
        }
    }

    #[test]
    fn detects_frameworks_from_dependencies() {
        let deps = vec![
            Dependency {
                name: "fastapi".to_string(),
                version: None,
                source: "pyproject.toml".to_string(),
            },
            Dependency {
                name: "@nestjs/core".to_string(),
                version: None,
                source: "package.json".to_string(),
            },
            Dependency {
                name: "axum".to_string(),
                version: None,
                source: "Cargo.toml".to_string(),
            },
            Dependency {
                name: "laravel/framework".to_string(),
                version: None,
                source: "composer.json".to_string(),
            },
        ];
        let frameworks = detect_frameworks(Path::new("."), &deps).unwrap();
        for expected in ["FastAPI", "NestJS", "Axum", "Laravel"] {
            assert!(
                frameworks.contains(&expected.to_string()),
                "{expected} missing from {frameworks:?}"
            );
        }
    }

    #[test]
    fn file_analyzer_extracts_symbols_and_imports() {
        let dir = tempdir().unwrap();
        let ts = dir.path().join("auth.ts");
        fs::write(
            &ts,
            "import {x} from './x';\nexport interface User {}\nexport function login() {\n console.log('x')\n}\n",
        )
        .unwrap();
        let report = analyze_file_target(&ts, &AnalyzeConfig::default()).unwrap();
        assert_eq!(report.language, "TypeScript");
        assert!(report.imports.contains(&"./x".to_string()));
        assert!(report.symbols.iter().any(|symbol| symbol.name == "login"));
    }

    #[test]
    fn rust_analyzer_extracts_symbol_names_and_visibility() {
        let dir = tempdir().unwrap();
        let rs = dir.path().join("lib.rs");
        fs::write(
            &rs,
            "pub fn run() {}\nfn helper() {}\npub(crate) struct Worker;\n",
        )
        .unwrap();

        let report = analyze_file_target(&rs, &AnalyzeConfig::default()).unwrap();
        assert!(report.symbols.iter().any(|symbol| {
            symbol.kind == "function"
                && symbol.name == "run"
                && symbol.visibility.as_deref() == Some("pub")
        }));
        assert!(report
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "helper"));
        assert!(report.symbols.iter().any(|symbol| {
            symbol.kind == "class"
                && symbol.name == "Worker"
                && symbol.visibility.as_deref() == Some("pub(crate)")
        }));
    }

    #[test]
    fn rust_imports_ignore_nested_test_module_imports() {
        let dir = tempdir().unwrap();
        let rs = dir.path().join("lib.rs");
        fs::write(
            &rs,
            "use std::fs;\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n",
        )
        .unwrap();

        let report = analyze_file_target(&rs, &AnalyzeConfig::default()).unwrap();
        assert!(report.imports.contains(&"std::fs".to_string()));
        assert!(!report.imports.contains(&"super::*".to_string()));
    }

    #[test]
    fn todo_count_only_counts_comment_markers() {
        let dir = tempdir().unwrap();
        let rs = dir.path().join("lib.rs");
        fs::write(
            &rs,
            "let todo_count = \"TODO not a marker\";\n// TODO: real work\n/* FIXME: real block */\n",
        )
        .unwrap();

        let report = analyze_file_target(&rs, &AnalyzeConfig::default()).unwrap();
        assert_eq!(report.todo_count, 2);
    }

    #[test]
    fn rust_debug_logging_ignores_renderer_calls_and_strings() {
        let dir = tempdir().unwrap();
        let rs = dir.path().join("lib.rs");
        fs::write(
            &rs,
            "fn main() {\n anstream::println!(\"status\");\n let pattern = \"println!\";\n dbg!(42);\n eprintln!(\"oops\");\n}\n",
        )
        .unwrap();

        let report = analyze_file_target(&rs, &AnalyzeConfig::default()).unwrap();
        assert_eq!(report.logging_count, 2);
    }

    #[test]
    fn rust_function_length_ignores_quoted_braces() {
        let dir = tempdir().unwrap();
        let rs = dir.path().join("lib.rs");
        fs::write(
            &rs,
            "fn braces() {\n let open = \"{\";\n let close = '}';\n}\n",
        )
        .unwrap();

        let report = analyze_file_target(&rs, &AnalyzeConfig::default()).unwrap();
        let symbol = report
            .symbols
            .iter()
            .find(|symbol| symbol.name == "braces")
            .unwrap();
        assert_eq!(symbol.lines, 4);
    }

    #[test]
    fn parses_new_dependency_sources() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("go.mod"),
            "module sample\nrequire (\nexample.com/lib v1.2.3\n)\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\ndependencies = [\"fastapi>=1\"]\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("composer.json"),
            r#"{"require":{"laravel/framework":"^11"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("Gemfile"), "gem \"rails\", \"~> 7\"\n").unwrap();
        fs::write(
            dir.path().join("pubspec.yaml"),
            "dependencies:\n  http: ^1.0.0\n",
        )
        .unwrap();
        fs::write(dir.path().join("mix.exs"), "{:phoenix, \"~> 1.7\"}").unwrap();
        fs::write(
            dir.path().join("app.csproj"),
            r#"<PackageReference Include="Serilog" Version="3.0.0" />"#,
        )
        .unwrap();

        let deps = collect_dependencies(dir.path()).unwrap();
        let names = deps.iter().map(|dep| dep.name.as_str()).collect::<Vec<_>>();
        for expected in [
            "example.com/lib",
            "fastapi",
            "laravel/framework",
            "rails",
            "http",
            "phoenix",
            "Serilog",
        ] {
            assert!(
                names.contains(&expected),
                "{expected} missing from {names:?}"
            );
        }
    }

    #[test]
    fn duplicate_code_detection_finds_repeated_blocks() {
        let dir = tempdir().unwrap();
        let text = (0..10)
            .map(|idx| format!("let value{idx} = {idx};"))
            .collect::<Vec<_>>()
            .join("\n");
        let a = dir.path().join("a.rs");
        let b = dir.path().join("b.rs");
        fs::write(&a, &text).unwrap();
        fs::write(&b, &text).unwrap();
        let config = AnalyzeConfig::default();
        let analyses = vec![
            analyze_file_target(&a, &config).unwrap(),
            analyze_file_target(&b, &config).unwrap(),
        ];
        assert!(!duplicate_code_blocks(&analyses, dir.path()).is_empty());
    }
}
