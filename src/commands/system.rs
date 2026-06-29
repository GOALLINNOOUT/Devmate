use std::{
    io::{self, IsTerminal, Write},
    process::Command,
    thread,
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use sysinfo::{Disks, Networks, System};

use crate::{
    cli::SystemArgs,
    models::{DiskInfo, NetworkInfo, SystemReport},
    output,
};

pub fn run(args: SystemArgs) -> Result<()> {
    if args.json {
        output::print_json(&collect())?;
        return Ok(());
    }

    let _watch_terminal = WatchTerminal::enter(args.watch)?;
    let ticks = if args.watch {
        args.ticks.unwrap_or(usize::MAX)
    } else {
        1
    };
    let interval = Duration::from_secs(args.interval.max(1));
    let mut sampler = SystemSampler::new();
    let mut last_rendered = None;

    if args.watch {
        sampler.wait_for_cpu_sample(interval);
    }

    for index in 0..ticks {
        let report = sampler.collect();
        let rendered = render_to_string(&report, args.watch, args.interval);
        if !args.watch {
            print_rendered(&rendered);
            break;
        }

        if last_rendered.as_deref() != Some(rendered.as_str()) {
            clear_screen()?;
            print_rendered(&rendered);
            last_rendered = Some(rendered);
        }

        if args.watch && index + 1 < ticks {
            thread::sleep(interval);
        }
    }
    Ok(())
}

pub fn collect() -> SystemReport {
    let mut sampler = SystemSampler::new();
    sampler.wait_for_cpu_sample(Duration::from_millis(750));
    sampler.collect()
}

struct SystemSampler {
    system: System,
}

impl SystemSampler {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        system.refresh_cpu();
        Self { system }
    }

    fn wait_for_cpu_sample(&mut self, duration: Duration) {
        thread::sleep(duration);
    }

    fn collect(&mut self) -> SystemReport {
        self.system.refresh_memory();
        self.system.refresh_cpu();
        self.system.refresh_cpu_frequency();

        let disks = Disks::new_with_refreshed_list()
            .iter()
            .map(|disk| DiskInfo {
                name: disk.name().to_string_lossy().to_string(),
                mount: disk.mount_point().to_path_buf(),
                total_bytes: disk.total_space(),
                available_bytes: disk.available_space(),
            })
            .collect();

        let networks = Networks::new_with_refreshed_list()
            .iter()
            .map(|(name, data)| NetworkInfo {
                name: name.clone(),
                received_bytes: data.total_received(),
                transmitted_bytes: data.total_transmitted(),
            })
            .collect();

        SystemReport {
            os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_string()),
            kernel: System::kernel_version(),
            hostname: System::host_name(),
            rust_version: rust_version(),
            cpu_usage_percent: self.system.global_cpu_info().cpu_usage(),
            cpu_frequency_mhz: cpu_frequency_mhz(&self.system),
            cpu_cores: self.system.cpus().len(),
            memory_total_bytes: self.system.total_memory(),
            memory_used_bytes: self.system.used_memory(),
            disks,
            networks,
            battery: battery_status(),
            gpu: gpu_status(),
        }
    }
}

fn cpu_frequency_mhz(system: &System) -> u64 {
    system
        .global_cpu_info()
        .frequency()
        .max(
            system
                .cpus()
                .iter()
                .map(|cpu| cpu.frequency())
                .max()
                .unwrap_or(0),
        )
        .max(windows_cpu_frequency_mhz().unwrap_or(0))
}

fn windows_cpu_frequency_mhz() -> Option<u64> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    Command::new("wmic")
        .args(["cpu", "get", "CurrentClockSpeed", "/value"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| parse_wmic_value(&text, "CurrentClockSpeed"))
        .or_else(|| {
            Command::new("wmic")
                .args(["cpu", "get", "MaxClockSpeed", "/value"])
                .output()
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .and_then(|text| parse_wmic_value(&text, "MaxClockSpeed"))
        })
}

fn parse_wmic_value(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .map(str::trim)
        .filter_map(|line| line.split_once('='))
        .find_map(|(name, value)| {
            (name.trim().eq_ignore_ascii_case(key))
                .then(|| value.trim().parse::<u64>().ok())
                .flatten()
        })
}

fn rust_version() -> Option<String> {
    let output = Command::new("rustc").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_string())
}

fn battery_status() -> Option<String> {
    let manager = battery::Manager::new().ok()?;
    let mut batteries = manager.batteries().ok()?;
    let battery = batteries.next()?.ok()?;
    Some(format!(
        "{:.0}% {:?}",
        battery.state_of_charge().value * 100.0,
        battery.state()
    ))
}

fn gpu_status() -> Option<String> {
    if cfg!(target_os = "windows") {
        Command::new("wmic")
            .args(["path", "win32_VideoController", "get", "name"])
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|text| {
                text.lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty() && *line != "Name")
                    .map(str::to_string)
            })
    } else {
        None
    }
}

fn clear_screen() -> Result<()> {
    execute!(io::stdout(), MoveTo(0, 0), Clear(ClearType::All))?;
    Ok(())
}

struct WatchTerminal {
    active: bool,
}

impl WatchTerminal {
    fn enter(watch: bool) -> Result<Self> {
        let active = watch && io::stdout().is_terminal();
        if active {
            execute!(
                io::stdout(),
                EnterAlternateScreen,
                Hide,
                MoveTo(0, 0),
                Clear(ClearType::All)
            )?;
        }
        Ok(Self { active })
    }
}

impl Drop for WatchTerminal {
    fn drop(&mut self) {
        if self.active {
            let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        }
    }
}

fn print_rendered(rendered: &str) {
    anstream::print!("{rendered}");
    let _ = io::stdout().flush();
}

fn render_to_string(report: &SystemReport, watch: bool, interval: u64) -> String {
    let mut sections = Vec::new();
    sections.push("DevMate System".to_string());
    if watch {
        sections.push(format!(
            "Live view, samples every {}s and redraws only when values change. Press Ctrl+C to stop.",
            interval.max(1)
        ));
    }
    sections.push(String::new());
    sections.push(format!(
        "Host: {}  OS: {}{}",
        report.hostname.as_deref().unwrap_or("unknown"),
        report.os,
        report
            .kernel
            .as_ref()
            .map(|kernel| format!(" ({kernel})"))
            .unwrap_or_default()
    ));
    sections.push(format!(
        "CPU: {:.1}% used  {} MHz  {} cores",
        report.cpu_usage_percent, report.cpu_frequency_mhz, report.cpu_cores
    ));
    sections.push(format!(
        "RAM: {} used / {} total ({:.1}%)",
        output::bytes(report.memory_used_bytes),
        output::bytes(report.memory_total_bytes),
        percent(report.memory_used_bytes, report.memory_total_bytes)
    ));
    sections.push(format!(
        "Rust: {}",
        report.rust_version.as_deref().unwrap_or("Unavailable")
    ));
    sections.push(format!(
        "Battery: {}",
        report.battery.as_deref().unwrap_or("Unavailable")
    ));
    sections.push(format!(
        "GPU: {}",
        report.gpu.as_deref().unwrap_or("Unavailable")
    ));
    sections.push(String::new());

    let disks = report
        .disks
        .iter()
        .map(|disk| {
            let used = disk.total_bytes.saturating_sub(disk.available_bytes);
            vec![
                display_or_dash(&disk.name),
                disk.mount.display().to_string(),
                output::bytes(used),
                output::bytes(disk.available_bytes),
                output::bytes(disk.total_bytes),
                format!("{:.1}%", percent(used, disk.total_bytes)),
            ]
        })
        .collect();
    sections
        .push(output::table(&["Disk", "Mount", "Used", "Free", "Total", "Use"], disks).to_string());

    if !report.networks.is_empty() {
        let networks = report
            .networks
            .iter()
            .map(|network| {
                vec![
                    network.name.clone(),
                    output::bytes(network.received_bytes),
                    output::bytes(network.transmitted_bytes),
                ]
            })
            .collect();
        sections.push(output::table(&["Network", "Received", "Sent"], networks).to_string());
    }

    if watch {
        sections.push(
            "Tip: use `devmate system --json` for one machine-readable snapshot.".to_string(),
        );
    }

    format!("{}\n", sections.join("\n"))
}

fn percent(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64 * 100.0
    }
}

fn display_or_dash(value: &str) -> String {
    if value.trim().is_empty() {
        "-".to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_collects_core_fields() {
        let report = collect();
        assert!(report.cpu_cores > 0);
        assert!(report.memory_total_bytes > 0);
    }

    #[test]
    fn parses_wmic_cpu_frequency_value() {
        assert_eq!(
            parse_wmic_value("CurrentClockSpeed=2400\r\n\r\n", "CurrentClockSpeed"),
            Some(2400)
        );
    }
}
