<div align="center"> <!-- markdownlint-disable MD033 MD041 -->

# Agentic Prompt Sync (aps)

**Compose and sync your own collection of AGENTS.md, Skills, and other agentic prompts.**

[![CI](https://github.com/westonplatter/agentic-prompt-sync/actions/workflows/ci.yml/badge.svg)](https://github.com/westonplatter/agentic-prompt-sync/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/aps?style=flat-square)](https://crates.io/crates/aps)
[![Downloads](https://img.shields.io/crates/d/aps?style=flat-square)](https://crates.io/crates/aps)
[![GitHub stars](https://img.shields.io/github/stars/westonplatter/agentic-prompt-sync?style=flat-square)](https://github.com/westonplatter/agentic-prompt-sync/stargazers)
[![License](https://img.shields.io/badge/license-BSD--3--Clause-blue?style=flat-square)](LICENSE)

**Cross-platform support:** macOS • Linux • Windows

![Example of running ap sync](./docs/aps-example.png)

</div>

## Features

`aps` is a manifest-driven, CLI tool for syncing agentic assets (Cursor rules, Agent Skills, and AGENTS.md files) from sources like git or your filesystem in your project folders.

- **Declarative manifest-driven sync** - Define your agentic assets in a YAML manifest
- **Composable AGENTS.md** - Merge multiple AGENTS.md files from local or remote sources into one
- **Safe installs** - Automatic conflict detection and backup creation
- **Deterministic lockfile** - Idempotent syncs that only update when needed
- **Optimized Git Operations** - Smart caching reuses clones and skips unchanged commits for fast syncs

## Installation

### Quick Install (macOS/Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/westonplatter/agentic-prompt-sync/main/install.sh | sh
```

This downloads the latest release and installs to `~/.local/bin`. Set `APS_INSTALL_DIR` to customize:

```bash
APS_INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/westonplatter/agentic-prompt-sync/main/install.sh | sh
```

### Download Binary

Pre-built binaries for all platforms are available on the [Releases page](https://github.com/westonplatter/agentic-prompt-sync/releases).

| Platform    | Download                    |
| ----------- | --------------------------- |
| Linux x64   | `aps-linux-x64-musl.tar.gz` |
| Linux ARM64 | `aps-linux-arm64.tar.gz`    |
| macOS Intel | `aps-macos-x64.tar.gz`      |
| macOS ARM   | `aps-macos-arm64.tar.gz`    |
| Windows x64 | `aps-windows-x64.zip`       |

### Cargo Install

If you have Rust installed:

```bash
cargo install aps
```

### Build from Source

```bash
git clone https://github.com/westonplatter/agentic-prompt-sync.git
cd agentic-prompt-sync
cargo build --release
# Binary at target/release/aps
```

## Updating

To update `aps` to the latest version, use the same method you used to install it.

### Quick Update (macOS/Linux)

Re-run the install script to download and install the latest version:

```bash
curl -fsSL https://raw.githubusercontent.com/westonplatter/agentic-prompt-sync/main/install.sh | sh
```

Or download the latest binary for your platform from the [Releases page](https://github.com/westonplatter/agentic-prompt-sync/releases) and replace your existing installation.

### Cargo Update

If you installed via `cargo install`:

```bash
cargo install aps
```

Cargo will automatically detect and install the newer version. If you want to force a reinstall of the same version, use:

```bash
cargo install aps --force
```

## Getting Started

### Quick Start

1. **Initialize a manifest** in your project:

   ```bash
   aps init
   ```

   This creates a `aps.yaml` manifest file with an example entry.

2. **Add assets directly from git repositories:**

   ```bash
   # Add a skill from a GitHub URL - automatically syncs the skill
   aps add agent_skill https://github.com/hashicorp/agent-skills/blob/main/terraform/module-generation/skills/refactor-module/SKILL.md

   # Or use the folder URL (SKILL.md is auto-detected)
   aps add agent_skill https://github.com/hashicorp/agent-skills/tree/main/terraform/module-generation/skills/refactor-module

   # Add Cursor rules from a private repo (SSH)
   aps add cursor_rules git@github.com:org/private-rules.git

   # Add a private skill with an explicit path
   aps add agent_skill git@github.com:org/private-skills.git --path skills/refactor-module
   ```

   This parses the repository identifier, adds an entry to `aps.yaml`, and syncs **only that asset** immediately (other entries are not affected).

3. **Or manually edit the manifest** to define your assets:

   ```yaml
   entries:
     - id: my-agents
       kind: agents_md
       source:
         type: filesystem
         root: $HOME
         path: personal-generic-AGENTS.md
       dest: ./AGENTS.md
   ```

4. **Sync and install** your assets:

   ```bash
   aps sync
   ```

5. **Check status** of synced assets:

   ```bash
   aps status
   ```

## Commands

| Command        | Description                                       |
| -------------- | ------------------------------------------------- |
| `aps init`     | Create a new manifest file and update .gitignore  |
| `aps add`      | Add an asset from a git repo and sync it          |
| `aps sync`     | Sync all entries from manifest and install assets |
| `aps validate` | Validate manifest schema and check sources        |
| `aps status`   | Display last sync information from lockfile       |

### Common Options

- `--verbose` - Enable verbose logging
- `--manifest <path>` - Specify manifest file path (default: `aps.yaml`)

### Add Options

- `aps add <asset_type> <repo_url_or_path>` - Asset types: `agent_skill`, `cursor_rules`, `cursor_skills_root`, `agents_md`
- `--id <name>` - Custom entry ID (defaults to repo or path name)
- `--path <path>` - Path within the repository (required for `agent_skill` when using SSH/HTTPS repo URLs)
- `--ref <ref>` - Git ref (branch/tag/commit) to use
- `--no-sync` - Only add to manifest, don't sync immediately

### Sync Options

- `--yes` - Non-interactive mode, automatically confirm overwrites
- `--dry-run` - Preview changes without applying them
- `--only <id>` - Only sync specific entry by ID

### Sync Behavior

When you run `aps sync`:

1. **Entries are synced** - Each entry in `aps.yaml` is installed to its destination (git sources reuse cached clones for speed)
2. **Stale entries are cleaned** - Entries in the lockfile that no longer exist in `aps.yaml` are automatically removed
3. **Lockfile is saved** - The updated lockfile is written to disk

Note: Stale entry cleanup only happens during a full sync. When using `--only <id>` to sync specific entries, other lockfile entries are preserved.

## Configuration

### Manifest File (`aps.yaml`)

```yaml
entries:
  # Single AGENTS.md file from one source
  - id: my-agents
    kind: agents_md
    source:
      type: filesystem
      root: $HOME
      path: AGENTS-generic.md
    dest: ./AGENTS-simple.md

  # Composite AGENTS.md - merge multiple markdown files into one
  - id: composite-agents
    kind: composite_agents_md
    sources:
      - type: filesystem
        root: $HOME/agents
        path: AGENT.python.md
      - type: filesystem
        root: $HOME/agents
        path: AGENT.docker.md
      - type: git
        repo: https://github.com/apache/airflow.git
        ref: main
        path: AGENTS.md
    dest: ./AGENTS.md

  # Pull in Agent Skills from a public git repo
  - id: anthropic-skills
    kind: agent_skill
    source:
      type: git
      repo: git@github.com:anthropics/skills.git
      ref: main
      path: skills
    include:
      - pdf
      - skill-creation
    dest: ./.claude/skills/

  # Pull Cursor Rules from a private git repo
  - id: personal-rules
    kind: cursor_rules
    source:
      type: git
      repo: git@github.com:your-username/dotfiles.git
      ref: main
      path: .cursor/rules
    dest: ./.cursor/rules/

  # Pull in more Cursor Rules from a local file system
  - id: company-rules
    kind: cursor_rules
    source:
      type: filesystem
      root: $HOME/work/acme-corp/internal-prompts
      path: rules
    dest: ./.cursor/rules/
```

### Asset Types

| Kind                  | Description                            | Default Destination |
| --------------------- | -------------------------------------- | ------------------- |
| `agents_md`           | Single AGENTS.md file                  | `./AGENTS.md`       |
| `composite_agents_md` | Merge multiple markdown files into one | `./AGENTS.md`       |
| `cursor_rules`        | Directory of Cursor rules              | `./.cursor/rules/`  |
| `cursor_hooks`        | Directory of Cursor hooks              | `./.cursor/hooks/`  |
| `cursor_skills_root`  | Directory with skill subdirs           | `./.cursor/skills/` |
| `agent_skill`         | Claude agent skill directory           | `./.claude/skills/` |

### Source Types

| Type         | Description                 | Key Properties                   |
| ------------ | --------------------------- | -------------------------------- |
| `filesystem` | Sync from a local directory | `root`, `path`, `symlink`        |
| `git`        | Sync from a git repository  | `repo`, `ref`, `path`, `shallow` |

**Shell Variable Expansion**: Path values in `root` and `path` fields support shell variable expansion (e.g., `$HOME`, `$USER`). This makes manifests portable across different machines and users.

### Composite AGENTS.md

The `composite_agents_md` kind allows you to merge multiple markdown files into a single `AGENTS.md` file. This is useful when you want to organize agent definitions across separate files (e.g., by language or framework) and combine them at sync time.

```yaml
entries:
  - id: my-composite-agents
    kind: composite_agents_md
    sources:
      # Local filesystem sources
      - type: filesystem
        root: $HOME/agents-md-partials
        path: AGENT.docker.md
      # Remote git sources
      - type: git
        repo: https://github.com/westonplatter/agentically.git
        ref: main
        path: agents-md-partials/AGENTS.python.md
    dest: ./AGENTS.md
```

Key features:

- **Mixed sources**: Combine local filesystem and remote git sources
- **Order preserved**: Files are merged in the order specified in `sources`
- **Auto-generated header**: Output includes a comment indicating it was composed by aps

### Lockfile (`aps.lock.yaml`)

The lockfile tracks installed assets and is automatically created/updated by `aps sync`. **This file should be committed to version control** to ensure reproducible installations across your team. It stores:

- Source information
- Destination paths
- Last update timestamp
- Content checksum (SHA256)

**Environment Variables Are Preserved**: Unlike other package managers (npm, uv, bundler) that expand environment variables to concrete paths, `aps` preserves shell variables like `$HOME` in the lockfile. This makes lockfiles portable across different machines and users who have the same relative directory structure.

## Examples

### Non-interactive sync for CI/CD

```bash
aps sync --yes
```

### Validate manifest before sync

```bash
aps validate --strict
```

## Development

### Build

```bash
cargo build           # Debug build
cargo build --release # Release build
```

### Run tests

```bash
cargo test
```

### Linting

This project uses [Trunk](https://docs.trunk.io) for linting and code quality checks.

```bash
trunk check       # Run linters on modified files
trunk fmt         # Format code
trunk check list  # View available linters
```

### Run with verbose logging

```bash
cargo run -- --verbose sync
```

## Inspiration

Built based on inspiration from these other projects,

- [rule-tool](https://github.com/circleci-petri/rule-tool/)

## License

See [LICENSE](LICENSE) for details.
