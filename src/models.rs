use std::{collections::BTreeMap, path::PathBuf};

use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct LineStats {
    pub files: usize,
    pub folders: usize,
    pub lines: usize,
    pub comments: usize,
    pub blanks: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct Dependency {
    pub name: String,
    pub version: Option<String>,
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeReport {
    pub root: PathBuf,
    pub target_kind: String,
    pub project_name: String,
    pub project_types: Vec<String>,
    pub stats: LineStats,
    pub file_types: BTreeMap<String, usize>,
    pub languages: Vec<AnalyzeLanguage>,
    pub frameworks: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub largest_files: Vec<FileEntry>,
    pub todo_count: usize,
    pub logging_count: usize,
    pub duplicate_assets: Vec<Vec<PathBuf>>,
    pub large_files: Vec<FileEntry>,
    pub health_score: u8,
    pub risk_level: String,
    pub health_breakdown: Vec<HealthScoreItem>,
    pub warnings: Vec<String>,
    pub issues: Vec<AnalyzeIssue>,
    pub recommendations: Vec<AnalyzeRecommendation>,
    pub git: Option<AnalyzeGit>,
    pub architecture: AnalyzeArchitecture,
    pub hotspots: Vec<AnalyzeHotspot>,
    pub config_used: AnalyzeConfigUsed,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalyzeLanguage {
    pub name: String,
    pub files: usize,
    pub lines: usize,
    pub bytes: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct HealthScoreItem {
    pub label: String,
    pub points: i32,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalyzeIssue {
    pub problem: String,
    pub why_it_matters: String,
    pub suggested_fix: String,
    pub affected_files: Vec<PathBuf>,
    pub priority: String,
    pub category: String,
    pub estimated_effort: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalyzeRecommendation {
    pub priority: String,
    pub action: String,
    pub reason: String,
    pub estimated_effort: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalyzeGit {
    pub branch: String,
    pub clean: bool,
    pub commits_30_days: usize,
    pub most_modified_files: Vec<AnalyzeHotspot>,
    pub contributors: Vec<GitContributor>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalyzeArchitecture {
    pub local_imports: Vec<AnalyzeImportEdge>,
    pub circular_dependencies: Vec<Vec<PathBuf>>,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalyzeImportEdge {
    pub from: PathBuf,
    pub to: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalyzeHotspot {
    pub path: PathBuf,
    pub score: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalyzeConfigUsed {
    pub ignore: Vec<String>,
    pub max_file_lines: usize,
    pub max_function_lines: usize,
    pub max_nesting_depth: usize,
    pub warn_console_log: bool,
    pub warn_todo: bool,
    pub health_fail_below: u8,
}

#[derive(Debug, Serialize)]
#[serde(tag = "target_kind", rename_all = "lowercase")]
pub enum AnalyzeTargetReport {
    Project(Box<AnalyzeReport>),
    File(Box<AnalyzeFileReport>),
}

#[derive(Debug, Serialize)]
pub struct AnalyzeFileReport {
    pub target_kind: String,
    pub path: PathBuf,
    pub language: String,
    pub bytes: u64,
    pub stats: LineStats,
    pub symbols: Vec<AnalyzeSymbol>,
    pub imports: Vec<String>,
    pub exports: Vec<String>,
    pub complexity: AnalyzeComplexity,
    pub todo_count: usize,
    pub logging_count: usize,
    pub issues: Vec<AnalyzeIssue>,
    pub risk_score: u8,
    pub risk_level: String,
    pub recommendations: Vec<AnalyzeRecommendation>,
    pub config_used: AnalyzeConfigUsed,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalyzeSymbol {
    pub kind: String,
    pub name: String,
    pub line: usize,
    pub visibility: Option<String>,
    pub lines: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalyzeComplexity {
    pub functions: usize,
    pub classes: usize,
    pub interfaces: usize,
    pub enums: usize,
    pub traits: usize,
    pub imports: usize,
    pub exports: usize,
    pub max_nesting_depth: usize,
    pub large_functions: Vec<AnalyzeSymbol>,
}

#[derive(Debug, Serialize)]
pub struct EnvReport {
    pub file: PathBuf,
    pub example: Option<PathBuf>,
    pub variables: usize,
    pub duplicates: Vec<String>,
    pub empty: Vec<String>,
    pub malformed: Vec<String>,
    pub referenced_variables: Vec<EnvReference>,
    pub missing_from_env: Vec<String>,
    pub unused_in_env: Vec<String>,
    pub missing_from_example: Vec<String>,
    pub extra_in_env: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct EnvReference {
    pub name: String,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct GitReport {
    pub root: PathBuf,
    pub branch: String,
    pub clean: bool,
    pub modified_files: Vec<String>,
    pub ahead: usize,
    pub behind: usize,
    pub recent_commits: Vec<GitCommit>,
    pub contributors: Vec<GitContributor>,
    pub branches: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct GitCommit {
    pub id: String,
    pub summary: String,
    pub author: String,
    pub time: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct GitContributor {
    pub name: String,
    pub commits: usize,
}

#[derive(Debug, Serialize)]
pub struct FileSearchResult {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct FileStatsReport {
    pub root: PathBuf,
    pub stats: LineStats,
    pub by_extension: BTreeMap<String, usize>,
    pub total_bytes: u64,
    pub largest_files: Vec<FileEntry>,
}

#[derive(Debug, Serialize)]
pub struct DuplicateGroup {
    pub sha256: String,
    pub bytes: u64,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct SystemReport {
    pub os: String,
    pub kernel: Option<String>,
    pub hostname: Option<String>,
    pub rust_version: Option<String>,
    pub cpu_usage_percent: f32,
    pub cpu_frequency_mhz: u64,
    pub cpu_cores: usize,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetworkInfo>,
    pub battery: Option<String>,
    pub gpu: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount: PathBuf,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct NetworkInfo {
    pub name: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub tools: Vec<ToolStatus>,
}

#[derive(Debug, Serialize)]
pub struct ToolStatus {
    pub name: String,
    pub importance: ToolImportance,
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolImportance {
    Required,
    Recommended,
    Optional,
}

#[derive(Debug, Serialize, Clone)]
pub struct KillCandidate {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub score: f64,
}

#[derive(Debug, Serialize)]
pub struct KillReport {
    pub dry_run: bool,
    pub candidates: Vec<KillCandidate>,
    pub results: Vec<KillResult>,
}

#[derive(Debug, Serialize)]
pub struct KillResult {
    pub pid: u32,
    pub name: String,
    pub killed: bool,
}
