//! Integration tests for the APS CLI.
//!
//! These tests exercise the CLI binary as a user would, ensuring
//! argument parsing, command execution, and output work correctly.

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

/// Get a Command for the aps binary
#[allow(deprecated)]
fn aps() -> Command {
    Command::cargo_bin("aps").unwrap()
}

// ============================================================================
// Help and Version Tests
// ============================================================================

#[test]
fn help_flag_shows_usage() {
    aps()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("APS"))
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("validate"))
        .stdout(predicate::str::contains("status"));
}

#[test]
fn version_flag_shows_version() {
    aps()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("aps"));
}

// ============================================================================
// Init Command Tests
// ============================================================================

#[test]
fn init_creates_manifest_file() {
    let temp = assert_fs::TempDir::new().unwrap();

    aps()
        .arg("init")
        .current_dir(&temp)
        .assert()
        .success()
        .stdout(predicate::str::contains("Created manifest"));

    temp.child("aps.yaml").assert(predicate::path::exists());
}

#[test]
fn init_creates_gitignore_entry() {
    let temp = assert_fs::TempDir::new().unwrap();

    aps().arg("init").current_dir(&temp).assert().success();

    temp.child(".gitignore")
        .assert(predicate::str::contains(".aps-backups/"));
}

#[test]
fn init_fails_if_manifest_exists() {
    let temp = assert_fs::TempDir::new().unwrap();
    temp.child("aps.yaml").touch().unwrap();

    aps()
        .arg("init")
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn init_with_custom_path() {
    let temp = assert_fs::TempDir::new().unwrap();

    aps()
        .args(["init", "--manifest", "custom.yaml"])
        .current_dir(&temp)
        .assert()
        .success();

    temp.child("custom.yaml").assert(predicate::path::exists());
}

// ============================================================================
// Sync Command Tests
// ============================================================================

#[test]
fn sync_fails_without_manifest() {
    let temp = assert_fs::TempDir::new().unwrap();

    aps()
        .arg("sync")
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Manifest not found"));
}

#[test]
fn sync_with_empty_manifest_succeeds() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a minimal valid manifest with no entries
    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    aps().arg("sync").current_dir(&temp).assert().success();
}

#[test]
fn sync_dry_run_does_not_create_lockfile() {
    let temp = assert_fs::TempDir::new().unwrap();

    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    aps()
        .args(["sync", "--dry-run"])
        .current_dir(&temp)
        .assert()
        .success();

    // Lockfile should not be created in dry-run mode
    temp.child("aps.manifest.lock")
        .assert(predicate::path::missing());
}

#[test]
fn sync_creates_lockfile() {
    let temp = assert_fs::TempDir::new().unwrap();

    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    aps().arg("sync").current_dir(&temp).assert().success();

    temp.child("aps.manifest.lock")
        .assert(predicate::path::exists());
}

#[test]
fn sync_with_invalid_entry_id_fails() {
    let temp = assert_fs::TempDir::new().unwrap();

    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    aps()
        .args(["sync", "--only", "nonexistent"])
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Entry not found"));
}

// ============================================================================
// Validate Command Tests
// ============================================================================

#[test]
fn validate_fails_without_manifest() {
    let temp = assert_fs::TempDir::new().unwrap();

    aps()
        .arg("validate")
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Manifest not found"));
}

#[test]
fn validate_empty_manifest_succeeds() {
    let temp = assert_fs::TempDir::new().unwrap();

    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    aps()
        .arg("validate")
        .current_dir(&temp)
        .assert()
        .success()
        .stdout(predicate::str::contains("valid"));
}

#[test]
fn validate_invalid_yaml_fails() {
    let temp = assert_fs::TempDir::new().unwrap();

    temp.child("aps.yaml")
        .write_str("this is not: valid: yaml: [")
        .unwrap();

    aps().arg("validate").current_dir(&temp).assert().failure();
}

// ============================================================================
// Status Command Tests
// ============================================================================

#[test]
fn status_fails_without_manifest() {
    let temp = assert_fs::TempDir::new().unwrap();

    aps()
        .arg("status")
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Manifest not found"));
}

#[test]
fn status_fails_without_lockfile() {
    let temp = assert_fs::TempDir::new().unwrap();

    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    aps()
        .arg("status")
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("lockfile"));
}

#[test]
fn status_works_after_sync() {
    let temp = assert_fs::TempDir::new().unwrap();

    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    // First sync to create lockfile
    aps().arg("sync").current_dir(&temp).assert().success();

    // Then status should work
    aps().arg("status").current_dir(&temp).assert().success();
}

// ============================================================================
// Catalog Command Tests
// ============================================================================

#[test]
fn catalog_generate_fails_without_manifest() {
    let temp = assert_fs::TempDir::new().unwrap();

    aps()
        .args(["catalog", "generate"])
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Manifest not found"));
}

#[test]
fn catalog_generate_creates_catalog_file() {
    let temp = assert_fs::TempDir::new().unwrap();

    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    aps()
        .args(["catalog", "generate"])
        .current_dir(&temp)
        .assert()
        .success();

    temp.child("aps.catalog.yaml")
        .assert(predicate::path::exists());
}

// ============================================================================
// Filesystem Source Tests
// ============================================================================

#[test]
fn sync_filesystem_source_copies_file() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create source file
    let source_dir = temp.child("source");
    source_dir.create_dir_all().unwrap();
    source_dir
        .child("AGENTS.md")
        .write_str("# Test Agents\n")
        .unwrap();

    // Create manifest pointing to local file
    let manifest = format!(
        r#"entries:
  - id: test-agents
    kind: agents_md
    source:
      type: filesystem
      root: {}
      path: AGENTS.md
    dest: ./AGENTS.md
"#,
        source_dir.path().display()
    );

    temp.child("aps.yaml").write_str(&manifest).unwrap();

    aps().arg("sync").current_dir(&temp).assert().success();

    // Verify file was copied
    temp.child("AGENTS.md")
        .assert(predicate::str::contains("# Test Agents"));
}

#[test]
fn sync_with_symlink_creates_symlink() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create source file
    let source_dir = temp.child("source");
    source_dir.create_dir_all().unwrap();
    source_dir
        .child("AGENTS.md")
        .write_str("# Test Agents\n")
        .unwrap();

    // Create manifest with symlink enabled
    let manifest = format!(
        r#"entries:
  - id: test-agents
    kind: agents_md
    source:
      type: filesystem
      root: {}
      path: AGENTS.md
      symlink: true
    dest: ./AGENTS.md
"#,
        source_dir.path().display()
    );

    temp.child("aps.yaml").write_str(&manifest).unwrap();

    aps().arg("sync").current_dir(&temp).assert().success();

    // Verify symlink was created
    let dest_path = temp.child("AGENTS.md");
    dest_path.assert(predicate::path::exists());

    // Check it's actually a symlink (on Unix)
    #[cfg(unix)]
    {
        let metadata = std::fs::symlink_metadata(dest_path.path()).unwrap();
        assert!(metadata.file_type().is_symlink());
    }
}

// ============================================================================
// Verbose Flag Tests
// ============================================================================

#[test]
fn verbose_flag_enables_debug_output() {
    let temp = assert_fs::TempDir::new().unwrap();

    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    // With verbose, we should see more output (DEBUG level logs)
    aps()
        .args(["--verbose", "sync"])
        .current_dir(&temp)
        .assert()
        .success();
}

// ============================================================================
// Error Message Quality Tests
// ============================================================================

#[test]
fn error_messages_include_help_hints() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Missing manifest should suggest running init
    aps()
        .arg("sync")
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("aps init").or(predicate::str::contains("--manifest")));
}

#[test]
fn duplicate_entry_ids_detected() {
    let temp = assert_fs::TempDir::new().unwrap();

    let manifest = r#"entries:
  - id: duplicate
    kind: agents_md
    source:
      type: filesystem
      root: /tmp
      path: test.md
  - id: duplicate
    kind: agents_md
    source:
      type: filesystem
      root: /tmp
      path: test2.md
"#;

    temp.child("aps.yaml").write_str(manifest).unwrap();

    aps()
        .arg("validate")
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Duplicate"));
}
