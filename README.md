# Agentic Prompt Sync (APS)

A manifest-driven CLI tool for safely syncing agentic assets (Cursor rules, Cursor skills, Claude agent skills, and AGENTS.md files) from git or filesystem sources into your repository.

## Features

- **Intelligent Asset Discovery** - Describe what you're working on and get suggestions for relevant prompts, skills, and rules
- **Declarative manifest-driven sync** - Define your assets in a YAML manifest
- **Safe installs** - Automatic conflict detection and backup creation
- **Deterministic lockfile** - Idempotent pulls that only update when needed
- **Scriptable CLI** - Optional interactivity for CI/CD pipelines

## What Makes It "Agentic"?

Unlike simple file syncers, APS includes **intelligent asset discovery** that analyzes what you're working on and recommends relevant prompts, skills, and rules from a curated catalog. This is genuinely agentic behavior - the tool makes context-aware decisions about which assets are relevant to your current task.

```bash
# Describe your task, get intelligent suggestions
aps suggest "I need to review a pull request for security vulnerabilities"

# Output:
# 🔍 Analyzing task: "I need to review a pull request for security vulnerabilities"
#
# Found 3 relevant asset(s) for your task:
#
#   1. Security-Focused Code Review [100%]
#      ID: code-review-security
#      Category: security | Kind: CursorRules
#      Why: Matched: tagged with 'security' and 2 more
#
#   2. Performance Review Guidelines [67%]
#      ID: code-review-performance
#      ...
```

The suggestion system uses **TF-IDF-style scoring** with field weighting to find the most relevant assets based on:
- Trigger phrases (user intent patterns)
- Tags and keywords
- Use cases and descriptions
- Category matching

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
      root: $HOME
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

| Command        | Description                                           |
| -------------- | ----------------------------------------------------- |
| `aps init`     | Create a new manifest file and update .gitignore      |
| `aps pull`     | Pull all entries from manifest and install assets     |
| `aps validate` | Validate manifest schema and check sources            |
| `aps status`   | Display last pull information from lockfile           |
| `aps suggest`  | **Intelligently suggest assets based on your task**   |
| `aps catalog`  | Manage and browse the asset catalog                   |
| `aps context`  | **Auto-detect project context and suggest assets**    |

### Common Options

- `--verbose` - Enable verbose logging
- `--manifest <path>` - Specify manifest file path (default: `aps.yaml`)

### Pull Options

- `--yes` - Non-interactive mode, automatically confirm overwrites
- `--dry-run` - Preview changes without applying them
- `--only <id>` - Only pull specific entry by ID

### Suggest Options

- `--limit <n>` - Maximum number of suggestions (default: 5)
- `--detailed` - Show full descriptions and use cases
- `--format <format>` - Output format: `pretty`, `json`, or `yaml`
- `--add-to-manifest` - Automatically add top suggestion to manifest
- `--catalog <path>` - Path to catalog file

### Catalog Subcommands

| Subcommand          | Description                              |
| ------------------- | ---------------------------------------- |
| `aps catalog list`  | List all assets in the catalog           |
| `aps catalog search`| Search for assets by keyword             |
| `aps catalog info`  | Show detailed information about an asset |
| `aps catalog init`  | Initialize a new catalog file            |
| `aps catalog add`   | Add a new asset to the catalog           |

### Context Options (for automation/hooks)

- `--message <text>` - Additional task description
- `--path <dir>` - Directory to analyze (default: current)
- `--limit <n>` - Maximum suggestions (default: 3)
- `--format <format>` - Output format: `mcp` (default), `pretty`, `json`, `yaml`
- `--auto-apply` - Automatically add suggestions to manifest
- `--threshold <0.0-1.0>` - Minimum confidence to include (default: 0.3)

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
| `filesystem` | Pull from a local directory | `root`, `path`, `symlink`        |
| `git`        | Pull from a git repository  | `repo`, `ref`, `path`, `shallow` |

**Shell Variable Expansion**: Path values in `root` and `path` fields support shell variable expansion (e.g., `$HOME`, `$USER`). This makes manifests portable across different machines and users.

### Lockfile (`.aps.lock`)

The lockfile tracks installed assets and is automatically created/updated by `aps pull`. It stores:

- Source information
- Destination paths
- Last update timestamp
- Content checksum (SHA256)

### Catalog File (`aps-catalog.yaml`)

The catalog is a curated index of available assets with rich metadata for intelligent discovery:

```yaml
version: "1.0"
assets:
  - id: code-review-security
    name: Security-Focused Code Review
    description: >-
      Comprehensive security review checklist covering OWASP Top 10,
      authentication patterns, and common vulnerabilities.
    kind: cursor_rules
    category: security
    tags:
      - security
      - code-review
      - owasp
    use_cases:
      - Reviewing PRs for security issues
      - Auditing authentication code
    keywords:
      - XSS
      - SQL injection
      - authentication
    triggers:
      - review this PR for security
      - check for vulnerabilities
    source:
      type: git
      repo: https://github.com/example/security-rules.git
      ref: main
      path: rules/security-review
    author: Security Team
    version: "2.1.0"
```

The catalog fields enable intelligent matching:
- **triggers**: User intent patterns that suggest this asset
- **use_cases**: Specific scenarios where this asset helps
- **keywords**: Technical terms for precise matching
- **tags**: Broad categories for filtering
- **description**: Full text search fallback

## Examples

### Intelligent Asset Discovery

```bash
# Get suggestions for your current task
aps suggest "I need to write tests for a React component"

# Get detailed suggestions with use cases
aps suggest --detailed "reviewing a Go microservice for performance"

# Automatically add the best match to your manifest
aps suggest --add-to-manifest "security audit for authentication"

# Output as JSON for scripting
aps suggest --format json "data science with pandas"
```

### Catalog Management

```bash
# Initialize a new catalog with examples
aps catalog init --with-examples

# Browse all available assets
aps catalog list

# Filter by category
aps catalog list --category security

# Search the catalog
aps catalog search "code review"

# Get detailed info about an asset
aps catalog info code-review-security
```

### Non-interactive pull for CI/CD

```bash
aps pull --yes
```

### Validate manifest before pull

```bash
aps validate --strict
```

## Integration with Agentic Tools

APS is designed to integrate with AI coding assistants like Claude Code and Cursor. The key integration point is the `aps context` command, which provides machine-readable output for automation.

### Context-Aware Auto-Suggestion

The `context` command analyzes your project and suggests relevant assets:

```bash
# Analyze current directory and get suggestions (MCP format by default)
aps context

# Output:
# {"suggestions":[{"id":"rust-best-practices","name":"Rust Best Practices","confidence":0.95,...}],"context":{"technologies":["rust"],...}}

# Add a task description for more targeted suggestions
aps context --message "I need to review this PR for security issues"

# Auto-apply suggestions (for hooks)
aps context --auto-apply --threshold 0.8
```

### Claude Code Integration

#### Using with Claude Code Hooks

Create a pre-task hook that suggests relevant assets:

```yaml
# .claude/hooks.yaml (proposed format)
hooks:
  pre_task:
    - name: suggest-assets
      command: aps context --format mcp
      on_result: suggest_to_user
```

#### As an MCP Tool

APS can be exposed as an MCP tool that Claude Code can call:

```json
{
  "name": "suggest_assets",
  "description": "Suggest relevant prompts, rules, and skills based on the current task",
  "input_schema": {
    "type": "object",
    "properties": {
      "task_description": {"type": "string"}
    }
  }
}
```

The agent would call:
```bash
aps suggest --format mcp "implement authentication with JWT"
```

### Cursor Integration

#### Dynamic Rule Loading

Instead of loading all rules statically, use APS to load rules based on context:

```bash
# In your project setup script or Cursor task
aps context --auto-apply
aps pull
```

#### Workspace Rules

Configure Cursor to use APS-managed rules:

```json
// .cursor/settings.json
{
  "rules": {
    "source": ".cursor/rules/",
    "refresh_command": "aps context --auto-apply && aps pull"
  }
}
```

### MCP Output Format

The `--format mcp` output is designed for tool consumption:

```json
{
  "suggestions": [
    {
      "id": "code-review-security",
      "name": "Security-Focused Code Review",
      "kind": "CursorRules",
      "category": "security",
      "confidence": 0.95,
      "reason": "Matched: tagged with 'security' and 2 more",
      "action": "aps pull --only code-review-security"
    }
  ],
  "context": {
    "technologies": ["rust", "typescript"],
    "frameworks": ["react"],
    "task_hints": ["testing"],
    "query": "rust typescript react testing"
  }
}
```

### Automation Patterns

#### Pre-commit Hook

```bash
#!/bin/bash
# .git/hooks/pre-commit
aps context --format mcp | jq -e '.suggestions | length > 0' && \
  echo "APS suggests additional rules for this commit type"
```

#### CI/CD Integration

```yaml
# .github/workflows/pr-review.yml
- name: Suggest review rules
  run: |
    SUGGESTIONS=$(aps suggest --format json "PR review for ${{ github.event.pull_request.title }}")
    echo "Suggested assets: $SUGGESTIONS"
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
cargo run -- --verbose pull
```

## License

See [LICENSE](LICENSE) for details.
