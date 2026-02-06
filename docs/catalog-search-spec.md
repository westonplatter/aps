# Catalog System - Technical Specification

## Overview

The catalog system enumerates all individual assets from manifest sources into a single searchable index. This enables discovery, filtering, and (future) intelligent suggestions based on natural language queries.

## Current Implementation

### Data Model

**Catalog** - Container for all asset entries:

```yaml
version: 1
entries:
  - id: company-rules:atmos-best-practices.mdc
    name: atmos-best-practices.mdc
    kind: cursor_rules
    destination: ./.cursor/rules/atmos-best-practices.mdc
    short_description: "Enforce best practices for organizing..."
```

**CatalogEntry** fields:

| Field             | Type      | Description                                    |
| ----------------- | --------- | ---------------------------------------------- |
| id                | string    | Unique identifier (`manifest_id:asset_name`)   |
| name              | string    | Human-readable asset name                      |
| kind              | AssetKind | Asset type (cursor_rules, cursor_skills, etc.) |
| destination       | string    | Installation path relative to project root     |
| short_description | string?   | Auto-extracted description (up to 200 chars)   |

**AssetKind** values:

- `cursor_rules` - Individual `.mdc` rule files
- `cursor_hooks` - Individual hook scripts
- `cursor_skills_root` - Skill folders for Cursor
- `agents_md` - AGENTS.md files
- `agent_skill` - Agent skill folders (per agentskills.io spec)

### Description Extraction

Descriptions are automatically extracted from asset files:

**Cursor Rules (.mdc files)**:

1. Tries YAML frontmatter `description` field first
2. Falls back to first non-heading paragraph

**Cursor Skills (SKILL.md)**:

1. Tries frontmatter description
2. Falls back to first paragraph

**Agent Skills**:

1. Tries SKILL.md with frontmatter/paragraph
2. Falls back to README.md first paragraph

**AGENTS.md files**:

- Reads first paragraph up to 200 chars

All descriptions are truncated to 200 characters at word boundaries with ellipsis.

### CLI Commands

```bash
# Generate catalog from manifest
aps catalog generate [--manifest <path>] [--output <path>]

# Examples:
aps catalog generate                           # Uses ./aps.yaml, outputs ./aps.catalog.yaml
aps catalog generate --manifest ~/rules/aps.yaml
aps catalog generate --output custom-catalog.yaml
```

The generate command:

1. Discovers and loads the manifest
2. Enumerates all individual assets from each source
3. Extracts descriptions from asset files
4. Writes `aps.catalog.yaml` alongside the manifest

### Files

- `src/catalog.rs` - Catalog, CatalogEntry structs, generation logic
- `src/cli.rs` - CatalogArgs, CatalogGenerateArgs
- `src/commands.rs` - cmd_catalog_generate
- `aps.catalog.yaml` - Generated catalog file (lives alongside aps.yaml)

---

## Future: Intelligent Search

The following describes proposed features for intelligent asset discovery.

### Extended Data Model

**Enriched Catalog Entry** - Additional metadata for search:

```yaml
- id: fastapi-auth
  name: FastAPI Authentication Patterns
  kind: cursor_rules
  destination: ./.cursor/rules/fastapi-auth.mdc
  short_description: JWT and OAuth2 patterns for FastAPI
  # --- Future fields ---
  category: security # One of: language, framework, security, testing, api-design, etc.
  tags: [fastapi, jwt, oauth2]
  keywords: [authentication, bearer, token, login]
  use_cases:
    - Adding user authentication to REST APIs
    - Implementing role-based access control
  triggers: # Natural language phrases
    - "add authentication to my API"
    - "implement JWT tokens"
```

**Why these fields?**

- `triggers`: Written as phrases humans say, weighted highest in search
- `keywords`: Technical terms for exact matching
- `tags`/`category`: Filtering and grouping
- `use_cases`: Longer descriptions for context

### Search Algorithm

**Inverted Index** - Built at load time for O(1) lookups:

```text
"jwt"    → [(entry_1, triggers, 2.5), (entry_3, keywords, 2.0)]
"fastapi" → [(entry_1, triggers, 2.5), (entry_2, tags, 2.0)]
```

**Field Weights:**

| Field             | Weight | Rationale                          |
| ----------------- | ------ | ---------------------------------- |
| name              | 3.0    | Exact name matches are intentional |
| triggers          | 2.5    | Natural language task descriptions |
| tags              | 2.0    | Curated relevance signals          |
| keywords          | 2.0    | Technical term matching            |
| use_cases         | 1.5    | Contextual but verbose             |
| category          | 1.5    | Broad classification               |
| short_description | 1.0    | General content, lowest signal     |

**Scoring (TF-IDF style):**

```text
score(entry, query) = Σ term_frequency(t) × field_weight(f) × idf(t)

where idf(t) = log(total_entries / entries_containing_t)
```

Rare terms score higher. A match on "JWT" (few entries) beats a match on "API" (many entries).

**Tokenization:**

1. Lowercase and split on non-alphanumeric
2. Remove stop words (the, a, to, for, etc.)
3. Apply simple stemming (authentication → authent)

### Future CLI Commands

```bash
# Core suggestion command
aps suggest "add JWT auth to FastAPI" --threshold 0.5 --limit 5

# Catalog management
aps catalog list [--category security] [--tag jwt]
aps catalog search "authentication patterns"
aps catalog info <asset-id>

# LLM-assisted catalog enrichment
aps catalog generate --output prompt
# Outputs prompt for LLM to enrich with metadata

# Project context detection
aps context --format mcp
# Auto-detects tech stack, suggests relevant assets
```

### Integration Points

**Claude Code Hooks:**

```bash
# .claude/hooks/on-session-start.sh
aps context --format mcp --auto-apply
```

**MCP Output Format:**

```json
{
  "suggestions": [...],
  "project_context": {"technologies": ["python", "fastapi"]},
  "confidence": 0.85
}
```

### Additional Considerations

- **Semantic search**: Embeddings would improve matching but add dependencies
- **Usage learning**: Track which suggestions users accept to improve ranking
- **Automatic triggers**: LLM generates triggers from file content analysis
