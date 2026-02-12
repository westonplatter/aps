# Manual Test Plan: Cursor Hooks (`cursor_hooks`)

This test plan covers the `cursor_hooks` asset kind introduced in v0.1.9.
Each test case includes setup steps, the action to perform, and expected results.

---

## Prerequisites

- `aps` binary built locally (`cargo build --release`)
- A clean temp directory for each test (e.g., `mktemp -d`)
- Unix/macOS system (some tests are Unix-specific for permissions)

---

## Test 1: Basic sync — filesystem source, copy mode

**Setup:**
```bash
WORKDIR=$(mktemp -d)
mkdir -p $WORKDIR/source/.cursor/scripts/nested
echo '#!/bin/bash\necho hello' > $WORKDIR/source/.cursor/scripts/hello.sh
echo '#!/bin/bash\necho inner' > $WORKDIR/source/.cursor/scripts/nested/inner.sh
cat > $WORKDIR/source/.cursor/hooks.json << 'EOF'
{
  "hooks": {
    "onStart": [
      { "command": "bash .cursor/scripts/hello.sh" },
      { "command": "bash .cursor/scripts/nested/inner.sh" }
    ]
  }
}
EOF

mkdir -p $WORKDIR/project
cat > $WORKDIR/project/aps.yaml << EOF
entries:
  - id: my-hooks
    kind: cursor_hooks
    source:
      type: filesystem
      root: $WORKDIR/source
      path: .cursor
      symlink: false
    dest: ./.cursor
EOF
```

**Action:**
```bash
cd $WORKDIR/project && aps sync
```

**Expected:**
- [ ] `$WORKDIR/project/.cursor/scripts/hello.sh` exists with correct content
- [ ] `$WORKDIR/project/.cursor/scripts/nested/inner.sh` exists with correct content
- [ ] `$WORKDIR/project/.cursor/hooks.json` exists (synced to parent of hooks dir)
- [ ] On Unix: `hello.sh` and `inner.sh` have executable permission (`ls -l` shows `x` bit)
- [ ] `aps.lock` is created/updated with a `my-hooks` entry
- [ ] Exit code is 0, no errors in output

---

## Test 2: Basic sync — filesystem source, symlink mode

**Setup:**
Same as Test 1, but change the manifest to use `symlink: true`:
```yaml
entries:
  - id: my-hooks
    kind: cursor_hooks
    source:
      type: filesystem
      root: $WORKDIR/source
      path: .cursor
      symlink: true
    dest: ./.cursor
```

**Action:**
```bash
cd $WORKDIR/project && aps sync
```

**Expected:**
- [ ] `$WORKDIR/project/.cursor/scripts/hello.sh` is a **symlink** pointing to source
- [ ] `$WORKDIR/project/.cursor/scripts/nested/inner.sh` is a **symlink** pointing to source
- [ ] `$WORKDIR/project/.cursor/hooks.json` is a **symlink** pointing to source
- [ ] Symlinks resolve correctly (`cat` shows expected content)
- [ ] Exit code is 0

---

## Test 3: Sync from a git source

**Setup:**
```bash
WORKDIR=$(mktemp -d)
# Create a local git repo as the "remote"
mkdir -p $WORKDIR/remote/.cursor/scripts
echo '#!/bin/bash\necho from-git' > $WORKDIR/remote/.cursor/scripts/run.sh
cat > $WORKDIR/remote/.cursor/hooks.json << 'EOF'
{
  "hooks": {
    "onStart": [
      { "command": "bash .cursor/scripts/run.sh" }
    ]
  }
}
EOF
cd $WORKDIR/remote && git init && git add -A && git commit -m "init"

mkdir -p $WORKDIR/project
cat > $WORKDIR/project/aps.yaml << EOF
entries:
  - id: git-hooks
    kind: cursor_hooks
    source:
      type: git
      repo: $WORKDIR/remote
      path: .cursor
      symlink: false
    dest: ./.cursor
EOF
```

**Action:**
```bash
cd $WORKDIR/project && aps sync
```

**Expected:**
- [ ] Scripts are cloned and copied into `$WORKDIR/project/.cursor/scripts/`
- [ ] `hooks.json` is present at `$WORKDIR/project/.cursor/hooks.json`
- [ ] Scripts are executable on Unix
- [ ] Lockfile records the git commit SHA

---

## Test 4: Validate — strict mode rejects missing `hooks.json`

**Setup:**
```bash
WORKDIR=$(mktemp -d)
mkdir -p $WORKDIR/source/.cursor/scripts
echo '#!/bin/bash\necho hello' > $WORKDIR/source/.cursor/scripts/hello.sh
# NOTE: No hooks.json created

mkdir -p $WORKDIR/project
cat > $WORKDIR/project/aps.yaml << EOF
entries:
  - id: no-config-hooks
    kind: cursor_hooks
    source:
      type: filesystem
      root: $WORKDIR/source
      path: .cursor
      symlink: false
    dest: ./.cursor
EOF
```

**Action:**
```bash
cd $WORKDIR/project && aps validate --strict
```

**Expected:**
- [ ] Command exits with **non-zero** status
- [ ] Error output mentions `hooks.json`

---

## Test 5: Validate — strict mode accepts valid hooks

**Setup:**
Same as Test 1 (valid `hooks.json` with matching scripts).

**Action:**
```bash
cd $WORKDIR/project && aps validate --strict
```

**Expected:**
- [ ] Command exits with **0** status
- [ ] No errors or warnings in output

---

## Test 6: Validate — strict rejects missing referenced script

**Setup:**
```bash
WORKDIR=$(mktemp -d)
mkdir -p $WORKDIR/source/.cursor/scripts
# Create hooks.json referencing a script that does NOT exist
cat > $WORKDIR/source/.cursor/hooks.json << 'EOF'
{
  "hooks": {
    "onStart": [
      { "command": "bash .cursor/scripts/nonexistent.sh" }
    ]
  }
}
EOF

mkdir -p $WORKDIR/project
cat > $WORKDIR/project/aps.yaml << EOF
entries:
  - id: bad-script-ref
    kind: cursor_hooks
    source:
      type: filesystem
      root: $WORKDIR/source
      path: .cursor
      symlink: false
    dest: ./.cursor
EOF
```

**Action:**
```bash
cd $WORKDIR/project && aps validate --strict
```

**Expected:**
- [ ] Command exits with **non-zero** status
- [ ] Error references `nonexistent.sh` or indicates a missing script

---

## Test 7: Validate — non-strict mode warns instead of failing

**Setup:**
Same as Test 4 (missing `hooks.json`).

**Action:**
```bash
cd $WORKDIR/project && aps validate
```

**Expected:**
- [ ] Command exits with **0** status
- [ ] Output contains a **warning** about missing `hooks.json` (not an error)

---

## Test 8: Conflict detection — existing files at destination

**Setup:**
```bash
WORKDIR=$(mktemp -d)
# Create source
mkdir -p $WORKDIR/source/.cursor/scripts
echo '#!/bin/bash\necho new' > $WORKDIR/source/.cursor/scripts/hello.sh
cat > $WORKDIR/source/.cursor/hooks.json << 'EOF'
{ "hooks": { "onStart": [{ "command": "bash .cursor/scripts/hello.sh" }] } }
EOF

# Create project with pre-existing conflicting file
mkdir -p $WORKDIR/project/.cursor/scripts
echo '#!/bin/bash\necho old' > $WORKDIR/project/.cursor/scripts/hello.sh

cat > $WORKDIR/project/aps.yaml << EOF
entries:
  - id: conflict-hooks
    kind: cursor_hooks
    source:
      type: filesystem
      root: $WORKDIR/source
      path: .cursor
      symlink: false
    dest: ./.cursor
EOF
```

**Action (interactive):**
```bash
cd $WORKDIR/project && aps sync
```

**Expected:**
- [ ] APS detects the conflict and **prompts** for confirmation
- [ ] Answering "yes" overwrites the file and creates a backup in `.aps-backups/`
- [ ] Answering "no" skips the entry

**Action (non-interactive / auto-yes):**
```bash
cd $WORKDIR/project && aps sync --yes
```

**Expected:**
- [ ] APS overwrites without prompting
- [ ] Backup is created in `.aps-backups/`
- [ ] New content is in place

---

## Test 9: Conflict detection — symlinks are NOT treated as conflicts

**Setup:**
```bash
WORKDIR=$(mktemp -d)
# Create two sources
mkdir -p $WORKDIR/source1/.cursor/scripts
echo '#!/bin/bash\necho src1' > $WORKDIR/source1/.cursor/scripts/a.sh
cat > $WORKDIR/source1/.cursor/hooks.json << 'EOF'
{ "hooks": { "onStart": [{ "command": "bash .cursor/scripts/a.sh" }] } }
EOF

mkdir -p $WORKDIR/source2/.cursor/scripts
echo '#!/bin/bash\necho src2' > $WORKDIR/source2/.cursor/scripts/b.sh
cat > $WORKDIR/source2/.cursor/hooks.json << 'EOF'
{ "hooks": { "onStart": [{ "command": "bash .cursor/scripts/b.sh" }] } }
EOF

# First sync source1 with symlinks
mkdir -p $WORKDIR/project
cat > $WORKDIR/project/aps.yaml << EOF
entries:
  - id: hooks-src1
    kind: cursor_hooks
    source:
      type: filesystem
      root: $WORKDIR/source1
      path: .cursor
      symlink: true
    dest: ./.cursor
EOF
cd $WORKDIR/project && aps sync

# Now change manifest to source2 (copy mode) — existing symlinks should not conflict
cat > $WORKDIR/project/aps.yaml << EOF
entries:
  - id: hooks-src2
    kind: cursor_hooks
    source:
      type: filesystem
      root: $WORKDIR/source2
      path: .cursor
      symlink: false
    dest: ./.cursor
EOF
```

**Action:**
```bash
cd $WORKDIR/project && aps sync
```

**Expected:**
- [ ] No conflict prompt (symlinks from source1 are excluded from conflict detection)
- [ ] source2 scripts are now present
- [ ] Exit code is 0

---

## Test 10: Dry-run mode

**Setup:**
Same as Test 1.

**Action:**
```bash
cd $WORKDIR/project && aps sync --dry-run
```

**Expected:**
- [ ] No files are created under `$WORKDIR/project/.cursor/`
- [ ] Output describes what *would* be done
- [ ] Exit code is 0

---

## Test 11: Idempotent re-sync (no changes)

**Setup:**
Run Test 1 setup and `aps sync` successfully once.

**Action:**
```bash
cd $WORKDIR/project && aps sync
```

**Expected:**
- [ ] Command succeeds (exit 0)
- [ ] Files are unchanged (same content, same permissions)
- [ ] No unnecessary backup created
- [ ] Lockfile unchanged (same checksum)

---

## Test 12: Re-sync after source changes (upgrade)

**Setup:**
Run Test 1 setup and `aps sync` successfully once. Then modify the source:
```bash
echo '#!/bin/bash\necho updated' > $WORKDIR/source/.cursor/scripts/hello.sh
```

**Action:**
```bash
cd $WORKDIR/project && aps sync --upgrade
```

**Expected:**
- [ ] `hello.sh` in the project now contains "updated"
- [ ] Lockfile checksum is updated
- [ ] Exit code is 0

---

## Test 13: `hooks.json` is synced to the parent directory

This validates `sync_hooks_config` places `hooks.json` at the parent of the hooks directory.

**Setup:**
Same as Test 1 (hooks scripts are under `.cursor/scripts/`, hooks.json at `.cursor/hooks.json`).

**Action:**
```bash
cd $WORKDIR/project && aps sync
```

**Expected:**
- [ ] `$WORKDIR/project/.cursor/hooks.json` exists (NOT inside `scripts/`)
- [ ] Content matches the source `hooks.json`

---

## Test 14: Executable permissions on `.sh` files (Unix only)

**Setup:**
Same as Test 1.

**Action:**
```bash
cd $WORKDIR/project && aps sync
stat -c '%a' $WORKDIR/project/.cursor/scripts/hello.sh
stat -c '%a' $WORKDIR/project/.cursor/scripts/nested/inner.sh
```

**Expected:**
- [ ] Both scripts have the execute bit set (mode includes `1` in owner/group/other, e.g., `755` or `711`)
- [ ] Non-`.sh` files are NOT given execute permissions

---

## Test 15: Manifest rejects deprecated `claude_hooks` kind

**Setup:**
```bash
WORKDIR=$(mktemp -d)
mkdir -p $WORKDIR/project
cat > $WORKDIR/project/aps.yaml << 'EOF'
entries:
  - id: legacy
    kind: claude_hooks
    source:
      type: filesystem
      root: /tmp
      path: .claude
EOF
```

**Action:**
```bash
cd $WORKDIR/project && aps validate
```

**Expected:**
- [ ] Command fails (non-zero exit)
- [ ] Error message mentions `claude_hooks` is invalid
- [ ] Error message suggests using `cursor_hooks` instead

---

## Test 16: Custom destination override

**Setup:**
Same as Test 1 but change `dest` in the manifest:
```yaml
dest: ./custom-hooks-dir
```

**Action:**
```bash
cd $WORKDIR/project && aps sync
```

**Expected:**
- [ ] Scripts are placed under `$WORKDIR/project/custom-hooks-dir/scripts/`
- [ ] `hooks.json` is synced to `$WORKDIR/project/hooks.json` (parent of custom-hooks-dir)
- [ ] Default `.cursor/hooks` directory is NOT created

---

## Test 17: `aps status` shows cursor hooks entry

**Setup:**
Run Test 1 setup and `aps sync` once.

**Action:**
```bash
cd $WORKDIR/project && aps status
```

**Expected:**
- [ ] Output lists the `my-hooks` entry
- [ ] Shows the kind as `cursor_hooks`
- [ ] Indicates it is synced/up-to-date

---

## Test 18: Orphan cleanup when entry is removed

**Setup:**
Run Test 1 and sync. Then remove the hooks entry from `aps.yaml`:
```yaml
entries: []
```

**Action:**
```bash
cd $WORKDIR/project && aps sync
```

**Expected:**
- [ ] APS detects the orphaned `.cursor/` path
- [ ] In interactive mode: prompts to confirm removal
- [ ] Backup is created before removal
- [ ] After confirmation, the hooks files are removed

---

## Test 19: Verbose output

**Setup:**
Same as Test 1.

**Action:**
```bash
cd $WORKDIR/project && aps --verbose sync
```

**Expected:**
- [ ] Additional debug-level output is shown (e.g., file operations, checksum computation)
- [ ] Sync succeeds as normal

---

## Test 20: Multiple hook event types in `hooks.json`

**Setup:**
```bash
WORKDIR=$(mktemp -d)
mkdir -p $WORKDIR/source/.cursor/scripts
echo '#!/bin/bash\necho start' > $WORKDIR/source/.cursor/scripts/start.sh
echo '#!/bin/bash\necho save' > $WORKDIR/source/.cursor/scripts/on-save.sh
echo '#!/bin/bash\necho exit' > $WORKDIR/source/.cursor/scripts/cleanup.sh

cat > $WORKDIR/source/.cursor/hooks.json << 'EOF'
{
  "hooks": {
    "onStart": [
      { "command": "bash .cursor/scripts/start.sh" }
    ],
    "onSave": [
      { "command": "bash .cursor/scripts/on-save.sh" }
    ],
    "onExit": [
      { "command": "bash .cursor/scripts/cleanup.sh" }
    ]
  }
}
EOF

mkdir -p $WORKDIR/project
cat > $WORKDIR/project/aps.yaml << EOF
entries:
  - id: multi-hooks
    kind: cursor_hooks
    source:
      type: filesystem
      root: $WORKDIR/source
      path: .cursor
      symlink: false
    dest: ./.cursor
EOF
```

**Action:**
```bash
cd $WORKDIR/project && aps sync
cd $WORKDIR/project && aps validate --strict
```

**Expected:**
- [ ] All three scripts are synced and executable
- [ ] `hooks.json` is synced with all event types preserved
- [ ] Strict validation passes (all referenced scripts exist)
