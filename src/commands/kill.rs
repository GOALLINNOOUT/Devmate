use std::{collections::HashSet, fmt, thread, time::Duration};

use anyhow::Result;
use inquire::MultiSelect;
use sysinfo::{get_current_pid, Pid, ProcessRefreshKind, RefreshKind, System};

use crate::{
    cli::KillArgs,
    models::{KillCandidate, KillReport, KillResult},
    output,
};

#[derive(Debug, Clone)]
pub struct ProcessRecord {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub is_system_user: bool,
}

pub fn run(args: KillArgs) -> Result<()> {
    let top = args.top.max(1);
    let records = collect_processes()?;
    let protected = protected_pids(&records);
    let candidates = rank_candidates(&records, args.name.as_deref(), top, &protected);

    if args.json && (args.dry_run || (!args.yes && !args.all_listed)) {
        output::print_json(&KillReport {
            dry_run: true,
            candidates,
            results: Vec::new(),
        })?;
        return Ok(());
    }

    if candidates.is_empty() {
        if args.json {
            output::print_json(&KillReport {
                dry_run: args.dry_run,
                candidates,
                results: Vec::new(),
            })?;
        } else {
            anstream::println!("No safe kill candidates found");
        }
        return Ok(());
    }

    let selected = if args.all_listed || args.yes || args.dry_run {
        candidates.clone()
    } else {
        MultiSelect::new("Select processes to kill", candidates.clone())
            .with_help_message("space to select, enter to confirm")
            .prompt()?
    };

    let results = if args.dry_run {
        Vec::new()
    } else {
        kill_selected(&selected)
    };

    let report = KillReport {
        dry_run: args.dry_run,
        candidates: selected,
        results,
    };

    if args.json {
        output::print_json(&report)?;
    } else {
        render_report(&report);
    }

    Ok(())
}

fn collect_processes() -> Result<Vec<ProcessRecord>> {
    let refresh = RefreshKind::new().with_processes(ProcessRefreshKind::everything());
    let mut system = System::new_with_specifics(refresh);
    system.refresh_processes();
    thread::sleep(Duration::from_millis(250));
    system.refresh_processes();

    Ok(system
        .processes()
        .values()
        .map(|process| ProcessRecord {
            pid: process.pid().as_u32(),
            parent_pid: process.parent().map(Pid::as_u32),
            name: process.name().to_string(),
            cpu_percent: process.cpu_usage(),
            memory_bytes: process.memory(),
            is_system_user: process.user_id().is_some_and(|user| {
                let value = user.to_string().to_ascii_lowercase();
                value == "0" || value == "s-1-5-18" || value == "root" || value == "system"
            }),
        })
        .collect())
}

fn protected_pids(records: &[ProcessRecord]) -> HashSet<u32> {
    let mut protected = HashSet::new();
    if let Ok(current) = get_current_pid() {
        let current = current.as_u32();
        protected.insert(current);
        if let Some(parent) = records
            .iter()
            .find(|record| record.pid == current)
            .and_then(|record| record.parent_pid)
        {
            protected.insert(parent);
        }
    }
    protected
}

pub fn rank_candidates(
    records: &[ProcessRecord],
    name_filter: Option<&str>,
    top: usize,
    protected_pids: &HashSet<u32>,
) -> Vec<KillCandidate> {
    let filter = name_filter.map(str::to_ascii_lowercase);
    let max_memory = records
        .iter()
        .map(|record| record.memory_bytes)
        .max()
        .unwrap_or(1)
        .max(1);

    let mut candidates = records
        .iter()
        .filter(|record| !is_protected(record, protected_pids))
        .filter(|record| {
            filter
                .as_ref()
                .map(|filter| record.name.to_ascii_lowercase().contains(filter))
                .unwrap_or(true)
        })
        .map(|record| {
            let memory_score = record.memory_bytes as f64 / max_memory as f64 * 100.0;
            let score = record.cpu_percent as f64 + memory_score;
            KillCandidate {
                pid: record.pid,
                name: record.name.clone(),
                cpu_percent: record.cpu_percent,
                memory_bytes: record.memory_bytes,
                score,
            }
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.memory_bytes.cmp(&left.memory_bytes))
    });
    candidates.truncate(top.max(1));
    candidates
}

pub fn is_protected(record: &ProcessRecord, protected_pids: &HashSet<u32>) -> bool {
    if protected_pids.contains(&record.pid) || record.pid <= 4 || record.is_system_user {
        return true;
    }

    matches!(
        record.name.to_ascii_lowercase().as_str(),
        "system"
            | "idle"
            | "registry"
            | "smss.exe"
            | "csrss.exe"
            | "wininit.exe"
            | "services.exe"
            | "lsass.exe"
            | "svchost.exe"
            | "explorer.exe"
            | "dwm.exe"
            | "sihost.exe"
            | "taskhostw.exe"
            | "securityhealthservice.exe"
            | "systemsettings.exe"
    )
}

fn kill_selected(selected: &[KillCandidate]) -> Vec<KillResult> {
    let system = System::new_all();
    selected
        .iter()
        .map(|candidate| {
            let killed = system
                .process(Pid::from_u32(candidate.pid))
                .map(|process| process.kill())
                .unwrap_or(false);
            KillResult {
                pid: candidate.pid,
                name: candidate.name.clone(),
                killed,
            }
        })
        .collect()
}

fn render_report(report: &KillReport) {
    if report.dry_run {
        anstream::println!("Dry run: no processes were killed");
    }
    let rows = report
        .candidates
        .iter()
        .map(|candidate| {
            vec![
                candidate.pid.to_string(),
                candidate.name.clone(),
                format!("{:.1}", candidate.cpu_percent),
                output::bytes(candidate.memory_bytes),
            ]
        })
        .collect();
    anstream::println!(
        "{}",
        output::table(&["PID", "Process", "CPU %", "RAM"], rows)
    );

    if !report.results.is_empty() {
        let rows = report
            .results
            .iter()
            .map(|result| {
                vec![
                    result.pid.to_string(),
                    result.name.clone(),
                    if result.killed { "killed" } else { "failed" }.to_string(),
                ]
            })
            .collect();
        anstream::println!("{}", output::table(&["PID", "Process", "Result"], rows));
    }
}

impl fmt::Display for KillCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} ({}) CPU {:.1}% RAM {}",
            self.name,
            self.pid,
            self.cpu_percent,
            output::bytes(self.memory_bytes)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pid: u32, name: &str, cpu: f32, memory: u64) -> ProcessRecord {
        ProcessRecord {
            pid,
            parent_pid: None,
            name: name.to_string(),
            cpu_percent: cpu,
            memory_bytes: memory,
            is_system_user: false,
        }
    }

    #[test]
    fn ranking_prefers_high_cpu_and_memory_processes() {
        let records = vec![
            record(10, "small.exe", 1.0, 10),
            record(11, "busy.exe", 80.0, 20),
            record(12, "large.exe", 5.0, 1_000),
        ];

        let ranked = rank_candidates(&records, None, 2, &HashSet::new());

        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].name, "large.exe");
        assert_eq!(ranked[1].name, "busy.exe");
    }

    #[test]
    fn protected_processes_are_filtered() {
        let mut protected = HashSet::new();
        protected.insert(20);
        let records = vec![
            record(4, "System", 99.0, 9_999),
            record(20, "devmate.exe", 50.0, 100),
            record(21, "app.exe", 10.0, 100),
        ];

        let ranked = rank_candidates(&records, None, 5, &protected);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].name, "app.exe");
    }
}
