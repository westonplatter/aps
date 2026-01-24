use console::{style, Style};
use std::path::Path;

/// Status of a sync operation for display purposes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncStatus {
    /// Entry was synced (symlinked)
    Synced,
    /// Entry was copied (not symlinked)
    Copied,
    /// Entry is already current (no changes needed)
    Current,
    /// Entry is current but has an upgrade available
    Upgradable,
    /// Entry had warnings during sync
    Warning,
    /// Entry failed to sync (reserved for future use)
    #[allow(dead_code)]
    Error,
}

/// Display item for sync output
#[derive(Debug)]
pub struct SyncDisplayItem {
    pub id: String,
    pub dest_path: String,
    pub status: SyncStatus,
    pub message: Option<String>,
}

impl SyncDisplayItem {
    pub fn new(id: String, dest_path: String, status: SyncStatus) -> Self {
        Self {
            id,
            dest_path,
            status,
            message: None,
        }
    }

    pub fn with_message(mut self, message: String) -> Self {
        self.message = Some(message);
        self
    }
}

/// Format a destination path for display, making it relative and concise
fn format_dest_path(dest_path: &str, manifest_dir: &Path) -> String {
    let manifest_str = manifest_dir.to_string_lossy();

    // Try to make the path relative to manifest directory
    if let Some(relative) = dest_path.strip_prefix(manifest_str.as_ref()) {
        let trimmed = relative.trim_start_matches('/');
        if trimmed.is_empty() {
            ".".to_string()
        } else {
            // Clean up any leading ./ from the trimmed path
            let cleaned = trimmed.trim_start_matches("./");
            format!("./{}", cleaned)
        }
    } else {
        dest_path.to_string()
    }
}

/// Print all sync results in the new styled format
pub fn print_sync_results(items: &[SyncDisplayItem], manifest_path: &Path, dry_run: bool) {
    let manifest_dir = manifest_path.parent().unwrap_or(Path::new("."));

    // Header
    let manifest_display = manifest_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| manifest_path.to_string_lossy().to_string());

    if dry_run {
        println!(
            "{} {} {}",
            style("Syncing from").dim(),
            style(&manifest_display).cyan(),
            style("[dry-run]").yellow().bold()
        );
    } else {
        println!(
            "{} {}",
            style("Syncing from").dim(),
            style(&manifest_display).cyan()
        );
    }
    println!();

    // Styles
    let green = Style::new().green();
    let dim = Style::new().dim();
    let yellow = Style::new().yellow();
    let orange = Style::new().color256(208); // Orange color for upgradable
    let red = Style::new().red();

    // Calculate column widths for alignment
    let max_id_len = items.iter().map(|i| i.id.len()).max().unwrap_or(0);
    let max_dest_len = items
        .iter()
        .map(|i| format_dest_path(&i.dest_path, manifest_dir).len())
        .max()
        .unwrap_or(0);

    // Print each entry
    for item in items {
        let (badge, badge_style, status_text, status_style): (&str, &Style, &str, &Style) =
            match item.status {
                SyncStatus::Synced => ("✓", &green, "[synced]", &green),
                SyncStatus::Copied => ("✓", &green, "[copied]", &green),
                SyncStatus::Current => ("·", &dim, "[current]", &dim),
                SyncStatus::Upgradable => ("↑", &orange, "[upgrade available]", &orange),
                SyncStatus::Warning => ("!", &yellow, "[warning]", &yellow),
                SyncStatus::Error => ("✗", &red, "[error]", &red),
            };

        let dest_display = format_dest_path(&item.dest_path, manifest_dir);

        // Format: "  ✓ entry-id         → ./dest/path     [synced]"
        let id_style = match item.status {
            SyncStatus::Current => Style::new().dim(),
            SyncStatus::Upgradable => Style::new().color256(208),
            SyncStatus::Warning => Style::new().yellow(),
            SyncStatus::Error => Style::new().red(),
            _ => Style::new().white(),
        };

        println!(
            "  {} {:<width_id$} {} {:<width_dest$} {}",
            badge_style.apply_to(badge),
            id_style.apply_to(&item.id),
            dim.apply_to("→"),
            dim.apply_to(&dest_display),
            status_style.apply_to(status_text),
            width_id = max_id_len,
            width_dest = max_dest_len,
        );

        // Print message if present (for warnings/errors/upgrades)
        if let Some(ref msg) = item.message {
            let msg_style = match item.status {
                SyncStatus::Upgradable => &orange,
                SyncStatus::Warning => &yellow,
                SyncStatus::Error => &red,
                _ => &dim,
            };
            println!("      {}", msg_style.apply_to(msg));
        }
    }

    println!();
}

/// Print the summary line after sync
pub fn print_sync_summary(
    synced_count: usize,
    copied_count: usize,
    current_count: usize,
    upgradable_count: usize,
    warning_count: usize,
    orphan_count: usize,
    dry_run: bool,
) {
    let green = Style::new().green();
    let dim = Style::new().dim();
    let orange = Style::new().color256(208);
    let yellow = Style::new().yellow();

    let mut parts = Vec::new();

    // Combine synced and copied into "installed"
    let installed_count = synced_count + copied_count;

    if dry_run {
        if installed_count > 0 {
            parts.push(format!(
                "{} {}",
                style(installed_count).green(),
                style("would sync").dim()
            ));
        }
        if current_count > 0 {
            parts.push(format!(
                "{} {}",
                dim.apply_to(current_count),
                dim.apply_to("current")
            ));
        }
    } else {
        if installed_count > 0 {
            parts.push(format!(
                "{} {}",
                green.apply_to(installed_count),
                green.apply_to("synced")
            ));
        }
        if current_count > 0 {
            parts.push(format!(
                "{} {}",
                dim.apply_to(current_count),
                dim.apply_to("current")
            ));
        }
    }

    if upgradable_count > 0 {
        parts.push(format!(
            "{} {}",
            orange.apply_to(upgradable_count),
            orange.apply_to(if upgradable_count == 1 {
                "upgrade available"
            } else {
                "upgrades available"
            })
        ));
    }

    if warning_count > 0 {
        parts.push(format!(
            "{} {}",
            yellow.apply_to(warning_count),
            yellow.apply_to(if warning_count == 1 {
                "warning"
            } else {
                "warnings"
            })
        ));
    }

    if orphan_count > 0 {
        parts.push(format!(
            "{} {}",
            dim.apply_to(orphan_count),
            dim.apply_to("orphans cleaned")
        ));
    }

    if !parts.is_empty() {
        println!("{}", parts.join(", "));
    }

    // Print upgrade hint if there are upgradable entries
    if upgradable_count > 0 {
        println!(
            "\n{} {}",
            orange.apply_to("↑"),
            orange.apply_to("Run `aps sync --upgrade` to update to latest versions.")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_format_dest_path_relative() {
        let manifest_dir = PathBuf::from("/home/user/project");
        let dest = "/home/user/project/.cursor/rules/";
        assert_eq!(format_dest_path(dest, &manifest_dir), "./.cursor/rules/");
    }

    #[test]
    fn test_format_dest_path_absolute() {
        let manifest_dir = PathBuf::from("/home/user/project");
        let dest = "/other/path/file.md";
        assert_eq!(format_dest_path(dest, &manifest_dir), "/other/path/file.md");
    }

    #[test]
    fn test_sync_display_item_with_message() {
        let item = SyncDisplayItem::new(
            "test-entry".to_string(),
            "/path/to/dest".to_string(),
            SyncStatus::Warning,
        )
        .with_message("Missing SKILL.md".to_string());

        assert_eq!(item.message, Some("Missing SKILL.md".to_string()));
    }
}
