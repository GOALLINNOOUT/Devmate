use std::{collections::BTreeMap, path::Path};

use anyhow::Result;
use chrono::{DateTime, Utc};
use git2::{BranchType, Repository, StatusOptions};

use crate::{
    cli::GitArgs,
    errors::DevMateError,
    models::{GitCommit, GitContributor, GitReport},
    output,
};

pub fn run(args: GitArgs) -> Result<()> {
    let report = summary(&args.path, args.commits)?;
    if args.json {
        output::print_json(&report)?;
    } else {
        render(&report);
    }
    Ok(())
}

pub fn summary(path: &Path, commit_limit: usize) -> Result<GitReport> {
    let repo = Repository::discover(path)
        .map_err(|_| DevMateError::NotGitRepository(path.to_path_buf()))?;
    let root = repo
        .workdir()
        .or_else(|| repo.path().parent())
        .unwrap_or(path)
        .to_path_buf();
    let branch = current_branch(&repo);
    let modified_files = modified_files(&repo)?;
    let (ahead, behind) = ahead_behind(&repo)?;
    let recent_commits = recent_commits(&repo, commit_limit)?;
    let contributors = contributors(&repo)?;
    let branches = branches(&repo)?;

    Ok(GitReport {
        root,
        branch,
        clean: modified_files.is_empty(),
        modified_files,
        ahead,
        behind,
        recent_commits,
        contributors,
        branches,
    })
}

fn current_branch(repo: &Repository) -> String {
    repo.head()
        .ok()
        .and_then(|head| head.shorthand().map(str::to_string))
        .unwrap_or_else(|| "DETACHED".to_string())
}

fn modified_files(repo: &Repository) -> Result<Vec<String>> {
    let mut options = StatusOptions::new();
    options.include_untracked(true).renames_head_to_index(true);
    let statuses = repo.statuses(Some(&mut options))?;
    let mut files = statuses
        .iter()
        .filter_map(|entry| entry.path().map(str::to_string))
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn ahead_behind(repo: &Repository) -> Result<(usize, usize)> {
    let head = repo.head()?;
    let Some(local_oid) = head.target() else {
        return Ok((0, 0));
    };
    let Some(upstream_name) = head
        .shorthand()
        .and_then(|name| repo.find_branch(name, BranchType::Local).ok())
        .and_then(|branch| branch.upstream().ok())
        .and_then(|branch| branch.get().target())
    else {
        return Ok((0, 0));
    };
    Ok(repo.graph_ahead_behind(local_oid, upstream_name)?)
}

fn recent_commits(repo: &Repository, limit: usize) -> Result<Vec<GitCommit>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    let mut commits = Vec::new();
    for oid in revwalk.take(limit) {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let time = DateTime::<Utc>::from_timestamp(commit.time().seconds(), 0)
            .map(|value| value.to_rfc3339())
            .unwrap_or_else(|| "unknown".to_string());
        commits.push(GitCommit {
            id: oid.to_string().chars().take(8).collect(),
            summary: commit.summary().unwrap_or("").to_string(),
            author: commit.author().name().unwrap_or("unknown").to_string(),
            time,
        });
    }
    Ok(commits)
}

fn contributors(repo: &Repository) -> Result<Vec<GitContributor>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    let mut counts = BTreeMap::<String, usize>::new();
    for oid in revwalk.take(500) {
        let commit = repo.find_commit(oid?)?;
        let name = commit.author().name().unwrap_or("unknown").to_string();
        *counts.entry(name).or_insert(0) += 1;
    }
    let mut contributors = counts
        .into_iter()
        .map(|(name, commits)| GitContributor { name, commits })
        .collect::<Vec<_>>();
    contributors.sort_by_key(|item| std::cmp::Reverse(item.commits));
    Ok(contributors)
}

fn branches(repo: &Repository) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for branch in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch?;
        if let Some(name) = branch.name()? {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

fn render(report: &GitReport) {
    anstream::println!("Repository: {}", report.root.display());
    anstream::println!(
        "Branch: {}  Status: {}  Ahead/behind: {}/{}",
        report.branch,
        if report.clean { "clean" } else { "dirty" },
        report.ahead,
        report.behind
    );
    if !report.modified_files.is_empty() {
        anstream::println!("Modified files:");
        for file in &report.modified_files {
            anstream::println!("  {file}");
        }
    }
    let commits = report
        .recent_commits
        .iter()
        .map(|commit| {
            vec![
                commit.id.clone(),
                commit.summary.clone(),
                commit.author.clone(),
                commit.time.clone(),
            ]
        })
        .collect();
    anstream::println!(
        "{}",
        output::table(&["Commit", "Summary", "Author", "Time"], commits)
    );
}
