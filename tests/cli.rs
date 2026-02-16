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
    temp.child("aps.lock.yaml")
        .assert(predicate::path::missing());
}

#[test]
fn sync_creates_lockfile() {
    let temp = assert_fs::TempDir::new().unwrap();

    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    aps().arg("sync").current_dir(&temp).assert().success();

    temp.child("aps.lock.yaml")
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
// Hooks Tests
// ============================================================================

#[test]
fn sync_cursor_hooks_copies_directory_and_sets_exec() {
    let temp = assert_fs::TempDir::new().unwrap();

    let source = temp.child("source");
    source.create_dir_all().unwrap();
    source.child(".cursor").create_dir_all().unwrap();
    source
        .child(".cursor/scripts/hello.sh")
        .write_str("echo hello\n")
        .unwrap();
    source
        .child(".cursor/scripts/nested")
        .create_dir_all()
        .unwrap();
    source
        .child(".cursor/scripts/nested/inner.sh")
        .write_str("echo inner\n")
        .unwrap();
    source
        .child(".cursor/hooks.json")
        .write_str(
            r#"{
  "hooks": {
    "onStart": [
      { "command": "bash .cursor/scripts/hello.sh" },
      { "command": "bash .cursor/scripts/nested/inner.sh" }
    ]
  }
}"#,
        )
        .unwrap();

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: cursor-hooks
    kind: cursor_hooks
    source:
      type: filesystem
      root: {}
      path: .cursor
      symlink: false
    dest: ./.cursor
"#,
        source.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    aps().arg("sync").current_dir(&project).assert().success();

    project
        .child(".cursor/scripts/hello.sh")
        .assert(predicate::path::exists());
    project
        .child(".cursor/scripts/nested/inner.sh")
        .assert(predicate::path::exists());
    // Verify config is also synced to parent dir
    project
        .child(".cursor/hooks.json")
        .assert(predicate::path::exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(project.path().join(".cursor/scripts/hello.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o100, 0);
        let nested_mode = std::fs::metadata(project.path().join(".cursor/scripts/nested/inner.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(nested_mode & 0o100, 0);
    }
}

#[test]
fn validate_cursor_hooks_strict_rejects_missing_config() {
    let temp = assert_fs::TempDir::new().unwrap();

    let source = temp.child("source");
    source.create_dir_all().unwrap();
    source.child(".cursor").create_dir_all().unwrap();
    source
        .child(".cursor/scripts/hello.sh")
        .write_str("echo hello\n")
        .unwrap();

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: cursor-hooks
    kind: cursor_hooks
    source:
      type: filesystem
      root: {}
      path: .cursor
      symlink: false
    dest: ./.cursor
"#,
        source.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    aps()
        .args(["validate", "--strict"])
        .current_dir(&project)
        .assert()
        .failure()
        .stderr(predicate::str::contains("hooks.json"));
}

#[test]
fn validate_cursor_hooks_strict_accepts_valid() {
    let temp = assert_fs::TempDir::new().unwrap();

    let source = temp.child("source");
    source.create_dir_all().unwrap();
    source.child(".cursor").create_dir_all().unwrap();
    source
        .child(".cursor/scripts/hello.sh")
        .write_str("echo hello\n")
        .unwrap();
    source
        .child(".cursor/hooks.json")
        .write_str(
            r#"{
  "hooks": {
    "onStart": [
      { "command": "bash .cursor/scripts/hello.sh" }
    ]
  }
}"#,
        )
        .unwrap();

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: cursor-hooks
    kind: cursor_hooks
    source:
      type: filesystem
      root: {}
      path: .cursor
      symlink: false
    dest: ./.cursor
"#,
        source.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    aps()
        .args(["validate", "--strict"])
        .current_dir(&project)
        .assert()
        .success();
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

#[test]
fn manifest_rejects_claude_hooks_kind() {
    let temp = assert_fs::TempDir::new().unwrap();

    let manifest = r#"entries:
  - id: legacy-claude-hooks
    kind: claude_hooks
    source:
      type: filesystem
      root: /tmp
      path: .claude
"#;

    temp.child("aps.yaml").write_str(manifest).unwrap();

    aps()
        .arg("validate")
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to parse manifest"))
        .stderr(predicate::str::contains("claude_hooks"))
        .stderr(predicate::str::contains("cursor_hooks"));
}

// ============================================================================
// Upgrade Flag Tests (Lock-Respecting Behavior)
// ============================================================================

/// Helper to run a git command in a directory
fn git(dir: &std::path::Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(dir);
    cmd
}

/// Helper to create a local git repo with an initial commit
fn create_git_repo_with_agents_md(dir: &std::path::Path, content: &str) {
    // Initialize git repo with main as default branch
    git(dir)
        .args(["init", "--initial-branch=main"])
        .output()
        .expect("Failed to init git repo");

    // Configure git user for commits
    git(dir)
        .args(["config", "user.email", "test@test.com"])
        .output()
        .expect("Failed to configure git email");
    git(dir)
        .args(["config", "user.name", "Test User"])
        .output()
        .expect("Failed to configure git name");

    // Disable GPG signing for test commits
    git(dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .expect("Failed to disable gpg signing");

    // Create AGENTS.md
    std::fs::write(dir.join("AGENTS.md"), content).expect("Failed to write AGENTS.md");

    // Add and commit
    git(dir)
        .args(["add", "AGENTS.md"])
        .output()
        .expect("Failed to git add");
    git(dir)
        .args(["commit", "--no-gpg-sign", "-m", "Initial commit"])
        .output()
        .expect("Failed to git commit");
}

/// Helper to update AGENTS.md and create a new commit
fn update_agents_md_in_repo(dir: &std::path::Path, new_content: &str) {
    std::fs::write(dir.join("AGENTS.md"), new_content).expect("Failed to write AGENTS.md");

    git(dir)
        .args(["add", "AGENTS.md"])
        .output()
        .expect("Failed to git add");
    git(dir)
        .args(["commit", "--no-gpg-sign", "-m", "Update AGENTS.md"])
        .output()
        .expect("Failed to git commit");
}

#[test]
fn sync_without_upgrade_respects_locked_commit() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a "remote" git repo (local directory acting as remote)
    let source_repo = temp.child("source-repo");
    source_repo.create_dir_all().unwrap();
    create_git_repo_with_agents_md(source_repo.path(), "# Version 1\nOriginal content\n");

    // Create project directory with manifest pointing to local git repo
    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: test-agents
    kind: agents_md
    source:
      type: git
      repo: {}
      ref: main
      shallow: false
      path: AGENTS.md
    dest: ./AGENTS.md
"#,
        source_repo.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    // First sync - should install version 1
    aps().arg("sync").current_dir(&project).assert().success();

    // Verify version 1 is installed
    project
        .child("AGENTS.md")
        .assert(predicate::str::contains("Version 1"));

    // Update the source repo with new content (version 2)
    update_agents_md_in_repo(source_repo.path(), "# Version 2\nUpdated content\n");

    // Sync WITHOUT --upgrade - should NOT update (respects locked commit)
    aps().arg("sync").current_dir(&project).assert().success();

    // Verify still has version 1 (locked version respected)
    project
        .child("AGENTS.md")
        .assert(predicate::str::contains("Version 1"));
    project
        .child("AGENTS.md")
        .assert(predicate::str::contains("Version 2").not());
}

#[test]
fn sync_with_upgrade_fetches_latest_version() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a "remote" git repo
    let source_repo = temp.child("source-repo");
    source_repo.create_dir_all().unwrap();
    create_git_repo_with_agents_md(source_repo.path(), "# Version 1\nOriginal content\n");

    // Create project directory with manifest
    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: test-agents
    kind: agents_md
    source:
      type: git
      repo: {}
      ref: main
      shallow: false
      path: AGENTS.md
    dest: ./AGENTS.md
"#,
        source_repo.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    // First sync - install version 1
    aps().arg("sync").current_dir(&project).assert().success();

    // Verify version 1
    project
        .child("AGENTS.md")
        .assert(predicate::str::contains("Version 1"));

    // Update the source repo
    update_agents_md_in_repo(source_repo.path(), "# Version 2\nUpdated content\n");

    // Sync WITH --upgrade - should update to version 2
    aps()
        .args(["sync", "--upgrade", "--yes"])
        .current_dir(&project)
        .assert()
        .success();

    // Verify version 2 is now installed
    project
        .child("AGENTS.md")
        .assert(predicate::str::contains("Version 2"));
}

#[test]
fn sync_shows_upgrade_available_status() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a "remote" git repo
    let source_repo = temp.child("source-repo");
    source_repo.create_dir_all().unwrap();
    create_git_repo_with_agents_md(source_repo.path(), "# Version 1\n");

    // Create project directory with manifest
    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: test-agents
    kind: agents_md
    source:
      type: git
      repo: {}
      ref: main
      shallow: false
      path: AGENTS.md
    dest: ./AGENTS.md
"#,
        source_repo.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    // First sync
    aps().arg("sync").current_dir(&project).assert().success();

    // Update the source repo
    update_agents_md_in_repo(source_repo.path(), "# Version 2\n");

    // Sync without upgrade - should show "upgrade available" message
    aps()
        .arg("sync")
        .current_dir(&project)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("upgrade available")
                .or(predicate::str::contains("upgrades available")),
        );
}

// ============================================================================
// Composite Agents MD Tests (Live Git Sources)
// ============================================================================

#[test]
#[ignore = "requires network access; run with --ignored or set APS_TEST_NETWORK=1"]
fn sync_composite_agents_md_from_git_sources() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create manifest with composite_agents_md using real git sources
    let manifest = r#"entries:
  - id: composite-test
    kind: composite_agents_md
    sources:
      - type: git
        repo: https://github.com/westonplatter/agentically.git
        ref: main
        path: agents-md-partials/AGENTS.docker.md
      - type: git
        repo: https://github.com/westonplatter/agentically.git
        ref: main
        path: agents-md-partials/AGENTS.pandas.md
    dest: ./AGENTS.md
"#;

    temp.child("aps.yaml").write_str(manifest).unwrap();

    // Sync should succeed
    aps().arg("sync").current_dir(&temp).assert().success();

    // Verify the composite file was created
    let agents_md = temp.child("AGENTS.md");
    agents_md.assert(predicate::path::exists());

    // Verify content from both sources is present
    agents_md.assert(predicate::str::contains(
        "auto-generated by aps (https://github.com/westonplatter/agentic-prompt-sync)",
    ));
    // Docker content should be present (check for something unique to that file)
    agents_md.assert(predicate::str::contains("docker").or(predicate::str::contains("Docker")));
    // Pandas content should be present
    agents_md.assert(predicate::str::contains("pandas").or(predicate::str::contains("Pandas")));

    // Verify lockfile was created with proper structure
    let lockfile = temp.child("aps.lock.yaml");
    lockfile.assert(predicate::path::exists());

    // Verify the lockfile has composite structure (not a string)
    lockfile.assert(predicate::str::contains("composite:"));
    lockfile.assert(predicate::str::contains(
        "- https://github.com/westonplatter/agentically.git:agents-md-partials/AGENTS.docker.md",
    ));
    lockfile.assert(predicate::str::contains(
        "- https://github.com/westonplatter/agentically.git:agents-md-partials/AGENTS.pandas.md",
    ));
}

#[test]
#[ignore = "requires network access; run with --ignored or set APS_TEST_NETWORK=1"]
fn sync_composite_agents_md_lockfile_is_valid_yaml() {
    let temp = assert_fs::TempDir::new().unwrap();

    let manifest = r#"entries:
  - id: composite-test
    kind: composite_agents_md
    sources:
      - type: git
        repo: https://github.com/westonplatter/agentically.git
        ref: main
        path: agents-md-partials/AGENTS.docker.md
      - type: git
        repo: https://github.com/westonplatter/agentically.git
        ref: main
        path: agents-md-partials/AGENTS.pandas.md
    dest: ./AGENTS.md
"#;

    temp.child("aps.yaml").write_str(manifest).unwrap();

    aps().arg("sync").current_dir(&temp).assert().success();

    // Read the lockfile and verify it can be re-parsed by aps status
    aps().arg("status").current_dir(&temp).assert().success();

    // Verify status output shows composite source correctly
    aps()
        .arg("status")
        .current_dir(&temp)
        .assert()
        .success()
        .stdout(predicate::str::contains("composite"))
        .stdout(predicate::str::contains("AGENTS.docker.md"))
        .stdout(predicate::str::contains("AGENTS.pandas.md"));
}

#[test]
#[ignore = "requires network access; run with --ignored or set APS_TEST_NETWORK=1"]
fn sync_composite_agents_md_respects_locked_version() {
    let temp = assert_fs::TempDir::new().unwrap();

    let manifest = r#"entries:
  - id: composite-test
    kind: composite_agents_md
    sources:
      - type: git
        repo: https://github.com/westonplatter/agentically.git
        ref: main
        path: agents-md-partials/AGENTS.docker.md
      - type: git
        repo: https://github.com/westonplatter/agentically.git
        ref: main
        path: agents-md-partials/AGENTS.pandas.md
    dest: ./AGENTS.md
"#;

    temp.child("aps.yaml").write_str(manifest).unwrap();

    // First sync
    aps().arg("sync").current_dir(&temp).assert().success();

    // Get the checksum from first sync
    let lockfile_content = std::fs::read_to_string(temp.child("aps.lock.yaml").path()).unwrap();
    let first_checksum = lockfile_content
        .lines()
        .find(|l| l.contains("checksum:"))
        .unwrap()
        .to_string();

    // Second sync should show [current] (no changes)
    aps()
        .arg("sync")
        .current_dir(&temp)
        .assert()
        .success()
        .stdout(predicate::str::contains("[current]"));

    // Verify checksum hasn't changed
    let lockfile_content_after =
        std::fs::read_to_string(temp.child("aps.lock.yaml").path()).unwrap();
    let second_checksum = lockfile_content_after
        .lines()
        .find(|l| l.contains("checksum:"))
        .unwrap()
        .to_string();

    assert_eq!(first_checksum, second_checksum);
}

#[test]
fn lockfile_migration_from_legacy_name() {
    // Test that the legacy lockfile name (aps.manifest.lock) is automatically
    // migrated to the new name (aps.lock.yaml) when running sync
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a manifest file
    temp.child("aps.yaml").write_str("entries: []\n").unwrap();

    // Create a legacy lockfile manually
    let legacy_lockfile_content = r#"version: 1
entries: {}
"#;
    temp.child("aps.manifest.lock")
        .write_str(legacy_lockfile_content)
        .unwrap();

    // Verify legacy lockfile exists
    temp.child("aps.manifest.lock")
        .assert(predicate::path::exists());

    // New lockfile should not exist yet
    temp.child("aps.lock.yaml")
        .assert(predicate::path::missing());

    // Run sync - this should load the legacy lockfile and save as new name
    aps().arg("sync").current_dir(&temp).assert().success();

    // After sync, new lockfile should exist
    temp.child("aps.lock.yaml")
        .assert(predicate::path::exists());

    // Legacy lockfile should be removed during migration
    temp.child("aps.manifest.lock")
        .assert(predicate::path::missing());
}

// ============================================================================
// Add Command Tests
// ============================================================================

#[test]
fn add_creates_manifest_entry_with_no_sync() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Use --no-sync to only test manifest creation (not network call)
    aps()
        .args([
            "add",
            "https://github.com/hashicorp/agent-skills/blob/main/terraform/module-generation/skills/refactor-module",
            "--no-sync",
        ])
        .current_dir(&temp)
        .assert()
        .success()
        .stdout(predicate::str::contains("Added entry 'refactor-module'"))
        .stdout(predicate::str::contains("Creating new manifest"));

    // Verify manifest was created
    let manifest = temp.child("aps.yaml");
    manifest.assert(predicate::path::exists());

    // Verify manifest content
    manifest.assert(predicate::str::contains("id: refactor-module"));
    manifest.assert(predicate::str::contains("kind: agent_skill"));
    manifest.assert(predicate::str::contains(
        "repo: https://github.com/hashicorp/agent-skills.git",
    ));
    manifest.assert(predicate::str::contains("ref: main"));
    manifest.assert(predicate::str::contains(
        "path: terraform/module-generation/skills/refactor-module",
    ));
}

#[test]
fn add_parses_skill_md_url_correctly() {
    let temp = assert_fs::TempDir::new().unwrap();

    // URL ending in SKILL.md should have the SKILL.md stripped from path
    aps()
        .args([
            "add",
            "https://github.com/hashicorp/agent-skills/blob/main/terraform/module-generation/skills/refactor-module/SKILL.md",
            "--no-sync",
        ])
        .current_dir(&temp)
        .assert()
        .success();

    // Verify the path doesn't include SKILL.md
    let manifest = temp.child("aps.yaml");
    manifest.assert(predicate::str::contains(
        "path: terraform/module-generation/skills/refactor-module",
    ));
    // Should NOT contain SKILL.md in the path
    manifest.assert(
        predicate::str::contains(
            "path: terraform/module-generation/skills/refactor-module/SKILL.md",
        )
        .not(),
    );
}

#[test]
fn add_with_custom_id() {
    let temp = assert_fs::TempDir::new().unwrap();

    aps()
        .args([
            "add",
            "https://github.com/owner/repo/blob/main/path/to/skill",
            "--id",
            "my-custom-skill",
            "--no-sync",
        ])
        .current_dir(&temp)
        .assert()
        .success()
        .stdout(predicate::str::contains("Added entry 'my-custom-skill'"));

    // Verify manifest has custom ID
    let manifest = temp.child("aps.yaml");
    manifest.assert(predicate::str::contains("id: my-custom-skill"));
    manifest.assert(predicate::str::contains(
        "dest: .claude/skills/my-custom-skill/",
    ));
}

#[test]
fn add_to_existing_manifest() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create existing manifest with an entry
    let existing_manifest = r#"entries:
  - id: existing-skill
    kind: agent_skill
    source:
      type: git
      repo: https://github.com/other/repo.git
      ref: main
      path: skills/existing
    dest: ./.claude/skills/existing-skill/
"#;
    temp.child("aps.yaml").write_str(existing_manifest).unwrap();

    // Add a new skill
    aps()
        .args([
            "add",
            "https://github.com/owner/repo/blob/main/path/to/new-skill",
            "--no-sync",
        ])
        .current_dir(&temp)
        .assert()
        .success()
        .stdout(predicate::str::contains("Added entry 'new-skill'"));

    // Verify both entries exist
    let manifest = temp.child("aps.yaml");
    manifest.assert(predicate::str::contains("id: existing-skill"));
    manifest.assert(predicate::str::contains("id: new-skill"));
}

#[test]
fn add_duplicate_id_fails() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create existing manifest with an entry
    let existing_manifest = r#"entries:
  - id: duplicate-skill
    kind: agent_skill
    source:
      type: git
      repo: https://github.com/other/repo.git
      ref: main
      path: skills/existing
    dest: ./.claude/skills/duplicate-skill/
"#;
    temp.child("aps.yaml").write_str(existing_manifest).unwrap();

    // Try to add a skill with the same ID (derived from folder name)
    aps()
        .args([
            "add",
            "https://github.com/owner/repo/blob/main/path/to/duplicate-skill",
            "--no-sync",
        ])
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Duplicate"));
}

#[test]
fn add_invalid_github_url_fails() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Non-GitHub URL
    aps()
        .args([
            "add",
            "https://gitlab.com/owner/repo/blob/main/path",
            "--no-sync",
        ])
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("github.com"));
}

#[test]
fn add_invalid_url_format_fails() {
    let temp = assert_fs::TempDir::new().unwrap();

    // URL without blob/tree
    aps()
        .args([
            "add",
            "https://github.com/owner/repo/commits/main/path",
            "--no-sync",
        ])
        .current_dir(&temp)
        .assert()
        .failure()
        .stderr(predicate::str::contains("blob").or(predicate::str::contains("tree")));
}

#[test]
fn add_with_tree_url() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Tree URLs (directory view) should work too
    aps()
        .args([
            "add",
            "https://github.com/owner/repo/tree/main/path/to/skill",
            "--no-sync",
        ])
        .current_dir(&temp)
        .assert()
        .success();

    let manifest = temp.child("aps.yaml");
    manifest.assert(predicate::str::contains("path: path/to/skill"));
}

#[test]
fn add_with_different_ref() {
    let temp = assert_fs::TempDir::new().unwrap();

    // URL with a different branch/tag
    aps()
        .args([
            "add",
            "https://github.com/owner/repo/blob/v1.2.3/path/to/skill",
            "--no-sync",
        ])
        .current_dir(&temp)
        .assert()
        .success();

    let manifest = temp.child("aps.yaml");
    manifest.assert(predicate::str::contains("ref: v1.2.3"));
}

#[test]
fn add_help_shows_usage() {
    aps()
        .args(["add", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("GitHub URL"))
        .stdout(predicate::str::contains("--id"))
        .stdout(predicate::str::contains("--kind"))
        .stdout(predicate::str::contains("--no-sync"))
        .stdout(predicate::str::contains("--all"));
}

// ============================================================================
// Repo-Level Discovery Tests
// ============================================================================

/// Helper to create a local git repo with multiple skills
fn create_skills_repo(dir: &std::path::Path) {
    // Initialize git repo with main as default branch
    git(dir)
        .args(["init", "--initial-branch=main"])
        .output()
        .expect("Failed to init git repo");

    // Configure git user for commits
    git(dir)
        .args(["config", "user.email", "test@test.com"])
        .output()
        .expect("Failed to configure git email");
    git(dir)
        .args(["config", "user.name", "Test User"])
        .output()
        .expect("Failed to configure git name");
    git(dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .expect("Failed to disable gpg signing");

    // Create skill directories with SKILL.md
    std::fs::create_dir_all(dir.join("skills/refactor")).unwrap();
    std::fs::write(
        dir.join("skills/refactor/SKILL.md"),
        "# Refactor\n\nRefactors code automatically.\n",
    )
    .unwrap();

    std::fs::create_dir_all(dir.join("skills/test-gen")).unwrap();
    std::fs::write(
        dir.join("skills/test-gen/SKILL.md"),
        "# Test Generation\n\nGenerates unit tests.\n",
    )
    .unwrap();

    std::fs::create_dir_all(dir.join("skills/lint-fix")).unwrap();
    std::fs::write(
        dir.join("skills/lint-fix/SKILL.md"),
        "# Lint Fix\n\nFixes linting issues.\n",
    )
    .unwrap();

    // Create a non-skill directory (no SKILL.md)
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(dir.join("docs/README.md"), "# Documentation\n").unwrap();

    // Add and commit all files
    git(dir)
        .args(["add", "."])
        .output()
        .expect("Failed to git add");
    git(dir)
        .args(["commit", "--no-gpg-sign", "-m", "Add skills"])
        .output()
        .expect("Failed to git commit");
}

#[test]
fn add_repo_level_url_non_github_fails() {
    let temp = assert_fs::TempDir::new().unwrap();
    let project = temp.child("project");
    project.create_dir_all().unwrap();

    // Non-GitHub repo-level URL should fail
    aps()
        .args(["add", "https://gitlab.com/owner/repo", "--all", "--no-sync"])
        .current_dir(&project)
        .assert()
        .failure()
        .stderr(predicate::str::contains("github.com"));
}

#[test]
fn add_repo_url_with_all_discovers_and_adds_skills() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a local skills repo (already a git repo via create_skills_repo)
    let source_repo = temp.child("skills-repo");
    source_repo.create_dir_all().unwrap();
    create_skills_repo(source_repo.path());

    // Create project directory
    let project = temp.child("project");
    project.create_dir_all().unwrap();

    // Use the local git repo path so the discovery flow runs without network access
    let repo_path = source_repo.path().to_str().unwrap();

    aps()
        .args(["add", repo_path, "--all", "--no-sync"])
        .current_dir(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains("Searching for skills"));
}

#[test]
fn add_repo_url_no_skills_found_errors() {
    let temp = assert_fs::TempDir::new().unwrap();
    let project = temp.child("project");
    project.create_dir_all().unwrap();

    // Use a repo directory that definitely has no SKILL.md files
    aps()
        .args([
            "add",
            "https://github.com/westonplatter/agentically/tree/main/agents-md-partials",
            "--all",
            "--no-sync",
        ])
        .current_dir(&project)
        .assert()
        .failure()
        .stderr(predicate::str::contains("No skills found"));
}

#[test]
fn sync_local_git_repo_installs_all_skills() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a local skills repo
    let source_repo = temp.child("skills-repo");
    source_repo.create_dir_all().unwrap();
    create_skills_repo(source_repo.path());

    // Create project directory
    let project = temp.child("project");
    project.create_dir_all().unwrap();

    // Manually create a manifest referencing skills from a local git repo.
    // This tests that `aps sync` can install skills from a local git source.
    let manifest = format!(
        r#"entries:
  - id: refactor
    kind: agent_skill
    source:
      type: git
      repo: {}
      ref: main
      shallow: false
      path: skills/refactor
    dest: ./.claude/skills/refactor/
  - id: test-gen
    kind: agent_skill
    source:
      type: git
      repo: {}
      ref: main
      shallow: false
      path: skills/test-gen
    dest: ./.claude/skills/test-gen/
  - id: lint-fix
    kind: agent_skill
    source:
      type: git
      repo: {}
      ref: main
      shallow: false
      path: skills/lint-fix
    dest: ./.claude/skills/lint-fix/
"#,
        source_repo.path().display(),
        source_repo.path().display(),
        source_repo.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    // Sync all three skills
    aps().arg("sync").current_dir(&project).assert().success();

    // Verify all three skills were installed
    project
        .child(".claude/skills/refactor/SKILL.md")
        .assert(predicate::path::exists());
    project
        .child(".claude/skills/test-gen/SKILL.md")
        .assert(predicate::path::exists());
    project
        .child(".claude/skills/lint-fix/SKILL.md")
        .assert(predicate::path::exists());
}

#[test]
fn add_existing_manifest_skips_duplicates_on_discover() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a local skills repo
    let source_repo = temp.child("skills-repo");
    source_repo.create_dir_all().unwrap();
    create_skills_repo(source_repo.path());

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    // Create an existing manifest with one entry already
    let existing = r#"entries:
  - id: existing-skill
    kind: agent_skill
    source:
      type: git
      repo: https://github.com/other/repo.git
      ref: main
      path: skills/existing
    dest: ./.claude/skills/existing-skill/
"#;
    project.child("aps.yaml").write_str(existing).unwrap();

    // The duplicate-skipping logic is tested via discover module unit tests.
    // Here we just verify the CLI flag works with existing manifests.
    aps()
        .args([
            "add",
            "https://github.com/westonplatter/agentically/tree/main/agents-md-partials",
            "--all",
            "--no-sync",
        ])
        .current_dir(&project)
        .assert()
        .failure()
        .stderr(predicate::str::contains("No skills found"));

    // The existing entry should still be there
    let manifest = project.child("aps.yaml");
    manifest.assert(predicate::str::contains("id: existing-skill"));
}

// ============================================================================
// Filesystem Path Discovery Tests
// ============================================================================

/// Helper to create a local skills directory (no git, just files)
fn create_skills_dir(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("skills/refactor")).unwrap();
    std::fs::write(
        dir.join("skills/refactor/SKILL.md"),
        "# Refactor\n\nRefactors code automatically.\n",
    )
    .unwrap();

    std::fs::create_dir_all(dir.join("skills/test-gen")).unwrap();
    std::fs::write(
        dir.join("skills/test-gen/SKILL.md"),
        "# Test Generation\n\nGenerates unit tests.\n",
    )
    .unwrap();

    // Non-skill directory
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(dir.join("docs/README.md"), "# Documentation\n").unwrap();
}

#[test]
fn add_local_path_discovers_skills_with_all() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a local skills directory
    let source = temp.child("my-skills");
    source.create_dir_all().unwrap();
    create_skills_dir(source.path());

    // Create project directory
    let project = temp.child("project");
    project.create_dir_all().unwrap();

    // Use a local path with --all --no-sync
    aps()
        .args([
            "add",
            &source.path().display().to_string(),
            "--all",
            "--no-sync",
        ])
        .current_dir(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains("Searching for skills"))
        .stdout(predicate::str::contains("Found 2 skill(s)"))
        .stdout(predicate::str::contains("Added 2 entries"));

    // Verify manifest was created with filesystem source entries
    let manifest = project.child("aps.yaml");
    manifest.assert(predicate::path::exists());
    manifest.assert(predicate::str::contains("type: filesystem"));
    manifest.assert(predicate::str::contains("id: refactor"));
    manifest.assert(predicate::str::contains("id: test-gen"));
    manifest.assert(predicate::str::contains("symlink: true"));
}

#[test]
fn add_local_single_skill_with_skill_md() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a single skill directory with SKILL.md
    let source = temp.child("my-skill");
    source.create_dir_all().unwrap();
    source
        .child("SKILL.md")
        .write_str("# My Skill\n\nDoes something.\n")
        .unwrap();

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    // Without --all, a dir with SKILL.md should be treated as single skill
    aps()
        .args(["add", &source.path().display().to_string(), "--no-sync"])
        .current_dir(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains("Added entry 'my-skill'"));

    // Verify manifest has filesystem source
    let manifest = project.child("aps.yaml");
    manifest.assert(predicate::str::contains("type: filesystem"));
    manifest.assert(predicate::str::contains("id: my-skill"));
}

#[test]
fn add_local_path_no_skills_found_errors() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Directory with no SKILL.md files
    let source = temp.child("empty-dir");
    source.create_dir_all().unwrap();
    source
        .child("README.md")
        .write_str("# Not a skill\n")
        .unwrap();

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    aps()
        .args([
            "add",
            &source.path().display().to_string(),
            "--all",
            "--no-sync",
        ])
        .current_dir(&project)
        .assert()
        .failure()
        .stderr(predicate::str::contains("No skills found"));
}

#[test]
fn add_local_path_syncs_filesystem_skills() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Create a local skills directory
    let source = temp.child("my-skills");
    source.create_dir_all().unwrap();
    create_skills_dir(source.path());

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    // Add and sync
    aps()
        .args(["add", &source.path().display().to_string(), "--all"])
        .current_dir(&project)
        .assert()
        .success();

    // Verify skills were synced (symlinked by default)
    project
        .child(".claude/skills/refactor/SKILL.md")
        .assert(predicate::path::exists());
    project
        .child(".claude/skills/test-gen/SKILL.md")
        .assert(predicate::path::exists());
}
