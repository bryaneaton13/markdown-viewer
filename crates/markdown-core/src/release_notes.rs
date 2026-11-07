//! Reading a version's notes out of a Keep a Changelog file, for the
//! "What's new" card shown once after an update.

/// The body of the `## [version]` section, without its heading. `None` when
/// the changelog has no section for that version or it is empty.
pub fn release_notes(changelog: &str, version: &str) -> Option<String> {
    let wanted = format!("[{version}]");
    let mut lines = changelog.lines();
    lines.find(|l| {
        l.strip_prefix("## ")
            .is_some_and(|h| h.trim_start().starts_with(&wanted))
    })?;
    let body: Vec<&str> = lines.take_while(|l| !l.starts_with("## ")).collect();
    let body = body.join("\n").trim().to_owned();
    (!body.is_empty()).then_some(body)
}

/// Up to `max` short lines for a card: each top-level bullet's bold lead
/// (`- **Tabs.** …` → "Tabs"), or its first sentence.
pub fn highlights(notes: &str, max: usize) -> Vec<String> {
    notes
        .lines()
        .filter_map(|l| l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")))
        .map(|item| {
            let item = item.trim();
            if let Some(rest) = item.strip_prefix("**")
                && let Some(end) = rest.find("**")
            {
                return rest[..end].trim().trim_end_matches(['.', ':']).to_owned();
            }
            let sentence = item.split(". ").next().unwrap_or(item);
            let plain: String = sentence
                .chars()
                .filter(|c| !matches!(c, '*' | '`'))
                .collect();
            let plain = plain.trim().trim_end_matches('.');
            if plain.chars().count() > 60 {
                let cut: String = plain.chars().take(57).collect();
                format!("{}…", cut.trim_end())
            } else {
                plain.to_owned()
            }
        })
        .filter(|s| !s.is_empty())
        .take(max)
        .collect()
}

/// Whether `a` is a newer `MAJOR.MINOR.PATCH` than `b`. A leading `v` and
/// any pre-release or build suffix are ignored; unparsable versions are never newer.
pub fn is_newer(a: &str, b: &str) -> bool {
    match (parse_version(a), parse_version(b)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let major = parts.next()??;
    let minor = parts.next().unwrap_or(Some(0))?;
    let patch = parts.next().unwrap_or(Some(0))?;
    parts.next().is_none().then_some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog\n\n## [Unreleased]\n\n- Soon.\n\n## [0.2.0] - 2026-11-10\n\n### Added\n- **Workspaces.** Each folder…\n- ⌘-click a file in the tree, or press ⌘↵ in Quick Open, to open it in a new tab. More.\n\n### Changed\n- **Tabs:** reorder\n\n## [0.1.0] - 2026-10-27\n\n- First.\n";

    #[test]
    fn sections_and_highlights() {
        let notes = release_notes(LOG, "0.2.0").unwrap();
        assert!(notes.starts_with("### Added") && notes.ends_with("- **Tabs:** reorder"));
        assert_eq!(release_notes(LOG, "0.1.0").as_deref(), Some("- First."));
        assert_eq!(release_notes(LOG, "0.3.0"), None);
        assert_eq!(
            highlights(&notes, 5),
            [
                "Workspaces",
                "⌘-click a file in the tree, or press ⌘↵ in Quick Open, to…",
                "Tabs"
            ]
        );
        assert_eq!(highlights(&notes, 1).len(), 1);
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.2.0-beta", "0.2.0"));
        assert!(!is_newer("latest", "0.1.0"));
        assert!(!is_newer("0.1.0.1", "0.1.0"));
    }
}
