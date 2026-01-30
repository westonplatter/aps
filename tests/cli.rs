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
fn sync_claude_hooks_copies_directory_and_sets_exec() {
    let temp = assert_fs::TempDir::new().unwrap();

    let source = temp.child("source");
    source.create_dir_all().unwrap();
    source.child(".claude").create_dir_all().unwrap();
    source
        .child(".claude/scripts/start.sh")
        .write_str("echo start\n")
        .unwrap();
    source
        .child(".claude/settings.json")
        .write_str(
            r#"{
  "hooks": {
    "onSessionStart": [
      { "command": "bash $CLAUDE_PROJECT_DIR/.claude/scripts/start.sh" }
    ]
  }
}"#,
        )
        .unwrap();

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: claude-hooks
    kind: claude_hooks
    source:
      type: filesystem
      root: {}
      path: .claude
      symlink: false
    dest: ./.claude
"#,
        source.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    aps().arg("sync").current_dir(&project).assert().success();

    project
        .child(".claude/scripts/start.sh")
        .assert(predicate::path::exists());
    // Verify config is also synced to parent dir
    project
        .child(".claude/settings.json")
        .assert(predicate::path::exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(project.path().join(".claude/scripts/start.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o100, 0);
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
fn validate_claude_hooks_strict_rejects_missing_config() {
    let temp = assert_fs::TempDir::new().unwrap();

    let source = temp.child("source");
    source.create_dir_all().unwrap();
    source.child(".claude").create_dir_all().unwrap();
    source
        .child(".claude/scripts/start.sh")
        .write_str("echo start\n")
        .unwrap();

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: claude-hooks
    kind: claude_hooks
    source:
      type: filesystem
      root: {}
      path: .claude
      symlink: false
    dest: ./.claude
"#,
        source.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    aps()
        .args(["validate", "--strict"])
        .current_dir(&project)
        .assert()
        .failure()
        .stderr(predicate::str::contains("settings.json"));
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

#[test]
fn validate_claude_hooks_strict_accepts_valid() {
    let temp = assert_fs::TempDir::new().unwrap();

    let source = temp.child("source");
    source.create_dir_all().unwrap();
    source.child(".claude").create_dir_all().unwrap();
    source
        .child(".claude/scripts/start.sh")
        .write_str("echo start\n")
        .unwrap();
    source
        .child(".claude/settings.json")
        .write_str(
            r#"{
  "hooks": {
    "onSessionStart": [
      { "command": "bash .claude/scripts/start.sh" }
    ]
  }
}"#,
        )
        .unwrap();

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: claude-hooks
    kind: claude_hooks
    source:
      type: filesystem
      root: {}
      path: .claude
      symlink: false
    dest: ./.claude
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

#[test]
fn sync_reuses_git_clone_for_same_repo() {
    let temp = assert_fs::TempDir::new().unwrap();

    let source_repo = temp.child("source-repo");
    source_repo.create_dir_all().unwrap();
    create_git_repo_with_agents_md(source_repo.path(), "# Version 1\nOriginal content\n");

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: agents-one
    kind: agents_md
    source:
      type: git
      repo: {}
      ref: main
      shallow: false
      path: AGENTS.md
    dest: ./AGENTS-1.md
  - id: agents-two
    kind: agents_md
    source:
      type: git
      repo: {}
      ref: main
      shallow: false
      path: AGENTS.md
    dest: ./AGENTS-2.md
"#,
        source_repo.path().display(),
        source_repo.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    let output = aps()
        .args(["--verbose", "sync"])
        .current_dir(&project)
        .output()
        .expect("Failed to run aps sync");

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let combined_output = format!("{stdout}\n{stderr}");
    assert!(
        output.status.success(),
        "aps sync failed: {combined_output}"
    );

    let clone_count = combined_output.matches("Cloning git repository:").count();
    let reuse_count = combined_output.matches("Reusing cached clone").count();
    assert_eq!(
        clone_count, 1,
        "expected one clone for shared repo, output: {combined_output}"
    );
    assert!(
        reuse_count >= 1,
        "expected cached clone reuse, output: {combined_output}"
    );

    project
        .child("AGENTS-1.md")
        .assert(predicate::path::exists());
    project
        .child("AGENTS-2.md")
        .assert(predicate::path::exists());
}

#[test]
fn sync_clones_each_distinct_repo_once() {
    let temp = assert_fs::TempDir::new().unwrap();

    let source_repo_a = temp.child("source-repo-a");
    source_repo_a.create_dir_all().unwrap();
    create_git_repo_with_agents_md(source_repo_a.path(), "# Repo A\n");

    let source_repo_b = temp.child("source-repo-b");
    source_repo_b.create_dir_all().unwrap();
    create_git_repo_with_agents_md(source_repo_b.path(), "# Repo B\n");

    let project = temp.child("project");
    project.create_dir_all().unwrap();

    let manifest = format!(
        r#"entries:
  - id: agents-a
    kind: agents_md
    source:
      type: git
      repo: {}
      ref: main
      shallow: false
      path: AGENTS.md
    dest: ./AGENTS-A.md
  - id: agents-b
    kind: agents_md
    source:
      type: git
      repo: {}
      ref: main
      shallow: false
      path: AGENTS.md
    dest: ./AGENTS-B.md
"#,
        source_repo_a.path().display(),
        source_repo_b.path().display()
    );

    project.child("aps.yaml").write_str(&manifest).unwrap();

    let output = aps()
        .args(["--verbose", "sync"])
        .current_dir(&project)
        .output()
        .expect("Failed to run aps sync");

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let combined_output = format!("{stdout}\n{stderr}");
    assert!(
        output.status.success(),
        "aps sync failed: {combined_output}"
    );

    let clone_count = combined_output.matches("Cloning git repository:").count();
    let reuse_count = combined_output.matches("Reusing cached clone").count();
    assert_eq!(
        clone_count, 2,
        "expected one clone per distinct repo, output: {combined_output}"
    );
    assert_eq!(
        reuse_count, 0,
        "did not expect cache reuse across distinct repos, output: {combined_output}"
    );
}

// ============================================================================
// Composite Agents MD Tests (Live Git Sources)
// ============================================================================

#[test]
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
        "auto-generated by agentic-prompt-sync",
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
