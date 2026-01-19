# Agentic Prompt Sync (APS)

A manifest-driven CLI tool for safely syncing agentic assets (Cursor rules, Cursor skills, Claude agent skills, and AGENTS.md files) from git or filesystem sources into your repository.

## Features

- **Declarative manifest-driven sync** - Define your assets in a YAML manifest
- **Safe installs** - Automatic conflict detection and backup creation
- **Deterministic lockfile** - Idempotent pulls that only update when needed
- **Scriptable CLI** - Optional interactivity for CI/CD pipelines

## Getting Started

### Prerequisites

- Rust toolchain (1.70+)
- Cargo package manager

### Installation

Clone the repository and build:

```bash
git clone https://github.com/westonplatter/agentic-prompt-sync.git
cd agentic-prompt-sync
cargo build --release
```

The binary will be available at `target/release/aps`.

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
      root: /Users/my-username
      path: personal-generic-AGENTS.md
    dest: ./AGENTS.md
```

3. **Pull and install** your assets:

```bash
aps pull
```

4. **Check status** of synced assets:

```bash
aps status
```

## Commands

| Command        | Description                                       |
| -------------- | ------------------------------------------------- |
| `aps init`     | Create a new manifest file and update .gitignore  |
| `aps pull`     | Pull all entries from manifest and install assets |
| `aps validate` | Validate manifest schema and check sources        |
| `aps status`   | Display last pull information from lockfile       |

### Common Options

- `--verbose` - Enable verbose logging
- `--manifest <path>` - Specify manifest file path (default: `aps.yaml`)

### Pull Options

- `--yes` - Non-interactive mode, automatically confirm overwrites
- `--dry-run` - Preview changes without applying them
- `--only <id>` - Only pull specific entry by ID

## Configuration

### Manifest File (`aps.yaml`)

```yaml
entries:
  - id: my-agents
    kind: agents_md
    source:
      type: filesystem
      root: /Users/my-username
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
      root: /Users/my-username/work/acme-corp/internal-prompts
      path: rules
    dest: ./.cursor/rules/

  - id: rules-in-formation
    kind: cursor_rules
    source:
      type: filesystem
      root: /Users/my-username/work/acme-corp/internal-prompts
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
| `filesystem` | Pull from a local directory | `root`, `path`, `symlink`        |
| `git`        | Pull from a git repository  | `repo`, `ref`, `path`, `shallow` |

### Lockfile (`.aps.lock`)

The lockfile tracks installed assets and is automatically created/updated by `aps pull`. It stores:

- Source information
- Destination paths
- Last update timestamp
- Content checksum (SHA256)

## Examples

### Non-interactive pull for CI/CD

```bash
aps pull --yes
```

### Validate manifest before pull

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

### Run with verbose logging

```bash
cargo run -- --verbose pull
```

## License

See [LICENSE](LICENSE) for details.
