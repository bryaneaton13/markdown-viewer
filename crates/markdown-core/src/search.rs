//! Search across Markdown files: the lines of a document's body that contain a
//! query, with the heading each one falls under.

use std::{ops::Range, path::Path};

/// One matching line.
#[derive(Clone, Debug, PartialEq)]
pub struct TextHit {
    /// 1-based line number in the file, counting any frontmatter.
    pub line: usize,
    /// Byte range of the match in the document body (what the reader shows).
    pub range: Range<usize>,
    /// The line, trimmed and shortened around the match.
    pub snippet: String,
    /// Where the match sits in `snippet`.
    pub snippet_match: Range<usize>,
    /// The nearest ATX heading above the line, if any.
    pub heading: Option<String>,
}

/// Longest snippet kept for a hit, in characters.
const SNIPPET_CHARS: usize = 120;
/// Context kept before the match when a line is shortened, in characters.
const LEAD_CHARS: usize = 32;

/// Where the body starts: after a closed `---` frontmatter block, else 0.
/// `text` is the source without a byte order mark.
pub fn frontmatter_end(text: &str) -> Option<usize> {
    let mut lines = text.split_inclusive('\n');
    let first = lines
        .next()
        .filter(|l| l.trim_end_matches(['\r', '\n']) == "---")?;
    let mut offset = first.len();
    for line in lines {
        if matches!(line.trim_end_matches(['\r', '\n']), "---" | "...") {
            return Some(offset + line.len());
        }
        offset += line.len();
    }
    None
}

/// Find `query` (case-insensitive, literal) in a document body. One hit per
/// match, at most `limit`. `line_offset` is how many lines precede the body.
pub fn search_body(body: &str, query: &str, line_offset: usize, limit: usize) -> Vec<TextHit> {
    let query = query.trim();
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let Ok(regex) = regex::RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
    else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    let mut heading: Option<String> = None;
    let mut fence: Option<(char, usize)> = None;
    let mut start = 0;
    for (n, raw) in body.split_inclusive('\n').enumerate() {
        let line = raw.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim_start();
        // Track fenced code so a `# comment` inside it isn't a heading.
        let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        if let Some(c) = marker {
            let run = trimmed.chars().take_while(|x| *x == c).count();
            if run >= 3 {
                match fence {
                    None => fence = Some((c, run)),
                    Some((open, len)) if open == c && run >= len => fence = None,
                    _ => {}
                }
            }
        } else if fence.is_none()
            && let Some(text) = atx_heading(trimmed)
        {
            heading = Some(text.to_owned());
        }
        for m in regex.find_iter(line) {
            let (snippet, snippet_match) = snippet(line, m.range());
            hits.push(TextHit {
                line: line_offset + n + 1,
                range: start + m.start()..start + m.end(),
                snippet,
                snippet_match,
                heading: heading.clone(),
            });
            if hits.len() >= limit {
                return hits;
            }
        }
        start += raw.len();
    }
    hits
}

/// Read a Markdown file and search its body. Unreadable, oversized and
/// non-UTF-8 files have no hits.
pub fn search_file(path: &Path, query: &str, limit: usize) -> Vec<TextHit> {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if file
        .take(crate::MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > crate::MAX_BYTES
    {
        return Vec::new();
    }
    let Ok(source) = String::from_utf8(bytes) else {
        return Vec::new();
    };
    let text = source.strip_prefix('\u{feff}').unwrap_or(&source);
    let start = frontmatter_end(text).unwrap_or(0);
    let lines_before = text[..start].matches('\n').count();
    search_body(&text[start..], query, lines_before, limit)
}

fn atx_heading(line: &str) -> Option<&str> {
    let level = line.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim_end();
    (!text.is_empty()).then_some(text)
}

/// The line trimmed, and cut to about `SNIPPET_CHARS` around the match, with
/// the match's new position.
fn snippet(line: &str, hit: Range<usize>) -> (String, Range<usize>) {
    let lead = line.len() - line.trim_start().len();
    let line = line.trim();
    let hit = hit.start.saturating_sub(lead).min(line.len())
        ..hit.end.saturating_sub(lead).min(line.len());
    if line.chars().count() <= SNIPPET_CHARS {
        return (line.to_owned(), hit);
    }
    // Start a little before the match, on a character boundary.
    let before = line[..hit.start].chars().count();
    let skip = before.saturating_sub(LEAD_CHARS);
    let from = line.char_indices().nth(skip).map_or(0, |(i, _)| i);
    let to = line[from..]
        .char_indices()
        .nth(SNIPPET_CHARS)
        .map_or(line.len(), |(i, _)| from + i)
        .max(hit.end);
    let mut out = String::new();
    let mut shift = 0isize - from as isize;
    if from > 0 {
        out.push('…');
        shift += '…'.len_utf8() as isize;
    }
    out.push_str(&line[from..to]);
    if to < line.len() {
        out.push('…');
    }
    let at = |i: usize| (i as isize + shift) as usize;
    (out, at(hit.start)..at(hit.end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_lines_with_headings_and_skips_code_headings() {
        let body = "# Intro\n\nApples are red.\n\n```sh\n# not a heading\napples\n```\n\n## Pears ##\nNo APPLES here? Apples!\n";
        let hits = search_body(body, "apples", 0, 10);
        let found: Vec<_> = hits
            .iter()
            .map(|h| (h.line, h.heading.as_deref(), &body[h.range.clone()]))
            .collect();
        assert_eq!(
            found,
            [
                (3, Some("Intro"), "Apples"),
                (7, Some("Intro"), "apples"),
                (11, Some("Pears"), "APPLES"),
                (11, Some("Pears"), "Apples"),
            ]
        );
        let h = &hits[2];
        assert_eq!(&h.snippet[h.snippet_match.clone()], "APPLES");
    }

    #[test]
    fn limit_and_blank_queries() {
        assert!(search_body("a a a", "  ", 0, 10).is_empty());
        assert_eq!(search_body("a a a", "a", 0, 2).len(), 2);
        assert!(
            search_body("a.c", ".", 0, 10).len() == 1,
            "literal, not a regex"
        );
    }

    #[test]
    fn long_lines_shorten_around_the_match() {
        let line = format!("{} needle {}", "é".repeat(200), "z".repeat(200));
        let hits = search_body(&line, "NEEDLE", 0, 1);
        let h = &hits[0];
        assert!(h.snippet.starts_with('…') && h.snippet.ends_with('…'));
        assert_eq!(&h.snippet[h.snippet_match.clone()], "needle");
        assert!(h.snippet.chars().count() <= SNIPPET_CHARS + 2);
    }

    #[test]
    fn files_skip_frontmatter_but_count_its_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        std::fs::write(&path, "\u{feff}---\ntitle: needle\n---\nbody needle\n").unwrap();
        let hits = search_file(&path, "needle", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 4);
        assert_eq!(hits[0].range, 5..11, "offsets are into the body");
        assert_eq!(frontmatter_end("---\nx\n"), None, "unclosed");
    }
}
