# Agentic Prompt Sync (aps)

Use `aps` to compose and sync your own custom collection of agentic prompts/skills/etc.

![Example of running ap sync](./docs/aps-example.png)

## Features

`aps` is a manifest-driven, CLI tool for syncing agentic assets (Cursor rules, Agent Skills, and AGENTS.md files) from sources like git or your filesystem in your project folders.

- **Declarative manifest-driven sync** - Define your agentic assets in a YAML manifest
- **Safe installs** - Automatic conflict detection and backup creation
- **Deterministic lockfile** - Idempotent syncs that only update when needed
- **Scriptable CLI** - Optional interactivity for CI/CD pipelines

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

## Getting Started

### Quick Start

1. **Initialize a manifest** in your project:

```bash
aps init
```

This creates a `aps.yaml` manifest file with an example entry.

2. **Edit the manifest** to define your assets:

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

3. **Sync and install** your assets:

```bash
aps sync
```

4. **Check status** of synced assets:

```bash
aps status
```

## Commands

| Command        | Description                                       |
| -------------- | ------------------------------------------------- |
| `aps init`     | Create a new manifest file and update .gitignore  |
| `aps sync`     | Sync all entries from manifest and install assets |
| `aps validate` | Validate manifest schema and check sources        |
| `aps status`   | Display last sync information from lockfile       |

### Common Options

- `--verbose` - Enable verbose logging
- `--manifest <path>` - Specify manifest file path (default: `aps.yaml`)

### Sync Options

- `--yes` - Non-interactive mode, automatically confirm overwrites
- `--dry-run` - Preview changes without applying them
- `--only <id>` - Only sync specific entry by ID

### Sync Behavior

When you run `aps sync`:

1. **Entries are synced** - Each entry in `aps.yaml` is installed to its destination
2. **Stale entries are cleaned** - Entries in the lockfile that no longer exist in `aps.yaml` are automatically removed
3. **Lockfile is saved** - The updated lockfile is written to disk

Note: Stale entry cleanup only happens during a full sync. When using `--only <id>` to sync specific entries, other lockfile entries are preserved.

## Configuration

### Manifest File (`aps.yaml`)

```yaml
entries:
  - id: my-agents
    kind: agents_md
    source:
      type: filesystem
      root: $HOME
      path: AGENTS-generic.md
    dest: AGENTS.md

  - id: personal-rules
    kind: cursor_rules
    source:
      type: git
      repo: git@github.com:your-username/dotfiles.git
      ref: main
      path: .cursor/rules
    dest: ./.cursor/rules/

  - id: company-rules
    kind: cursor_rules
    source:
      type: filesystem
      root: $HOME/work/acme-corp/internal-prompts
      path: rules
    dest: ./.cursor/rules/

  - id: rules-in-formation
    kind: cursor_rules
    source:
      type: filesystem
      root: $HOME/work/acme-corp/internal-prompts
      path: dumping-ground
    dest: ./.cursor/rules/

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
```

### Asset Types

| Kind                 | Description                  | Default Destination |
| -------------------- | ---------------------------- | ------------------- |
| `agents_md`          | Single AGENTS.md file        | `./AGENTS.md`       |
| `cursor_rules`       | Directory of Cursor rules    | `./.cursor/rules/`  |
| `cursor_skills_root` | Directory with skill subdirs | `./.cursor/skills/` |
| `agent_skill`        | Claude agent skill directory | `./.claude/skills/` |

### Source Types

| Type         | Description                 | Key Properties                   |
| ------------ | --------------------------- | -------------------------------- |
| `filesystem` | Sync from a local directory | `root`, `path`, `symlink`        |
| `git`        | Sync from a git repository  | `repo`, `ref`, `path`, `shallow` |

**Shell Variable Expansion**: Path values in `root` and `path` fields support shell variable expansion (e.g., `$HOME`, `$USER`). This makes manifests portable across different machines and users.

### Lockfile (`aps.manifest.lock`)

The lockfile tracks installed assets and is automatically created/updated by `aps sync`. **This file should be committed to version control** to ensure reproducible installations across your team. It stores:

- Source information
- Destination paths
- Last update timestamp
- Content checksum (SHA256)

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

- rule-tool - https://github.com/circleci-petri/rule-tool/

## License

See [LICENSE](LICENSE) for details.
