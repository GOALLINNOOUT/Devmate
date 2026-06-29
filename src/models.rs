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
    pub project_types: Vec<String>,
    pub stats: LineStats,
    pub file_types: BTreeMap<String, usize>,
    pub dependencies: Vec<Dependency>,
    pub largest_files: Vec<FileEntry>,
    pub todo_count: usize,
    pub logging_count: usize,
    pub duplicate_assets: Vec<Vec<PathBuf>>,
    pub large_files: Vec<FileEntry>,
    pub health_score: u8,
    pub warnings: Vec<String>,
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

#[derive(Debug, Serialize)]
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
