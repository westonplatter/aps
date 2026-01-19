# Intelligent Asset Discovery - Technical Specification

## Overview

This spec describes an intelligent suggestion system that recommends prompts, rules, and skills based on natural language task descriptions. The goal is to transform APS from a file syncer into a context-aware assistant.

## Architecture

### Data Model

**Catalog Entry** - Metadata layer on top of manifest entries:

```yaml
- id: fastapi-auth # Links to manifest entry
  name: FastAPI Authentication Patterns
  description: JWT and OAuth2 patterns for FastAPI
  kind: cursor_rules
  category: security # One of: language, framework, security, testing, api-design, etc.
  tags: [fastapi, jwt, oauth2]
  keywords: [authentication, bearer, token, login]
  use_cases:
    - Adding user authentication to REST APIs
    - Implementing role-based access control
  triggers: # Natural language phrases
    - "add authentication to my API"
    - "implement JWT tokens"
  source: { copied from manifest }
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

| Field       | Weight | Rationale                          |
| ----------- | ------ | ---------------------------------- |
| name        | 3.0    | Exact name matches are intentional |
| triggers    | 2.5    | Natural language task descriptions |
| tags        | 2.0    | Curated relevance signals          |
| keywords    | 2.0    | Technical term matching            |
| use_cases   | 1.5    | Contextual but verbose             |
| category    | 1.5    | Broad classification               |
| description | 1.0    | General content, lowest signal     |

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

### CLI Commands

```bash
# Core suggestion command
aps suggest "add JWT auth to FastAPI" --threshold 0.5 --limit 5

# Catalog management
aps catalog list [--category security] [--tag jwt]
aps catalog search "authentication patterns"
aps catalog info <asset-id>
aps catalog init    # Create empty catalog
aps catalog add     # Add entry interactively

# LLM-assisted catalog generation
aps catalog generate --manifest aps.yaml --output prompt
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

## Future Considerations

- **Semantic search**: Embeddings would improve matching but add dependencies
- **Usage learning**: Track which suggestions users accept to improve ranking
- **Automatic triggers**: LLM generates triggers from file content analysis

## Files (Proposed)

- `src/catalog.rs` - CatalogEntry, CatalogSearch, inverted index, scoring
- `src/cli.rs` - SuggestArgs, CatalogArgs, ContextArgs
- `src/commands.rs` - cmd_suggest, cmd_catalog, cmd_context
- `aps-catalog.yaml` - User's catalog file (lives alongside aps.yaml)
