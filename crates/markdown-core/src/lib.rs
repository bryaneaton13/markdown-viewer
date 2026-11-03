//! UI-independent, read-only document preparation shared with the Quick Look extension.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};
use std::{
    ffi::{CString, c_char},
    ops::Range,
    path::{Path, PathBuf},
};

pub const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Heading {
    pub text: String,
    pub id: String,
    /// 1 for `#`, up to 6.
    pub level: u8,
    pub range: Range<usize>,
    /// Prose words from this heading to the next one.
    pub words: usize,
}

/// What a document is made of, for the info panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Prose words: code, URLs and Markdown syntax don't count.
    pub words: usize,
    pub characters: usize,
    pub lines: usize,
    /// Heading counts by level, `#` first.
    pub headings: [usize; 6],
    pub paragraphs: usize,
    pub list_items: usize,
    pub tasks: usize,
    pub tasks_done: usize,
    pub links: usize,
    pub images: usize,
    pub tables: usize,
    pub quotes: usize,
    pub footnotes: usize,
    pub code_blocks: usize,
    pub code_lines: usize,
    /// Fenced code languages, most used first.
    pub languages: Vec<(String, usize)>,
}

impl Stats {
    /// Minutes at an unhurried 230 words a minute, at least one.
    pub fn reading_minutes(&self) -> usize {
        self.words.div_ceil(230).max(1)
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    pub source: String,
    pub body: String,
    pub metadata: Option<String>,
    pub metadata_error: Option<String>,
    pub headings: Vec<Heading>,
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("md") || s.eq_ignore_ascii_case("markdown"))
}

pub fn read_document(path: &Path) -> Result<Document, String> {
    use std::io::Read;
    if !is_markdown(path) {
        return Err("Only .md and .markdown files are supported.".into());
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("This prototype supports Markdown files up to 8 MiB.".into());
    }
    let source =
        String::from_utf8(bytes).map_err(|_| "This file is not UTF-8 Markdown.".to_string())?;
    Ok(Document::parse(source))
}

impl Document {
    pub fn parse(source: String) -> Self {
        let text = source.strip_prefix('\u{feff}').unwrap_or(&source);
        let mut body = text.to_owned();
        let mut metadata = None;
        let mut metadata_error = None;
        let mut lines = text.split_inclusive('\n');
        if let Some(first) = lines
            .next()
            .filter(|l| l.trim_end_matches(['\r', '\n']) == "---")
        {
            let start = first.len();
            let mut offset = start;
            let mut closed = false;
            for line in lines {
                if matches!(line.trim_end_matches(['\r', '\n']), "---" | "...") {
                    let raw = text[start..offset].to_owned();
                    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&raw) {
                        Ok(serde_yaml_ng::Value::Mapping(_)) | Ok(serde_yaml_ng::Value::Null) => {}
                        Ok(_) => {
                            metadata_error = Some(
                                "Properties must be a YAML mapping; original metadata shown below."
                                    .into(),
                            )
                        }
                        Err(e) => metadata_error = Some(format!("Invalid YAML: {e}")),
                    }
                    metadata = Some(raw);
                    body = text[offset + line.len()..].to_owned();
                    closed = true;
                    break;
                }
                offset += line.len();
            }
            if !closed {
                metadata_error =
                    Some("Unclosed frontmatter; the complete document is shown.".into());
            }
        }
        let mut headings = Vec::new();
        let mut current: Option<(String, u8, Range<usize>)> = None;
        let mut counts = std::collections::HashMap::<String, usize>::new();
        for (event, range) in Parser::new_ext(&body, Options::all()).into_offset_iter() {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    current = Some((String::new(), level as u8, range))
                }
                Event::Text(t) | Event::Code(t) => {
                    if let Some((name, _, _)) = &mut current {
                        name.push_str(&t);
                    }
                }
                Event::End(TagEnd::Heading(_)) => {
                    if let Some((text, level, range)) = current.take() {
                        let slug = text
                            .to_lowercase()
                            .chars()
                            .filter(|c| {
                                c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_'
                            })
                            .collect::<String>()
                            .replace(' ', "-");
                        let count = counts.entry(slug.clone()).or_default();
                        let id = if *count == 0 {
                            slug
                        } else {
                            format!("{slug}-{count}")
                        };
                        *count += 1;
                        headings.push(Heading {
                            text,
                            id,
                            level,
                            range,
                            words: 0,
                        });
                    }
                }
                _ => {}
            }
        }
        // Each heading owns the prose up to the next one.
        let ends: Vec<usize> = headings
            .iter()
            .skip(1)
            .map(|h| h.range.start)
            .chain([body.len()])
            .collect();
        for (heading, end) in headings.iter_mut().zip(ends) {
            heading.words = prose_words(&body[heading.range.end.min(end)..end]);
        }
        Self {
            source,
            body,
            metadata,
            metadata_error,
            headings,
        }
    }

    pub fn stats(&self) -> Stats {
        let mut s = Stats {
            characters: self.body.chars().count(),
            lines: self.body.lines().count(),
            ..Stats::default()
        };
        let mut languages = std::collections::HashMap::<String, usize>::new();
        let mut code = false;
        for event in Parser::new_ext(&self.body, Options::all()) {
            match event {
                Event::Start(Tag::Heading { level, .. }) => s.headings[level as usize - 1] += 1,
                Event::Start(Tag::Paragraph) => s.paragraphs += 1,
                Event::Start(Tag::Item) => s.list_items += 1,
                Event::TaskListMarker(done) => {
                    s.tasks += 1;
                    s.tasks_done += done as usize;
                }
                Event::Start(Tag::Link { .. }) => s.links += 1,
                Event::Start(Tag::Image { .. }) => s.images += 1,
                Event::Start(Tag::Table(_)) => s.tables += 1,
                Event::Start(Tag::BlockQuote(_)) => s.quotes += 1,
                Event::Start(Tag::FootnoteDefinition(_)) => s.footnotes += 1,
                Event::Start(Tag::CodeBlock(kind)) => {
                    code = true;
                    s.code_blocks += 1;
                    if let pulldown_cmark::CodeBlockKind::Fenced(info) = kind
                        && let Some(lang) = info.split([' ', ',', '{']).next()
                        && !lang.is_empty()
                    {
                        *languages.entry(lang.to_lowercase()).or_default() += 1;
                    }
                }
                Event::End(TagEnd::CodeBlock) => code = false,
                Event::Text(t) if code => s.code_lines += t.lines().count(),
                Event::Text(t) => s.words += t.split_whitespace().count(),
                _ => {}
            }
        }
        s.languages = languages.into_iter().collect();
        s.languages
            .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        s
    }

    pub fn source_markdown(&self) -> String {
        // A delimiter longer than any run in the source cannot be closed by its content.
        let longest = self
            .source
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0);
        let fence = "`".repeat(longest.max(2) + 1);
        format!("{fence}markdown\n{}\n{fence}", self.source)
    }
}

/// Words of running text, skipping code blocks and Markdown syntax.
fn prose_words(markdown: &str) -> usize {
    let mut code = false;
    let mut words = 0;
    for event in Parser::new_ext(markdown, Options::all()) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code = true,
            Event::End(TagEnd::CodeBlock) => code = false,
            Event::Text(t) if !code => words += t.split_whitespace().count(),
            _ => {}
        }
    }
    words
}

pub enum Link {
    Local(PathBuf, Option<String>),
    External(String),
    Blocked,
}

pub fn resolve_link(base: &Path, link: &str) -> Link {
    let Ok(base) = url::Url::from_file_path(base) else {
        return Link::Blocked;
    };
    let Ok(url) = base.join(link) else {
        return Link::Blocked;
    };
    match url.scheme() {
        "file" => url
            .to_file_path()
            .map(|p| Link::Local(p, url.fragment().map(str::to_owned)))
            .unwrap_or(Link::Blocked),
        "https" | "http" | "mailto" => Link::External(url.to_string()),
        _ => Link::Blocked,
    }
}

pub fn find_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    // Regex handles Unicode case folding while retaining original UTF-8 byte offsets.
    regex::RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
        .map(|r| r.find_iter(text).take(2000).map(|m| m.range()).collect())
        .unwrap_or_default()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Offline HTML for the sandboxed Finder provider. Images become alt text: the
/// extension is granted the document, not arbitrary access to adjacent files.
pub fn preview_html(source: &str) -> String {
    let doc = Document::parse(source.to_owned());
    let events = Parser::new_ext(
        &doc.body,
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES,
    )
    .filter_map(|event| match event {
        Event::Html(_) | Event::InlineHtml(_) => None,
        Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => None,
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let safe = dest_url.starts_with("https://")
                || dest_url.starts_with("http://")
                || dest_url.starts_with('#');
            Some(Event::Start(Tag::Link {
                link_type,
                dest_url: if safe { dest_url } else { "#".into() },
                title,
                id,
            }))
        }
        other => Some(other),
    });
    let mut body = String::new();
    html::push_html(&mut body, events);
    let properties = doc
        .metadata
        .map(|m| {
            format!(
                "<details><summary>Properties</summary><pre>{}</pre></details>",
                escape(&m)
            )
        })
        .unwrap_or_default();
    let error = doc
        .metadata_error
        .map(|m| format!("<aside>{}</aside>", escape(&m)))
        .unwrap_or_default();
    format!(
        r##"<!doctype html><html><head><meta charset="utf-8"><meta name="color-scheme" content="light dark"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'"><style>
html {{ color-scheme: light dark; }} body {{ font: 16px/1.65 -apple-system, sans-serif; margin: 32px auto; padding: 0 32px; max-width: 850px; overflow-wrap: break-word; }}
h1,h2,h3 {{ line-height:1.25; letter-spacing:-.025em; }} h1 {{ font-size:32px; }} h2 {{ margin-top:1.6em; }} a {{ color:#3983d6; }} pre {{ overflow:auto; padding:16px; background:light-dark(#f3f4f6,#24272c); border-radius:8px; }} code {{ font: .9em ui-monospace, monospace; }} blockquote {{ border-left:3px solid #6d8bad; margin-left:0; padding-left:20px; color:light-dark(#596372,#aeb9c8); }} table {{ border-collapse:collapse; display:block; overflow:auto; }} th,td {{ border:1px solid #8885; padding:8px 14px; }} details,aside {{ color:light-dark(#596372,#aeb9c8); font-size:13px; }} input {{ pointer-events:none; }}
</style></head><body>{error}{properties}{body}</body></html>"##
    )
}

/// # Safety
/// `bytes` must reference `len` readable bytes. The result must be freed once
/// with `markdown_preview_free`. Never unwinds across the Swift boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn markdown_preview_html(bytes: *const u8, len: usize) -> *mut c_char {
    if bytes.is_null() || len as u64 > MAX_BYTES {
        return std::ptr::null_mut();
    }
    std::panic::catch_unwind(|| {
        let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
        let source = std::str::from_utf8(bytes).ok()?;
        CString::new(preview_html(source))
            .ok()
            .map(CString::into_raw)
    })
    .ok()
    .flatten()
    .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// `ptr` must be null or an unfreed result of `markdown_preview_html`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn markdown_preview_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_boundaries_and_invalid_yaml() {
        let d = Document::parse("\u{feff}---\r\ntitle: Read\r\n---\r\n# Hello\n---\n".into());
        assert_eq!(d.metadata.as_deref(), Some("title: Read\r\n"));
        assert!(d.body.starts_with("# Hello"));
        let d = Document::parse("---\ntitle: [\n---\nBody".into());
        assert!(d.metadata_error.is_some());
        assert_eq!(d.body, "Body");
        let d = Document::parse("---\ntitle: unfinished".into());
        assert_eq!(d.body, d.source);
        assert!(d.metadata_error.is_some());
    }
    #[test]
    fn headings_skip_code_and_disambiguate() {
        let d = Document::parse("# Hello\n```\n# Fake\n```\n## Hello\n".into());
        assert_eq!(
            d.headings.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
            ["hello", "hello-1"]
        );
    }
    #[test]
    fn headings_know_their_level_and_section_words() {
        let d = Document::parse(
            "# One\nfour words of prose\n## Two\n```\nnot counted here\n```\nthree more words\n"
                .into(),
        );
        let h: Vec<_> = d.headings.iter().map(|h| (h.level, h.words)).collect();
        assert_eq!(h, [(1, 4), (2, 3)]);
    }
    #[test]
    fn stats_break_the_document_down() {
        let d = Document::parse(
            "---\ntitle: x\n---\n# Title\n\nSome *real* words, [a link](x.md).\n\n## Tasks\n- [x] done\n- [ ] todo\n- plain\n\n> quoted\n\n```rust\nfn a() {}\nfn b() {}\n```\n\n```Rust\n1\n```\n\n~~~\nx\n~~~\n\n| A |\n|---|\n| 1 |\n\n![pic](p.png)\n".into(),
        );
        let s = d.stats();
        assert_eq!(s.headings, [1, 1, 0, 0, 0, 0]);
        assert_eq!((s.tasks, s.tasks_done, s.list_items), (2, 1, 3));
        assert_eq!((s.links, s.images, s.tables, s.quotes), (1, 1, 1, 1));
        assert_eq!((s.code_blocks, s.code_lines), (3, 4));
        assert_eq!(s.languages, [("rust".to_string(), 2)]);
        // Title, Some real words a link, Tasks, done todo plain, quoted, A 1, pic
        assert_eq!(s.words, 15);
        assert_eq!(s.reading_minutes(), 1);
    }
    #[test]
    fn unicode_search_returns_original_offsets() {
        let text = "İstanbul café CAFÉ";
        for r in find_ranges(text, "café") {
            assert!(text.is_char_boundary(r.start));
            assert_eq!(text[r].chars().count(), 4);
        }
        assert_eq!(find_ranges(text, "café").len(), 2);
    }
    #[test]
    fn preview_is_offline_and_escapes_metadata() {
        let html = preview_html(
            "---\ntitle: '<script>bad</script>'\n---\n# Hi\n<script>alert(1)</script>\n\n![alt](https://tracker.test/a.png)\n\n[x](javascript:bad)\n\n| A | B |\n|---|---|\n|1|2|",
        );
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<img"));
        assert!(!html.contains("javascript:"));
        assert!(html.contains("<table>"));
        assert!(html.contains("&lt;script&gt;"));
    }
    #[test]
    fn reads_never_change_source_and_reject_other_formats() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("A.MD");
        let source = b"---\ntitle: Test\n---\n- [ ] untouched\n";
        std::fs::write(&path, source).unwrap();
        let d = read_document(&path).unwrap();
        let _ = d.source_markdown();
        let _ = preview_html(&d.source);
        assert_eq!(std::fs::read(&path).unwrap(), source);
        assert!(read_document(&dir.path().join("a.txt")).is_err());
    }
    #[test]
    fn local_links_decode_and_schemes_are_limited() {
        match resolve_link(Path::new("/docs/a.md"), "next%20file.md#hello") {
            Link::Local(p, h) => {
                assert_eq!(p, Path::new("/docs/next file.md"));
                assert_eq!(h.as_deref(), Some("hello"));
            }
            _ => panic!(),
        }
        assert!(matches!(
            resolve_link(Path::new("/a.md"), "javascript:foo"),
            Link::Blocked
        ));
    }
}
