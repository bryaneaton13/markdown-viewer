//! Update check. Asks GitHub for the latest release 10 seconds after launch
//! and every 12 hours while "Check for Updates Automatically" is on; "Check
//! for Updates…" asks at once. A newer version shows a button in the status
//! bar that opens its release page. Nothing is downloaded or installed, and
//! the only request is that one GET (no identifiers are sent).
//!
//! The request goes through the system `curl` (built into macOS, Windows 10+
//! and most Linux systems), so the app carries no TLS stack of its own.
//!
//! Automatic checks are off in debug builds and when
//! `MARKDOWN_VIEWER_NO_UPDATE_CHECK` is set. `MARKDOWN_VIEWER_UPDATE_URL`
//! points the check at another JSON document with the same `tag_name` and
//! `html_url` fields, for testing (a `file://` URL works).

use serde::Deserialize;
use std::{process::Command, time::Duration};

const LATEST: &str = "https://api.github.com/repos/bryaneaton13/markdown-viewer/releases/latest";
pub const RELEASES: &str = "https://github.com/bryaneaton13/markdown-viewer/releases";
pub const FIRST_CHECK: Duration = Duration::from_secs(10);
pub const INTERVAL: Duration = Duration::from_secs(12 * 3600);
/// How long "up to date" or a failed manual check stays in the status bar.
pub const MESSAGE_FOR: Duration = Duration::from_secs(5);

/// The changelog, compiled in: "What's new" never needs the network.
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// This version's notes from the changelog, if it has a section.
pub fn notes() -> Option<String> {
    markdown_core::release_notes(CHANGELOG, env!("CARGO_PKG_VERSION"))
}

/// The "What's new" page: this version's notes, or a pointer to them all.
pub fn notes_page() -> String {
    let version = env!("CARGO_PKG_VERSION");
    match notes() {
        Some(notes) => {
            format!("# What's new in {version}\n\n{notes}\n\n---\n\n[All releases]({RELEASES})\n")
        }
        None => format!(
            "# Markdown Viewer {version}\n\nThis version has no release notes yet.\n\n[All releases]({RELEASES})\n"
        ),
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum State {
    #[default]
    Idle,
    /// `manual`: the person asked, so the result is shown either way.
    Checking {
        manual: bool,
    },
    UpToDate,
    Available {
        version: String,
        url: String,
    },
    Failed,
}

#[derive(Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

pub fn automatic_allowed() -> bool {
    !cfg!(debug_assertions)
        && !cfg!(test)
        && std::env::var_os("MARKDOWN_VIEWER_NO_UPDATE_CHECK").is_none()
}

/// Read GitHub's "latest release" answer.
pub fn parse(json: &[u8]) -> Result<Release, String> {
    let release: GithubRelease =
        serde_json::from_slice(json).map_err(|e| format!("unexpected response: {e}"))?;
    if release.draft || release.prerelease {
        return Err("latest release is a draft or prerelease".into());
    }
    let version = release.tag_name.trim_start_matches(['v', 'V']).to_owned();
    Ok(Release {
        url: release
            .html_url
            .unwrap_or_else(|| format!("{RELEASES}/tag/{}", release.tag_name)),
        version,
    })
}

/// Fetch the latest release. Blocking: run it off the main thread.
pub fn fetch() -> Result<Release, String> {
    let url = std::env::var("MARKDOWN_VIEWER_UPDATE_URL").unwrap_or_else(|_| LATEST.to_owned());
    let mut command = Command::new("curl");
    command.args([
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--max-time",
        "20",
        "--proto",
        "=https,file",
        "--header",
        "Accept: application/vnd.github+json",
        "--user-agent",
        concat!("MarkdownViewer/", env!("CARGO_PKG_VERSION")),
        &url,
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    parse(&output.stdout)
}

/// What a finished check means for the running version.
pub fn outcome(result: Result<Release, String>, manual: bool) -> State {
    match result {
        Ok(release) if markdown_core::is_newer(&release.version, env!("CARGO_PKG_VERSION")) => {
            log::info!("Update available: {}", release.version);
            State::Available {
                version: release.version,
                url: release.url,
            }
        }
        Ok(release) => {
            log::info!("Up to date (latest release {})", release.version);
            if manual { State::UpToDate } else { State::Idle }
        }
        Err(e) => {
            log::warn!("Update check failed: {e}");
            if manual { State::Failed } else { State::Idle }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_github_releases() {
        let release = parse(
            br#"{"tag_name":"v9.1.0","html_url":"https://github.com/x/y/releases/tag/v9.1.0","draft":false,"prerelease":false,"assets":[]}"#,
        )
        .unwrap();
        assert_eq!(release.version, "9.1.0");
        assert!(release.url.ends_with("/v9.1.0"));
        assert!(parse(br#"{"tag_name":"v9.2.0","prerelease":true}"#).is_err());
        assert!(parse(b"<html>").is_err());
    }

    #[test]
    fn outcomes() {
        let newer = || {
            Ok(Release {
                version: "999.0.0".into(),
                url: "u".into(),
            })
        };
        let same = || {
            Ok(Release {
                version: env!("CARGO_PKG_VERSION").into(),
                url: "u".into(),
            })
        };
        assert!(matches!(outcome(newer(), false), State::Available { .. }));
        assert_eq!(outcome(same(), false), State::Idle);
        assert_eq!(outcome(same(), true), State::UpToDate);
        assert_eq!(outcome(Err("x".into()), false), State::Idle);
        assert_eq!(outcome(Err("x".into()), true), State::Failed);
    }

    #[test]
    fn fetches_from_an_override_url() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("latest.json");
        std::fs::write(
            &file,
            r#"{"tag_name":"v1.2.3","html_url":"https://example.com/r"}"#,
        )
        .unwrap();
        let url = url::Url::from_file_path(&file).unwrap();
        // SAFETY: tests in this module don't read the variable concurrently.
        unsafe { std::env::set_var("MARKDOWN_VIEWER_UPDATE_URL", url.as_str()) };
        let release = fetch();
        unsafe { std::env::remove_var("MARKDOWN_VIEWER_UPDATE_URL") };
        assert_eq!(
            release.unwrap(),
            Release {
                version: "1.2.3".into(),
                url: "https://example.com/r".into()
            }
        );
    }
}
