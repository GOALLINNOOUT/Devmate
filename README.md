# DevMate

DevMate is a developer diagnostics CLI for project analysis, env files, Git, JWTs, files, system health, installed tools, updates, and safe cleanup.

It is designed to be useful both for humans in a terminal and for scripts through `--json` output.

## What DevMate Does

- `analyze`: audits projects or individual source files with language/framework detection, LOC stats, dependency summaries, health scoring, warnings, recommendations, and optional detailed architecture/git/complexity insight.
- `setup`: first-run onboarding for the current project.
- `doctor`: checks important developer tools for the detected project stack.
- `system`: shows one-shot or live system status.
- `files`: searches, summarizes, trees, and finds duplicate files while respecting gitignore rules.
- `env`: inspects `.env` files and compares expected/used variables.
- `git`: summarizes repository state, commits, branches, and contributors.
- `json`: validates, formats, minifies, and diffs JSON.
- `jwt`: generates, decodes, and verifies HMAC JWTs.
- `kill`: safely previews and terminates selected resource-heavy processes.
- `update` / `uninstall`: manages DevMate based on how it was installed.

## 5-Minute Setup

### Windows

The planned Windows install path is winget:

```powershell
winget install ADELA.Devmate
```

Until the winget package is accepted, install from GitHub Releases after the first release is published:

Download:
https://github.com/GOALLINNOOUT/Devmate/releases/download/v0.1.6/devmate-v0.1.6-x86_64-pc-windows-msvc.zip

```powershell
iwr "https://github.com/GOALLINNOOUT/Devmate/releases/download/v0.1.6/devmate-v0.1.6-x86_64-pc-windows-msvc.zip" -OutFile devmate.zip
Expand-Archive devmate.zip -DestinationPath devmate -Force
```

Then open the extracted folder and run the app:

```powershell
cd .\devmate
.\devmate.exe setup
```

If you extracted with File Explorer, open the folder that contains `devmate.exe`, press and hold shift and right-click inside the folder, choose **Open in Terminal**, then run:

```powershell
.\devmate.exe setup
```

To make `devmate` work from any terminal, add the folder that contains `devmate.exe` to your user `PATH`.

PowerShell:

```powershell
$devmatePath = "$PWD"
[Environment]::SetEnvironmentVariable(
  "Path",
  [Environment]::GetEnvironmentVariable("Path", "User") + ";$devmatePath",
  "User"
)
```

Close and reopen Terminal, then verify:

```powershell
devmate --version
devmate setup
```

You can also add it with the Windows UI:

1. Press **Start** and search **Environment Variables**.
2. Open **Edit environment variables for your account**.
3. Select **Path**, then **Edit**.
4. Click **New** and paste the folder path that contains `devmate.exe`.
5. Click **OK**, close Terminal, and open a new Terminal.

Release downloads are published at:
https://github.com/GOALLINNOOUT/Devmate/releases

Expected release files:

- `devmate-v0.1.6-x86_64-pc-windows-msvc.zip`
- `devmate-v0.1.6-x86_64-unknown-linux-gnu.tar.gz`
- `devmate-v0.1.6-x86_64-apple-darwin.tar.gz`
- `devmate-v0.1.6-aarch64-apple-darwin.tar.gz`

### Linux

Download:
https://github.com/GOALLINNOOUT/Devmate/releases/download/v0.1.6/devmate-v0.1.6-x86_64-unknown-linux-gnu.tar.gz

```bash
curl -L \
  https://github.com/GOALLINNOOUT/Devmate/releases/download/v0.1.6/devmate-v0.1.6-x86_64-unknown-linux-gnu.tar.gz \
  -o devmate-linux.tar.gz
tar -xzf devmate-linux.tar.gz
cd devmate-v0.1.6-x86_64-unknown-linux-gnu
./devmate setup
```

To install it for your user:

```bash
mkdir -p ~/.local/bin
cp devmate ~/.local/bin/devmate
chmod +x ~/.local/bin/devmate
```

Make sure `~/.local/bin` is on your `PATH`, then open a new terminal and verify:

```bash
devmate --version
devmate setup
```

### macOS

Intel Macs:

```bash
curl -L \
  https://github.com/GOALLINNOOUT/Devmate/releases/download/v0.1.6/devmate-v0.1.6-x86_64-apple-darwin.tar.gz \
  -o devmate-macos.tar.gz
```

Apple Silicon Macs:

```bash
curl -L \
  https://github.com/GOALLINNOOUT/Devmate/releases/download/v0.1.6/devmate-v0.1.6-aarch64-apple-darwin.tar.gz \
  -o devmate-macos.tar.gz
```

Then extract and install:

```bash
tar -xzf devmate-macos.tar.gz
cd devmate-v0.1.6-*-apple-darwin
./devmate setup
mkdir -p ~/.local/bin
cp devmate ~/.local/bin/devmate
chmod +x ~/.local/bin/devmate
```

Make sure `~/.local/bin` is on your `PATH`, then open a new terminal and verify:

```bash
devmate --version
devmate setup
```

If macOS blocks the binary because it was downloaded from the internet, open **System Settings > Privacy & Security** and allow DevMate, or remove the quarantine attribute:

```bash
xattr -d com.apple.quarantine ~/.local/bin/devmate
```

### Rust Users

If you already have Rust:

```powershell
cargo install devmate --force
```

### Build From Source

```powershell
git clone https://github.com/GOALLINNOOUT/Devmate.git
cd Devmate
cargo build --release
cargo install --path .
```

## First Run

```powershell
devmate setup
devmate doctor
devmate analyze
devmate system
devmate files stats
```

`setup` is the onboarding command. It detects the current project, lists missing required/recommended/optional tools, and suggests the next commands to try.

## Update

```powershell
devmate update
```

`devmate update` detects the install method and runs the right package-manager command when possible:

- winget install: `winget upgrade --id ADELA.Devmate --exact`
- cargo install: `cargo install devmate --force`
- manual GitHub Release download: downloads the latest release and replaces the current executable after DevMate exits

Preview first:

```powershell
devmate update --dry-run
devmate update --json
```

## Uninstall

```powershell
devmate uninstall
```

`devmate uninstall` asks before removing anything. It uses winget or cargo when DevMate was installed that way. For a manual zip/tar install, it schedules the current `devmate` executable for deletion after DevMate exits and reminds you to remove that folder from `PATH`.

Non-interactive:

```powershell
devmate uninstall --yes
devmate uninstall --dry-run
```

## Quick Start

```powershell
devmate analyze
devmate doctor
devmate system --watch
devmate files stats
devmate env inspect --example .env.example
```

Most diagnostic commands support `--json` for automation:

```powershell
devmate setup --json
devmate analyze --json
devmate doctor --json
devmate system --json
devmate update --json
devmate uninstall --json
```

## Commands

### `analyze`

Analyzes a project directory or a single source file. The target defaults to the current directory.

```powershell
devmate analyze [target] [--details] [--json] [--config <devmate.toml>] [--large-file-bytes <BYTES>]
```

What it reports:

- Project name
- Detected languages, frameworks, and tooling
- Transparent health score and risk level
- File, folder, line, comment, and blank-line counts
- Dependency summaries from supported manifests
- File type and language breakdowns
- Largest files
- TODO/FIXME markers
- Debug logging markers
- Duplicate asset groups
- Large files
- Actionable warnings and recommendations

Detailed project mode adds:

- Architecture/import graph summaries
- Circular dependency detection where import syntax is supported
- Duplicate code block detection
- Large functions and deep nesting
- Git intelligence such as branch, status, 30-day commits, contributors, churn, and hotspots
- Issue objects with problem, impact, suggested fix, affected files, priority, category, and estimated effort

Single-file mode reports:

- Language, size, LOC, comments, and blanks
- Imports and exports
- Functions, classes, interfaces, enums, and traits where detectable
- Complexity, nesting depth, large functions, TODO/FIXME markers, debug logging, risk score, and recommendations

Examples:

```powershell
devmate analyze
devmate analyze C:\path\to\project
devmate analyze src\commands\analyze.rs
devmate analyze --details
devmate analyze --json
devmate analyze --config devmate.toml
devmate analyze --large-file-bytes 1048576
```

Detection includes Rust, Go, Python, Node, TypeScript, JavaScript, React, Next.js, Express, NestJS, Vite, Vue, Nuxt, Angular, Astro, Svelte, Deno, Docker, Kubernetes, Terraform, Java, Kotlin, Scala, C, C++, C#, PHP, Ruby, Swift, Dart/Flutter, Elixir, Erlang, Haskell, Lua, R, Julia, Zig, Nim, SQL, and related frameworks/tools where manifests reveal them.

Optional `devmate.toml`:

```toml
ignore = ["dist", "coverage"]
max_file_lines = 500
max_function_lines = 80
max_nesting_depth = 5
warn_console_log = true
warn_todo = true
health_fail_below = 75
```

### `setup`

Runs a first-use check for the current project and suggests what to do next.

```powershell
devmate setup [path] [--json]
```

Examples:

```powershell
devmate setup
devmate setup C:\path\to\project
devmate setup --json
```

It reports detected project types, missing required/recommended/optional tools, useful next commands, and update commands.

### `update`

Updates DevMate using the install method that is available on this machine.

```powershell
devmate update [--dry-run] [--yes] [--json]
```

Examples:

```powershell
devmate update
devmate update --dry-run
devmate update --json
```

Package-manager installs update through the package manager. Manual GitHub Release installs start a background updater that downloads the latest release for your OS and replaces the current executable after DevMate exits.

### `uninstall`

Uninstalls DevMate.

```powershell
devmate uninstall [--dry-run] [--yes] [--json]
```

Examples:

```powershell
devmate uninstall
devmate uninstall --dry-run
devmate uninstall --yes
```

It asks for confirmation unless `--yes` is supplied. Manual zip/tar installs delete the current executable after DevMate exits; package-manager installs use the package manager.

### `json`

Validates and transforms JSON files.

```powershell
devmate json validate <file>
devmate json format <file> [-o <output>]
devmate json minify <file> [-o <output>]
devmate json diff <left> <right>
```

Examples:

```powershell
devmate json validate package.json
devmate json format data.json --output pretty.json
devmate json minify data.json --output data.min.json
devmate json diff old.json new.json
```

### `env`

Inspects `.env` files and compares them with variables referenced in source files.

```powershell
devmate env inspect [path] [-f <file>] [-e <example>] [--json]
```

Defaults:

- `path`: `.`
- `file`: `.env`

Examples:

```powershell
devmate env inspect
devmate env inspect . --example .env.example
devmate env inspect . --file .env.local --json
```

It reports duplicates, empty values, malformed lines, variables referenced by source code, values missing from `.env`, and differences against `.env.example`.

### `git`

Summarizes a Git repository.

```powershell
devmate git [path] [--json] [--commits <N>]
```

Examples:

```powershell
devmate git
devmate git . --commits 20
devmate git --json
```

It reports branch state, clean/dirty status, modified files, ahead/behind counts, recent commits, contributors, and branches.

### `files`

Searches, visualizes, and audits files while respecting gitignore-style rules and always skipping common heavy folders such as `.git`, `node_modules`, and `target`.

```powershell
devmate files search <pattern> [path] [--regex] [--json]
devmate files tree [path] [-d <depth>] [--json]
devmate files stats [path] [--json]
devmate files dupes [path] [--json]
```

Examples:

```powershell
devmate files search TODO
devmate files search "println!" src --regex
devmate files tree . --depth 4
devmate files stats
devmate files dupes --json
```

### `jwt`

Generates, inspects, and verifies HMAC JSON Web Tokens.

Supported algorithms:

- `hs256`
- `hs384`
- `hs512`

```powershell
devmate jwt generate --secret <secret> [--algorithm hs256] [--claim key=value] [--expires-in <seconds>]
devmate jwt decode <token> [--secret <secret>] [--algorithm hs256]
devmate jwt verify <token> --secret <secret> [--algorithm hs256]
devmate jwt interactive
```

Examples:

```powershell
devmate jwt generate --secret secret --claim sub=123
devmate jwt decode <token>
devmate jwt decode --secret secret <token>
devmate jwt verify <token> --secret secret
devmate jwt generate --secret secret --claim admin=true --claim count=3
```

`decode` without a secret performs unverified inspection. `decode --secret` verifies while still showing the decoded header and claims. `verify` returns verified claims only.

### `system`

Shows system information.

```powershell
devmate system [--json] [--watch] [--interval <seconds>]
```

Examples:

```powershell
devmate system
devmate system --json
devmate system --watch
devmate system --watch --interval 2
```

`--watch` opens a live dashboard view with clearer CPU, RAM, disk, network, battery, GPU, and Rust status. It samples on the selected interval, keeps the CPU sampler warm for more stable readings, and redraws only when visible values change. Press `Ctrl+C` to stop it. If you still see repeated blocks or broken table borders, your shell may be running an old installed binary; check with `where.exe devmate` and reinstall.

### `doctor`

Checks installed developer tools and versions.

```powershell
devmate doctor [path] [--json]
```

Examples:

```powershell
devmate doctor
devmate doctor C:\path\to\project
devmate doctor --json
```

Doctor checks baseline tools such as Git and common editors, then adds project-relevant checks inferred from files such as `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, `Dockerfile`, and lockfiles.

Tools are labeled:

- `required`: needed for the detected project stack
- `recommended`: useful for normal workflows
- `optional`: nice to have, but not a failure if missing

### `kill`

Safely previews resource-heavy processes and asks before terminating anything.

```powershell
devmate kill [--top <N>] [--dry-run] [--yes] [--all-listed] [--name <PATTERN>] [--json]
```

Examples:

```powershell
devmate kill
devmate kill --top 10
devmate kill --dry-run --json
devmate kill --name chrome
devmate kill --all-listed --dry-run
```

Safety behavior:

- Ranks candidates by CPU and RAM pressure
- Filters protected/system processes
- Does not target the current DevMate process or parent shell
- Prompts before killing unless `--yes`, `--all-listed`, or `--dry-run` is used
- Reports per-process success or failure

Use `--dry-run` first if you only want to see what DevMate would target.

## JSON Output

Use `--json` when you want stable machine-readable output for scripts:

```powershell
devmate analyze --json
devmate doctor --json
devmate files stats --json
devmate kill --dry-run --json
```

Human-readable output uses ASCII tables so it works better in older Windows PowerShell code pages.

## Development

### Release Checklist

1. Update `CHANGELOG.md`.
2. Run:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo package --list
cargo publish --dry-run
```

3. Tag and push:

```powershell
git tag v0.1.6
git push origin v0.1.6
```

4. Confirm the GitHub Release includes:

- Windows zip
- SHA256 checksum
- Release notes

5. Submit or update the winget package using `packaging/winget/README.md`.

Format, lint, and test:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Build release:

```powershell
cargo build --release
```

Install the current workspace build:

```powershell
cargo install --path .
```

## Troubleshooting

### `devmate` Still Shows Old Output

PowerShell may be running an installed binary instead of the one you just built.

```powershell
where.exe devmate
```

If it points to `C:\Users\User\.cargo\bin\devmate.exe`, update it:

```powershell
cargo install --path .
```

Or run the local release binary directly:

```powershell
.\target\release\devmate.exe system
```

### GitHub Release Install Fails

Make sure the version in the download URL exists on GitHub Releases:

```powershell
https://github.com/GOALLINNOOUT/Devmate/releases
```

If the zip is downloaded from a browser, Windows may mark it as downloaded from the internet. Right-click the zip or extracted executable, open Properties, and choose Unblock if Windows shows that option.

### Broken Table Characters

Current DevMate source uses ASCII table borders. If you see mojibake or broken box-drawing characters, you are almost certainly running an older installed binary.

### Cargo Warning About `C:\Users\User`

If Cargo prints `warn: could not canonicalize path C:\Users\User`, that is a Cargo/environment warning and not a DevMate runtime failure.

## License

MIT
