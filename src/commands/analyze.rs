use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;

use crate::{
    cli::AnalyzeArgs,
    commands::files,
    fswalk,
    models::{AnalyzeReport, Dependency, FileEntry, LineStats},
    output,
};

const ASSET_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "pdf"];
const DISPLAY_LIMIT: usize = 10;

pub fn run(args: AnalyzeArgs) -> Result<()> {
    let report = analyze(&args.path, args.large_file_bytes)?;
    if args.json {
        output::print_json(&report)?;
    } else {
        render(&report);
    }
    Ok(())
}

pub fn analyze(root: &Path, large_file_bytes: u64) -> Result<AnalyzeReport> {
    fswalk::ensure_dir(root)?;
    let paths = fswalk::walk(root)?;
    let project_types = detect_project_types(root)?;
    let dependencies = collect_dependencies(root)?;
    let mut stats = LineStats {
        files: 0,
        folders: 0,
        lines: 0,
        comments: 0,
        blanks: 0,
    };
    let mut file_types = BTreeMap::new();
    let mut largest_files = Vec::new();
    let mut large_files = Vec::new();
    let mut todo_count = 0;
    let mut logging_count = 0;

    for path in &paths {
        if path.is_dir() {
            stats.folders += 1;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        stats.files += 1;
        *file_types.entry(fswalk::extension(path)).or_insert(0) += 1;
        let bytes = fs::metadata(path)?.len();
        let entry = FileEntry {
            path: fswalk::relative(root, path),
            bytes,
        };
        largest_files.push(entry.clone());
        if bytes >= large_file_bytes {
            large_files.push(entry);
        }
        if !files::is_binary_like(path) {
            if let Ok(text) = fs::read_to_string(path) {
                files::add_line_counts(&text, &mut stats);
                let lower = text.to_ascii_lowercase();
                todo_count += lower.matches("todo").count() + lower.matches("fixme").count();
                logging_count += debug_logging_count(&text);
            }
        }
    }

    largest_files.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
    largest_files.truncate(10);
    large_files.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
    let duplicate_assets = duplicate_assets(root)?;
    let warnings = warnings(
        &project_types,
        stats.files,
        dependencies.len(),
        todo_count,
        logging_count,
        large_files.len(),
        duplicate_assets.len(),
    );
    let health_score = health_score(&warnings, todo_count, logging_count, large_files.len());

    Ok(AnalyzeReport {
        root: root.to_path_buf(),
        project_types,
        stats,
        file_types,
        dependencies,
        largest_files,
        todo_count,
        logging_count,
        duplicate_assets,
        large_files,
        health_score,
        warnings,
    })
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
        if root.join("tsconfig.json").exists() || deps.contains_key("typescript") {
            types.push("TypeScript".to_string());
        }
        if deps.contains_key("react") {
            types.push("React".to_string());
        }
        if deps.contains_key("next") {
            types.push("Next.js".to_string());
        }
        if deps.contains_key("express") {
            types.push("Express".to_string());
        }
        if deps.contains_key("vite") {
            types.push("Vite".to_string());
        }
        if deps.contains_key("tailwindcss") {
            types.push("Tailwind".to_string());
        }
        if deps.contains_key("prisma") || deps.contains_key("@prisma/client") {
            types.push("Prisma".to_string());
        }
        if (deps.contains_key("mongoose") || deps.contains_key("mongodb"))
            && deps.contains_key("react")
            && deps.contains_key("express")
        {
            types.push("MERN".to_string());
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
    if root.join("deno.json").exists() || root.join("deno.jsonc").exists() {
        types.push("Deno".to_string());
        types.push("TypeScript".to_string());
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
    if has_any(root, &["pom.xml"]) {
        types.push("Java".to_string());
        types.push("Maven".to_string());
        if file_contains(&root.join("pom.xml"), "spring") {
            types.push("Spring".to_string());
        }
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
    if root.join("composer.json").exists() {
        types.push("PHP".to_string());
        types.push("Composer".to_string());
        if file_contains(&root.join("composer.json"), "laravel") {
            types.push("Laravel".to_string());
        }
    }
    if root.join("Gemfile").exists() || root.join("gems.rb").exists() {
        types.push("Ruby".to_string());
        types.push("Bundler".to_string());
        if file_contains(&root.join("Gemfile"), "rails") {
            types.push("Rails".to_string());
        }
    }
    if root.join("pubspec.yaml").exists() {
        types.push("Dart".to_string());
        if file_contains(&root.join("pubspec.yaml"), "flutter") {
            types.push("Flutter".to_string());
        }
    }
    if root.join("mix.exs").exists() {
        types.push("Elixir".to_string());
    }
    if root.join("Package.swift").exists() {
        types.push("Swift".to_string());
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

    for (extension, label) in [
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
        ("yaml", "Kubernetes"),
        ("yml", "Kubernetes"),
    ] {
        if extensions.get(extension).copied().unwrap_or_default() > 0 {
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
            .unwrap_or_default()
            .to_ascii_lowercase();
        (name.ends_with(".yaml") || name.ends_with(".yml"))
            && file_contains(path, "apiversion:")
            && file_contains(path, "kind:")
    }) {
        types.push("Kubernetes".to_string());
    }

    Ok(())
}

fn add_if_exists(root: &Path, file: &str, types: &mut Vec<String>, label: &str) {
    if root.join(file).exists() {
        types.push(label.to_string());
    }
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
    let pyproject = root.join("pyproject.toml");
    if pyproject.exists() {
        deps.extend(parse_pyproject(&pyproject)?);
    }
    let requirements = root.join("requirements.txt");
    if requirements.exists() {
        deps.extend(parse_requirements(&requirements)?);
    }
    let go_mod = root.join("go.mod");
    if go_mod.exists() {
        deps.extend(parse_go_mod(&go_mod)?);
    }
    for (file, parser) in [
        (
            "composer.json",
            parse_composer as fn(&Path) -> Result<Vec<Dependency>>,
        ),
        ("pubspec.yaml", parse_yaml_dependencies),
        ("deno.json", parse_deno),
        ("deno.jsonc", parse_deno),
    ] {
        let path = root.join(file);
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
    let mix = root.join("mix.exs");
    if mix.exists() {
        deps.extend(parse_regex_dependencies(
            &mix,
            "mix.exs",
            r#"\{:\s*([A-Za-z0-9_]+)\s*,\s*"([^"]+)""#,
        )?);
    }
    let swift = root.join("Package.swift");
    if swift.exists() {
        deps.extend(parse_regex_dependencies(
            &swift,
            "Package.swift",
            r#"\.package\([^)]*url:\s*"([^"]+)""#,
        )?);
    }
    let pom = root.join("pom.xml");
    if pom.exists() {
        deps.extend(parse_pom(&pom)?);
    }
    for gradle in ["build.gradle", "build.gradle.kts"] {
        let path = root.join(gradle);
        if path.exists() {
            deps.extend(parse_gradle(&path)?);
        }
    }
    for path in fswalk::walk(root)?.into_iter().filter(|path| {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("csproj"))
    }) {
        deps.extend(parse_csproj(&path)?);
    }
    for path in fswalk::walk(root)?.into_iter().filter(|path| {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("tf"))
    }) {
        deps.extend(parse_terraform(&path)?);
    }
    dedupe_dependencies(&mut deps);
    deps.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(deps)
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
    let mut active_array: Option<&str> = None;
    let mut active_table = false;

    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            active_array = None;
            active_table =
                line == "[tool.poetry.dependencies]" || line == "[project.optional-dependencies]";
            continue;
        }
        if line.starts_with("dependencies") && line.contains('[') {
            if line.contains(']') {
                continue;
            }
            active_array = Some("pyproject.toml");
            continue;
        }
        if active_array.is_some() {
            if line.starts_with(']') {
                active_array = None;
                continue;
            }
            let value = line
                .trim_matches(',')
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            if !value.is_empty() {
                let (name, version) =
                    split_dependency_version(value, &["==", ">=", "<=", "~=", ">", "<"]);
                deps.push(Dependency {
                    name,
                    version,
                    source: "pyproject.toml".to_string(),
                });
            }
            continue;
        }
        if active_table && !line.is_empty() && !line.starts_with('#') {
            if let Some((name, version)) = line.split_once('=') {
                let name = name.trim();
                if name != "python" {
                    deps.push(Dependency {
                        name: name.to_string(),
                        version: Some(version.trim().trim_matches('"').to_string()),
                        source: "pyproject.toml".to_string(),
                    });
                }
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

fn debug_logging_count(text: &str) -> usize {
    [
        "console.log",
        "console.debug",
        "console.warn",
        "println!",
        "dbg!",
        "eprintln!",
        "print(",
        "System.out.println",
        "Console.WriteLine",
        "Debug.Log",
        "NSLog",
        "puts ",
        "p ",
        "logger.debug",
        "log.debug",
        "print_r",
        "var_dump",
        "dd(",
        "dump(",
    ]
    .iter()
    .map(|pattern| text.matches(pattern).count())
    .sum()
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

fn warnings(
    project_types: &[String],
    files: usize,
    dependencies: usize,
    todo_count: usize,
    logging_count: usize,
    large_files: usize,
    duplicate_assets: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if project_types.iter().any(|kind| kind == "Unknown") {
        warnings.push("Project type could not be detected".to_string());
    }
    if files == 0 {
        warnings.push("No files were found".to_string());
    }
    if dependencies == 0 {
        warnings.push("No dependencies were detected".to_string());
    }
    if todo_count > 0 {
        warnings.push(format!("{todo_count} TODO/FIXME markers found"));
    }
    if logging_count > 0 {
        warnings.push(format!("{logging_count} debug logging calls found"));
    }
    if large_files > 0 {
        warnings.push(format!("{large_files} large files found"));
    }
    if duplicate_assets > 0 {
        warnings.push(format!("{duplicate_assets} duplicate asset groups found"));
    }
    warnings
}

fn health_score(
    warnings: &[String],
    todo_count: usize,
    logging_count: usize,
    large_files: usize,
) -> u8 {
    let mut score = 100_i32;
    score -= warnings.len() as i32 * 5;
    score -= todo_count.min(10) as i32;
    score -= logging_count.min(10) as i32;
    score -= (large_files.min(5) as i32) * 3;
    score.clamp(0, 100) as u8
}

fn render(report: &AnalyzeReport) {
    anstream::println!("Project: {}", report.root.display());
    anstream::println!("Detected: {}", report.project_types.join(", "));
    anstream::println!("Health score: {}/100", report.health_score);
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
        for warning in &report.warnings {
            anstream::println!("  - {warning}");
        }
    }
}

fn print_limit_notice(label: &str, total: usize) {
    if total > DISPLAY_LIMIT {
        anstream::println!("Showing {DISPLAY_LIMIT} of {total} {label}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn health_score_stays_in_bounds() {
        let warnings = vec!["a".to_string(); 30];
        assert_eq!(health_score(&warnings, 99, 99, 99), 0);
        assert_eq!(health_score(&[], 0, 0, 0), 100);
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
    fn detects_backend_and_language_manifests() {
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
            "dependencies:\n  flutter:\n",
        )
        .unwrap();
        fs::write(dir.path().join("app.csproj"), r#"<Project></Project>"#).unwrap();
        fs::write(dir.path().join("pom.xml"), "<project><dependency><groupId>org.springframework</groupId><artifactId>spring-core</artifactId></dependency></project>")
            .unwrap();

        let types = detect_project_types(dir.path()).unwrap();

        for expected in [
            "Go", "Python", "PHP", "Laravel", "Ruby", "Rails", "Dart", "Flutter", "C#", ".NET",
            "Java", "Maven", "Spring",
        ] {
            assert!(
                types.contains(&expected.to_string()),
                "{expected} missing from {types:?}"
            );
        }
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
}
