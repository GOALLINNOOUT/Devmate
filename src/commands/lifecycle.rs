use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Result};
use inquire::Confirm;
use serde::Serialize;

use crate::{
    cli::{UninstallArgs, UpdateArgs},
    output,
};

const PACKAGE_ID: &str = "ADELA.Devmate";
const CRATE_NAME: &str = "devmate";
const REPOSITORY_URL: &str = "https://github.com/GOALLINNOOUT/Devmate";
const RELEASES_URL: &str = "https://github.com/GOALLINNOOUT/Devmate/releases/latest";
const RELEASE_API_URL: &str = "https://api.github.com/repos/GOALLINNOOUT/Devmate/releases/latest";

pub fn update(args: UpdateArgs) -> Result<()> {
    let plan = plan_update()?;
    if args.json {
        output::print_json(&plan)?;
        return Ok(());
    }

    render_plan("DevMate update", &plan);
    if args.dry_run || !plan.runnable {
        return Ok(());
    }
    if !args.yes && !confirm("Run this update command now?")? {
        anstream::println!("Update cancelled.");
        return Ok(());
    }
    run_command(&plan.command, &plan.args)
}

pub fn uninstall(args: UninstallArgs) -> Result<()> {
    let plan = plan_uninstall()?;
    if args.json {
        output::print_json(&plan)?;
        return Ok(());
    }

    render_plan("DevMate uninstall", &plan);
    if args.dry_run || !plan.runnable {
        return Ok(());
    }
    if !args.yes && !confirm("Uninstall DevMate now?")? {
        anstream::println!("Uninstall cancelled.");
        return Ok(());
    }
    run_command(&plan.command, &plan.args)
}

#[derive(Debug, Serialize)]
pub struct LifecyclePlan {
    pub action: String,
    pub install_method: InstallMethod,
    pub executable: PathBuf,
    pub runnable: bool,
    pub command: String,
    pub args: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallMethod {
    Winget,
    Cargo,
    Manual,
}

pub fn plan_update() -> Result<LifecyclePlan> {
    plan("update")
}

pub fn plan_uninstall() -> Result<LifecyclePlan> {
    plan("uninstall")
}

fn plan(action: &str) -> Result<LifecyclePlan> {
    let executable = env::current_exe()?;
    let method = detect_install_method(&executable);
    let (runnable, command, args, notes) = match (action, method) {
        ("update", InstallMethod::Winget) => (
            true,
            "winget".to_string(),
            vec![
                "upgrade".to_string(),
                "--id".to_string(),
                PACKAGE_ID.to_string(),
                "--exact".to_string(),
            ],
            vec!["Uses Windows Package Manager.".to_string()],
        ),
        ("uninstall", InstallMethod::Winget) => (
            true,
            "winget".to_string(),
            vec![
                "uninstall".to_string(),
                "--id".to_string(),
                PACKAGE_ID.to_string(),
                "--exact".to_string(),
            ],
            vec!["Uses Windows Package Manager.".to_string()],
        ),
        ("update", InstallMethod::Cargo) => (
            true,
            "cargo".to_string(),
            vec![
                "install".to_string(),
                "--git".to_string(),
                REPOSITORY_URL.to_string(),
                "--force".to_string(),
            ],
            vec![
                "Uses cargo install from the DevMate GitHub repository.".to_string(),
                "DevMate is not published on crates.io yet.".to_string(),
            ],
        ),
        ("uninstall", InstallMethod::Cargo) => (
            true,
            "cargo".to_string(),
            vec!["uninstall".to_string(), CRATE_NAME.to_string()],
            vec!["Removes the cargo-installed binary from Cargo's bin directory.".to_string()],
        ),
        ("update", InstallMethod::Manual) => (
            true,
            "self-update".to_string(),
            vec![executable.display().to_string()],
            vec![
                "Downloads the latest GitHub Release for this OS.".to_string(),
                "Replaces the current executable after DevMate exits.".to_string(),
                format!("Release page: {RELEASES_URL}"),
            ],
        ),
        ("uninstall", InstallMethod::Manual) => (
            true,
            "self-delete".to_string(),
            vec![executable.display().to_string()],
            vec![
                "Schedules this DevMate executable for deletion after the process exits."
                    .to_string(),
                "If you added this folder to PATH, remove it from PATH after uninstalling."
                    .to_string(),
            ],
        ),
        _ => unreachable!("unsupported lifecycle action"),
    };

    Ok(LifecyclePlan {
        action: action.to_string(),
        install_method: method,
        executable,
        runnable,
        command,
        args,
        notes,
    })
}

pub fn detect_install_method(executable: &Path) -> InstallMethod {
    if cfg!(windows) && winget_package_installed() {
        return InstallMethod::Winget;
    }

    let exe = executable.to_string_lossy().to_ascii_lowercase();
    let cargo_bin = format!(
        "{}{}bin",
        env::var("CARGO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_cargo_home())
            .display(),
        std::path::MAIN_SEPARATOR
    )
    .to_ascii_lowercase();

    if exe.starts_with(&cargo_bin) {
        InstallMethod::Cargo
    } else {
        InstallMethod::Manual
    }
}

fn default_cargo_home() -> PathBuf {
    if cfg!(windows) {
        env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cargo")
    } else {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cargo")
    }
}

fn winget_package_installed() -> bool {
    which::which("winget")
        .ok()
        .and_then(|winget| {
            Command::new(winget)
                .args(["list", "--id", PACKAGE_ID, "--exact"])
                .output()
                .ok()
        })
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .to_ascii_lowercase()
                    .contains(&PACKAGE_ID.to_ascii_lowercase())
        })
        .unwrap_or(false)
}

fn render_plan(title: &str, plan: &LifecyclePlan) {
    anstream::println!("{title}");
    anstream::println!("Install method: {:?}", plan.install_method);
    anstream::println!("Executable: {}", plan.executable.display());
    if plan.runnable {
        anstream::println!("Command: {}", display_command(&plan.command, &plan.args));
    } else {
        anstream::println!("Command: manual action required");
    }
    if !plan.notes.is_empty() {
        anstream::println!();
        for note in &plan.notes {
            anstream::println!("- {note}");
        }
    }
}

fn display_command(command: &str, args: &[String]) -> String {
    std::iter::once(command)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn confirm(prompt: &str) -> Result<bool> {
    Ok(Confirm::new(prompt).with_default(false).prompt()?)
}

fn run_command(command: &str, args: &[String]) -> Result<()> {
    if command.is_empty() {
        bail!("No automatic command is available for this install method");
    }

    if command == "self-delete" {
        schedule_self_delete(args.first().map(String::as_str).unwrap_or_default())?;
        anstream::println!(
            "DevMate uninstall scheduled. Close this terminal if Windows keeps the file locked."
        );
        return Ok(());
    }

    if command == "self-update" {
        schedule_self_update(args.first().map(String::as_str).unwrap_or_default())?;
        anstream::println!(
            "DevMate update started in the background. Run `devmate --version` again in a moment."
        );
        return Ok(());
    }

    let status = Command::new(command).args(args).status()?;
    if !status.success() {
        bail!("Command failed: {}", display_command(command, args));
    }
    Ok(())
}

fn schedule_self_update(path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("Could not determine DevMate executable path");
    }

    if cfg!(windows) {
        let escaped = path.replace('\'', "''");
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &windows_update_script(&escaped),
            ])
            .spawn()?;
    } else {
        Command::new("sh")
            .args(["-c", unix_update_script(), "sh", path])
            .spawn()?;
    }

    Ok(())
}

fn schedule_self_delete(path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("Could not determine DevMate executable path");
    }

    if cfg!(windows) {
        let escaped = path.replace('\'', "''");
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &format!("Start-Sleep -Seconds 2; Remove-Item -LiteralPath '{escaped}' -Force"),
            ])
            .spawn()?;
    } else {
        Command::new("sh")
            .args(["-c", "sleep 2; rm -f -- \"$1\"", "sh", path])
            .spawn()?;
    }

    Ok(())
}

fn windows_update_script(path: &str) -> String {
    format!(
        "$ErrorActionPreference='Stop'; \
         $temp=Join-Path $env:TEMP ('devmate-update-'+[guid]::NewGuid()); \
         New-Item -ItemType Directory -Path $temp | Out-Null; \
         $release=Invoke-RestMethod -Headers @{{'User-Agent'='devmate'}} -Uri '{RELEASE_API_URL}'; \
         $asset=$release.assets | Where-Object {{ $_.name -like '*x86_64-pc-windows-msvc.zip' }} | Select-Object -First 1; \
         Invoke-WebRequest $asset.browser_download_url -OutFile (Join-Path $temp 'devmate.zip'); \
         Expand-Archive (Join-Path $temp 'devmate.zip') -DestinationPath $temp -Force; \
         $exe=Get-ChildItem $temp -Recurse -Filter devmate.exe | Select-Object -First 1; \
         Start-Sleep -Seconds 2; \
         Copy-Item $exe.FullName -Destination '{path}' -Force"
    )
}

fn unix_update_script() -> &'static str {
    r#"set -eu
target="$1"
tmp="${TMPDIR:-/tmp}/devmate-update-$$"
mkdir -p "$tmp"
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) pattern='x86_64-unknown-linux-gnu.tar.gz' ;;
  Darwin-x86_64) pattern='x86_64-apple-darwin.tar.gz' ;;
  Darwin-arm64) pattern='aarch64-apple-darwin.tar.gz' ;;
  *) echo "Unsupported platform for automatic DevMate update" >&2; exit 1 ;;
esac
url="$(curl -fsSL -H 'User-Agent: devmate' 'https://api.github.com/repos/GOALLINNOOUT/Devmate/releases/latest' | grep 'browser_download_url' | grep "$pattern" | head -n 1 | sed 's/.*"browser_download_url": "\(.*\)".*/\1/')"
curl -fL "$url" -o "$tmp/devmate.tar.gz"
tar -xzf "$tmp/devmate.tar.gz" -C "$tmp"
new="$(find "$tmp" -type f -name devmate | head -n 1)"
sleep 2
cp "$new" "$target"
chmod +x "$target"
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_bin_path_detects_cargo_install_without_winget() {
        if cfg!(windows) && winget_package_installed() {
            return;
        }

        let path = default_cargo_home().join("bin").join(if cfg!(windows) {
            "devmate.exe"
        } else {
            "devmate"
        });

        let method = detect_install_method(&path);
        assert_eq!(method, InstallMethod::Cargo);
    }

    #[test]
    fn non_cargo_path_detects_manual_without_winget() {
        if cfg!(windows) && winget_package_installed() {
            return;
        }

        let method = detect_install_method(Path::new("C:/Tools/devmate/devmate.exe"));
        assert_eq!(method, InstallMethod::Manual);
    }
}
