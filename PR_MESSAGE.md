# Hey Weston 👋

> [!NOTE]
> First, thank you for building and maintaining this repository — it’s been genuinely useful for distributing rules and skills across multiple projects based on different needs.
> Second, I am not a Rust expert, so please bear with me if I made any mistakes. I tried to follow the existing code style and conventions.

While using the tool, I found a couple of areas where additional functionality would be extremely valuable:

## 🪝 Hooks (Cursor, Claude, ...)

The Hooks documentations for the designated IDEs enforce the scripts running the Hooks to be marked as executable with `chmod +x`.

I extended the already existing pipeline to support this functionality with new **Asset Types**:

- `cursor_hooks`
- `claude_hooks`

**Implementation Details:**

- **Smart Syncing**: Hooks are installed using a merge strategy that preserves existing files in `.cursor/` or `.claude/` directories (like extensions or other configs), only overwriting the specific hook scripts and configuration.
- **Config Management**: Automatically syncs the associated configuration file (`.cursor/hooks.json` or `.claude/settings.json`) from the source alongside the hooks directory.
- **Executable Permissions**: Post-installation step recursively applies `chmod +x` to all `.sh` files, ensuring hooks work immediately.
- **Validation**: Enforces correct structure where configuration files reside in the parent directory of the hooks folder, and verifies referenced scripts exist.
- **Catalog**: Updated generation to enumerate individual hook scripts.

## 🤖 Copilot Rule convert

...
