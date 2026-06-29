use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn json_validate_accepts_valid_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name":"DevMate"}"#).unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args(["json", "validate", file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid JSON"));
}

#[test]
fn env_reports_missing_example_values() {
    let dir = tempdir().unwrap();
    let env = dir.path().join(".env");
    let example = dir.path().join(".env.example");
    let source = dir.path().join("app.js");
    fs::write(&env, "DATABASE_URL=postgres://local\nEMPTY=\n").unwrap();
    fs::write(&example, "DATABASE_URL=\nREDIS_URL=\n").unwrap();
    fs::write(&source, "console.log(process.env.REDIS_URL)\n").unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args([
            "env",
            "inspect",
            dir.path().to_str().unwrap(),
            "--example",
            example.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("REDIS_URL"));
}

#[test]
fn files_search_finds_plain_text() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "fn main() {\nprintln!(\"hi\");\n}\n",
    )
    .unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args(["files", "search", "println", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("main.rs"));
}

#[test]
fn files_search_respects_gitignore() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored.txt\n*.log\n").unwrap();
    fs::write(dir.path().join("kept.txt"), "needle\n").unwrap();
    fs::write(dir.path().join("ignored.txt"), "needle\n").unwrap();
    fs::write(dir.path().join("debug.log"), "needle\n").unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args(["files", "search", "needle", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("kept.txt"))
        .stdout(predicate::str::contains("ignored.txt").not())
        .stdout(predicate::str::contains("debug.log").not());
}

#[test]
fn analyze_detects_rust_project() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"sample\"\nversion=\"0.1.0\"\n[dependencies]\nanyhow=\"1\"\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src").join("main.rs"), "fn main() {}\n").unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args(["analyze", dir.path().to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Rust"));
}

#[test]
fn analyze_defaults_to_current_directory() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"sample\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .current_dir(dir.path())
        .args(["analyze", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Rust"));
}

#[test]
fn analyze_human_output_uses_ascii_tables() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"sample\"\nversion=\"0.1.0\"\n[dependencies]\nanyhow=\"1\"\n",
    )
    .unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args(["analyze", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Detected:"))
        .stdout(predicate::str::contains("+"))
        .stdout(predicate::str::contains("\u{250c}").not());
}

#[test]
fn setup_outputs_first_run_guidance() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"sample\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args(["setup", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("DevMate setup"))
        .stdout(predicate::str::contains("devmate doctor"))
        .stdout(predicate::str::contains("Rust"));
}

#[test]
fn jwt_generate_and_decode_work() {
    let output = Command::cargo_bin("devmate")
        .unwrap()
        .args([
            "jwt", "generate", "--secret", "secret", "--claim", "sub=123",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let token = String::from_utf8(output.stdout).unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args(["jwt", "decode", token.trim()])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"sub\""));
}

#[test]
fn jwt_decode_with_secret_verifies_token() {
    let output = Command::cargo_bin("devmate")
        .unwrap()
        .args([
            "jwt", "generate", "--secret", "secret", "--claim", "sub=123",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let token = String::from_utf8(output.stdout).unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args(["jwt", "decode", "--secret", "secret", token.trim()])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"sub\""));
}

#[test]
fn jwt_decode_with_wrong_secret_fails() {
    let output = Command::cargo_bin("devmate")
        .unwrap()
        .args([
            "jwt", "generate", "--secret", "secret", "--claim", "sub=123",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let token = String::from_utf8(output.stdout).unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args(["jwt", "decode", "--secret", "wrong", token.trim()])
        .assert()
        .failure();
}

#[test]
fn system_json_smoke() {
    Command::cargo_bin("devmate")
        .unwrap()
        .args(["system", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cpu_cores"));
}

#[test]
fn system_watch_exits_after_ticks() {
    Command::cargo_bin("devmate")
        .unwrap()
        .args(["system", "--watch", "--ticks", "2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DevMate System"));
}

#[test]
fn system_watch_json_outputs_one_snapshot() {
    let output = Command::cargo_bin("devmate")
        .unwrap()
        .args(["system", "--watch", "--json", "--ticks", "2"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.matches("\"cpu_cores\"").count(), 1);
}

#[test]
fn doctor_json_smoke() {
    Command::cargo_bin("devmate")
        .unwrap()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Git"));
}

#[test]
fn doctor_json_marks_project_tools() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"sample\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();

    Command::cargo_bin("devmate")
        .unwrap()
        .args(["doctor", dir.path().to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"importance\": \"required\""))
        .stdout(predicate::str::contains("Rust"));
}

#[test]
fn kill_dry_run_json_smoke() {
    Command::cargo_bin("devmate")
        .unwrap()
        .args(["kill", "--dry-run", "--top", "5", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"dry_run\": true"));
}
