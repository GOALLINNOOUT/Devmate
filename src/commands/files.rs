use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::{
    cli::{FilesArgs, FilesCommand},
    fswalk,
    models::{DuplicateGroup, FileEntry, FileSearchResult, FileStatsReport, LineStats},
    output,
};

pub fn run(args: FilesArgs) -> Result<()> {
    match args.command {
        FilesCommand::Search {
            pattern,
            path,
            regex,
            json,
        } => {
            let results = search(&path, &pattern, regex)?;
            if json {
                output::print_json(&results)?;
            } else {
                let rows = results
                    .iter()
                    .map(|item| {
                        vec![
                            item.path.display().to_string(),
                            item.line.to_string(),
                            item.text.clone(),
                        ]
                    })
                    .collect();
                anstream::println!("{}", output::table(&["File", "Line", "Text"], rows));
            }
        }
        FilesCommand::Tree { path, depth, json } => {
            let tree = tree(&path, depth)?;
            if json {
                output::print_json(&tree)?;
            } else {
                for line in tree {
                    anstream::println!("{line}");
                }
            }
        }
        FilesCommand::Stats { path, json } => {
            let report = stats(&path)?;
            if json {
                output::print_json(&report)?;
            } else {
                render_stats(&report);
            }
        }
        FilesCommand::Dupes { path, json } => {
            let groups = duplicates(&path)?;
            if json {
                output::print_json(&groups)?;
            } else if groups.is_empty() {
                anstream::println!("No duplicate files found");
            } else {
                for group in groups {
                    anstream::println!(
                        "{} duplicate bytes across {} files ({})",
                        output::bytes(group.bytes),
                        group.files.len(),
                        group.sha256
                    );
                    for file in group.files {
                        anstream::println!("  {}", file.display());
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn search(root: &Path, pattern: &str, use_regex: bool) -> Result<Vec<FileSearchResult>> {
    let regex = if use_regex {
        Some(Regex::new(pattern).with_context(|| format!("invalid regex: {pattern}"))?)
    } else {
        None
    };
    let mut results = Vec::new();
    for path in files_only(root)? {
        if is_binary_like(&path) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let matched = regex
                .as_ref()
                .map(|expression| expression.is_match(line))
                .unwrap_or_else(|| line.contains(pattern));
            if matched {
                results.push(FileSearchResult {
                    path: fswalk::relative(root, &path),
                    line: index + 1,
                    text: line.trim().to_string(),
                });
            }
        }
    }
    Ok(results)
}

pub fn tree(root: &Path, depth: usize) -> Result<Vec<String>> {
    fswalk::ensure_dir(root)?;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut lines = vec![root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(".")
        .to_string()];
    let mut entries = fswalk::walk(&root)?
        .into_iter()
        .filter(|path| path != &root)
        .filter_map(|path| {
            let relative = fswalk::relative(&root, &path);
            let entry_depth = relative.components().count();
            (entry_depth <= depth).then_some((path, entry_depth))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|(path, _)| path.clone());

    for (path, entry_depth) in entries {
        let indent = "  ".repeat(entry_depth);
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let suffix = if path.is_dir() { "/" } else { "" };
        lines.push(format!("{indent}{name}{suffix}"));
    }

    Ok(lines)
}

pub fn stats(root: &Path) -> Result<FileStatsReport> {
    let mut line_stats = LineStats {
        files: 0,
        folders: 0,
        lines: 0,
        comments: 0,
        blanks: 0,
    };
    let mut by_extension = BTreeMap::new();
    let mut total_bytes = 0;
    let mut largest_files = Vec::new();

    for path in fswalk::walk(root)? {
        if path.is_dir() {
            line_stats.folders += 1;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        line_stats.files += 1;
        *by_extension.entry(fswalk::extension(&path)).or_insert(0) += 1;
        let metadata = fs::metadata(&path)?;
        total_bytes += metadata.len();
        largest_files.push(FileEntry {
            path: fswalk::relative(root, &path),
            bytes: metadata.len(),
        });
        if !is_binary_like(&path) {
            if let Ok(text) = fs::read_to_string(&path) {
                add_line_counts(&text, &mut line_stats);
            }
        }
    }
    largest_files.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
    largest_files.truncate(10);

    Ok(FileStatsReport {
        root: root.to_path_buf(),
        stats: line_stats,
        by_extension,
        total_bytes,
        largest_files,
    })
}

pub fn duplicates(root: &Path) -> Result<Vec<DuplicateGroup>> {
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for path in files_only(root)? {
        let len = fs::metadata(&path)?.len();
        if len > 0 {
            by_size.entry(len).or_default().push(path);
        }
    }

    let mut by_hash: HashMap<String, (u64, Vec<PathBuf>)> = HashMap::new();
    for (size, paths) in by_size.into_iter().filter(|(_, paths)| paths.len() > 1) {
        for path in paths {
            let hash = sha256_file(&path)?;
            by_hash
                .entry(hash)
                .or_insert_with(|| (size, Vec::new()))
                .1
                .push(fswalk::relative(root, &path));
        }
    }

    let mut groups = by_hash
        .into_iter()
        .filter_map(|(sha256, (bytes, files))| {
            if files.len() > 1 {
                Some(DuplicateGroup {
                    sha256,
                    bytes,
                    files,
                })
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| std::cmp::Reverse(group.bytes));
    Ok(groups)
}

pub fn add_line_counts(text: &str, stats: &mut LineStats) {
    for line in text.lines() {
        stats.lines += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            stats.blanks += 1;
        } else if trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
        {
            stats.comments += 1;
        }
    }
}

pub fn files_only(root: &Path) -> Result<Vec<PathBuf>> {
    Ok(fswalk::walk(root)?
        .into_iter()
        .filter(|path| path.is_file())
        .collect())
}

pub fn is_binary_like(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some(
            "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "ico"
                | "pdf"
                | "zip"
                | "gz"
                | "tar"
                | "exe"
                | "dll"
                | "so"
                | "dylib"
        )
    )
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn render_stats(report: &FileStatsReport) {
    anstream::println!("Root: {}", report.root.display());
    anstream::println!(
        "Files: {}  Folders: {}  Lines: {}  Comments: {}  Blanks: {}  Size: {}",
        report.stats.files,
        report.stats.folders,
        report.stats.lines,
        report.stats.comments,
        report.stats.blanks,
        output::bytes(report.total_bytes)
    );
    let rows = report
        .largest_files
        .iter()
        .map(|entry| vec![entry.path.display().to_string(), output::bytes(entry.bytes)])
        .collect();
    anstream::println!("{}", output::table(&["Largest file", "Size"], rows));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_count_classifies_basic_lines() {
        let mut stats = LineStats {
            files: 0,
            folders: 0,
            lines: 0,
            comments: 0,
            blanks: 0,
        };
        add_line_counts("let x = 1;\n\n// comment\n# also comment", &mut stats);
        assert_eq!(stats.lines, 4);
        assert_eq!(stats.blanks, 1);
        assert_eq!(stats.comments, 2);
    }
}
