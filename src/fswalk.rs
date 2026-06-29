use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::WalkBuilder;

use crate::errors::DevMateError;

pub const IGNORED_DIRS: [&str; 3] = [".git", "node_modules", "target"];

pub fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(DevMateError::MissingPath(path.to_path_buf()).into());
    }
    if !path.is_dir() {
        return Err(DevMateError::ExpectedDirectory(path.to_path_buf()).into());
    }
    Ok(())
}

pub fn ensure_file(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(DevMateError::MissingPath(path.to_path_buf()).into());
    }
    if !path.is_file() {
        return Err(DevMateError::ExpectedFile(path.to_path_buf()).into());
    }
    Ok(())
}

pub fn walk(path: &Path) -> Result<Vec<PathBuf>> {
    ensure_dir(path)?;
    let mut paths = Vec::new();
    for entry in WalkBuilder::new(path)
        .follow_links(false)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false)
        .filter_entry(|entry| !is_ignored(entry.path()))
        .build()
    {
        let entry =
            entry.with_context(|| format!("failed to read path under {}", path.display()))?;
        paths.push(entry.path().to_path_buf());
    }
    Ok(paths)
}

fn is_ignored(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| IGNORED_DIRS.contains(&name))
        .unwrap_or(false)
}

pub fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

pub fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "[none]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn walk_respects_gitignore_without_hiding_dotfiles() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(
            root.join(".gitignore"),
            "ignored.txt\nignored_dir/\n*.log\n",
        )
        .unwrap();
        fs::write(root.join(".env"), "TOKEN=value\n").unwrap();
        fs::write(root.join("kept.txt"), "ok\n").unwrap();
        fs::write(root.join("ignored.txt"), "skip\n").unwrap();
        fs::write(root.join("debug.log"), "skip\n").unwrap();
        fs::create_dir(root.join("ignored_dir")).unwrap();
        fs::write(root.join("ignored_dir").join("file.txt"), "skip\n").unwrap();

        let walked = walk(root)
            .unwrap()
            .into_iter()
            .map(|path| relative(root, &path).display().to_string())
            .collect::<Vec<_>>();

        assert!(walked.iter().any(|path| path == ".env"));
        assert!(walked.iter().any(|path| path == "kept.txt"));
        assert!(!walked.iter().any(|path| path == "ignored.txt"));
        assert!(!walked.iter().any(|path| path == "debug.log"));
        assert!(!walked.iter().any(|path| path.starts_with("ignored_dir")));
    }
}
