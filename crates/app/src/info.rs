//! Little views about the open document: the outline picker's rows, the
//! footer's "what's in here" panel and the line Nib (`pet.rs`) says there.

use gpui_kit::{component::*, prelude::FluentBuilder, *};
use markdown_core::{Heading, Stats};
use std::rc::Rc;

/// `12345` → `12,345`.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `1234` words → `1.2k`, so the outline's right edge stays narrow.
fn compact(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f32 / 1000.)
    } else {
        n.to_string()
    }
}

/// One outline row: indented by depth, with its level and how much it holds.
pub fn heading_row(heading: &Heading, top: u8, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let depth = heading.level.saturating_sub(top) as f32;
    let major = heading.level == top;
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .pl(px(depth * 18.))
        .child(
            div()
                .flex_shrink_0()
                .w(px(24.))
                .py(px(1.))
                .rounded_sm()
                .text_center()
                .text_size(px(10.))
                .font_family("Menlo")
                .font_weight(FontWeight::SEMIBOLD)
                .map(|d| {
                    if major {
                        d.bg(theme.primary.opacity(0.14)).text_color(theme.primary)
                    } else {
                        d.border_1()
                            .border_color(theme.border)
                            .text_color(theme.muted_foreground)
                    }
                })
                .child(format!("H{}", heading.level)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(major, |d| d.font_weight(FontWeight::SEMIBOLD))
                .when(heading.level > top + 1, |d| {
                    d.text_color(theme.muted_foreground)
                })
                .child(heading.text.clone()),
        )
        .when(heading.words > 0, |d| {
            d.child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} w", compact(heading.words))),
            )
        })
        .into_any_element()
}

/// Keyboard hints for a picker footer: each key in a little keycap.
pub fn key_hints(hints: &[(&str, &str)], cx: &App) -> AnyElement {
    let theme = cx.theme();
    let mut row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_4()
        .gap_y_1()
        .px_3()
        .py_2()
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.muted.opacity(0.5))
        .text_xs()
        .text_color(theme.muted_foreground);
    for (keys, label) in hints {
        let mut hint = div().flex().items_center().gap_1();
        for key in keys.split(' ') {
            hint = hint.child(
                div()
                    .min_w(px(18.))
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .text_center()
                    .child(key.to_string()),
            );
        }
        row = row.child(hint.child(div().ml_0p5().child(label.to_string())));
    }
    row.into_any_element()
}

/// What Nib thinks of the document.
pub fn mood(stats: &Stats) -> String {
    let left = stats.tasks - stats.tasks_done;
    if stats.words == 0 && stats.code_blocks == 0 {
        "Nothing to nibble on yet.".into()
    } else if stats.tasks > 0 && left == 0 {
        "Every box is ticked. Snack time!".into()
    } else if left > 0 {
        format!(
            "{left} task{} still open. I believe in you.",
            if left == 1 { "" } else { "s" }
        )
    } else if stats.code_lines > stats.words / 3 {
        "Lots of code in here. Crunchy.".into()
    } else if stats.reading_minutes() >= 15 {
        "A long one. Get comfy.".into()
    } else if stats.reading_minutes() <= 1 {
        "A quick bite.".into()
    } else {
        format!("About {} minutes of reading. Yum.", stats.reading_minutes())
    }
}

/// Jumps the focused pane to the heading at this index.
pub type OnJump = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// The footer's document panel: Nib, the headline numbers, what the file is
/// made of, and its top-level contents.
pub fn panel(
    stats: &Stats,
    headings: &[Heading],
    on_jump: OnJump,
    nib: AnyElement,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let (muted, border, hover, primary) = (
        theme.muted_foreground,
        theme.border,
        theme.accent,
        theme.primary,
    );
    let section = |label: &'static str| {
        div()
            .pt_3()
            .pb_1()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(muted)
            .child(label)
    };
    let tile = |value: String, label: &'static str| {
        div()
            .flex_1()
            .px_2()
            .py_1p5()
            .rounded_md()
            .bg(theme.muted)
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(value),
            )
            .child(div().text_xs().text_color(muted).child(label))
    };
    let sections: usize = stats.headings.iter().sum();

    let mut panel = div()
        .w(px(300.))
        .p_1()
        .flex()
        .flex_col()
        .text_sm()
        .child(nib)
        .child(
            div()
                .pt_3()
                .flex()
                .gap_2()
                .child(tile(thousands(stats.words), "words"))
                .child(tile(format!("{}", stats.reading_minutes()), "min read"))
                .child(tile(sections.to_string(), "headings")),
        );

    // What it's made of: only what's actually there.
    let parts: Vec<(String, &str)> = [
        (stats.paragraphs, "paragraph", "paragraphs"),
        (stats.list_items, "list item", "list items"),
        (stats.links, "link", "links"),
        (stats.images, "image", "images"),
        (stats.tables, "table", "tables"),
        (stats.quotes, "quote", "quotes"),
        (stats.footnotes, "footnote", "footnotes"),
    ]
    .into_iter()
    .filter(|(n, ..)| *n > 0)
    .map(|(n, one, many)| (thousands(n), if n == 1 { one } else { many }))
    .chain((stats.code_blocks > 0).then(|| {
        (
            thousands(stats.code_blocks),
            if stats.code_blocks == 1 {
                "code block"
            } else {
                "code blocks"
            },
        )
    }))
    .collect();
    if !parts.is_empty() {
        let mut grid = div().flex().flex_wrap().gap_1();
        for (n, label) in parts {
            grid = grid.child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .text_xs()
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(n))
                            .child(div().text_color(muted).child(label)),
                    ),
            );
        }
        panel = panel.child(section("INSIDE")).child(grid);
    }

    // Heading levels as a tiny bar chart.
    if sections > 0 {
        let most = *stats.headings.iter().max().unwrap_or(&1) as f32;
        let mut bars = div().flex().items_end().gap_2().h(px(56.));
        for (i, &n) in stats.headings.iter().enumerate() {
            if n == 0 && i >= stats.headings.iter().rposition(|&n| n > 0).unwrap_or(0) {
                break;
            }
            bars = bars.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_end()
                    .gap_0p5()
                    .h_full()
                    .child(div().text_size(px(10.)).text_color(muted).child(if n > 0 {
                        n.to_string()
                    } else {
                        String::new()
                    }))
                    .child(
                        div()
                            .flex_shrink_0()
                            .w_full()
                            .h(px(3. + 21. * n as f32 / most))
                            .rounded_sm()
                            .bg(primary.opacity(0.35 + 0.65 * (1. - i as f32 / 6.))),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_family("Menlo")
                            .text_color(muted)
                            .child(format!("H{}", i + 1)),
                    ),
            );
        }
        panel = panel.child(section("HEADINGS BY LEVEL")).child(bars);
    }

    if stats.tasks > 0 {
        let done = stats.tasks_done as f32 / stats.tasks as f32;
        panel = panel.child(section("TASKS")).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .h(px(6.))
                        .rounded_full()
                        .bg(theme.muted)
                        .child(
                            div()
                                .h_full()
                                .w(relative(done))
                                .rounded_full()
                                .bg(rgb(0x3fae6a)),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{} / {} done", stats.tasks_done, stats.tasks)),
                ),
        );
    }

    if !stats.languages.is_empty() {
        let mut chips = div().flex().flex_wrap().gap_1();
        for (lang, n) in stats.languages.iter().take(8) {
            chips = chips.child(
                div()
                    .px_1p5()
                    .rounded_sm()
                    .bg(theme.muted)
                    .text_xs()
                    .font_family("Menlo")
                    .child(if *n > 1 {
                        format!("{lang} ×{n}")
                    } else {
                        lang.clone()
                    }),
            );
        }
        panel = panel.child(section("CODE")).child(chips);
    }

    // The two top levels, clickable, as a quick table of contents.
    if let Some(top) = headings.iter().map(|h| h.level).min() {
        let mut toc = div()
            .id("doc-info-contents")
            .flex()
            .flex_col()
            .max_h(px(180.))
            .overflow_y_scroll();
        let shown: Vec<_> = headings
            .iter()
            .enumerate()
            .filter(|(_, h)| h.level <= top + 1)
            .collect();
        for &(ix, h) in &shown {
            let jump = on_jump.clone();
            toc = toc.child(
                div()
                    .id(("doc-info-heading", ix))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_1p5()
                    .py_0p5()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .when(h.level > top, |d| d.pl(px(18.)))
                    .child(div().flex_shrink_0().size(px(5.)).rounded_full().map(|d| {
                        if h.level == top {
                            d.bg(primary)
                        } else {
                            d.border_1().border_color(muted)
                        }
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(h.level == top, |d| d.font_weight(FontWeight::MEDIUM))
                            .child(h.text.clone()),
                    )
                    .on_click(move |_, window, cx| jump(ix, window, cx)),
            );
        }
        panel = panel.child(section("CONTENTS")).child(toc);
    }
    panel.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{Stats, compact, mood, thousands};

    #[test]
    fn numbers_read_well() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1459), "1,459");
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(compact(950), "950");
        assert_eq!(compact(1250), "1.2k");
    }

    #[test]
    fn nib_has_opinions() {
        let mut s = Stats::default();
        assert_eq!(mood(&s), "Nothing to nibble on yet.");
        s.words = 100;
        s.tasks = 3;
        s.tasks_done = 2;
        assert!(mood(&s).starts_with("1 task still open"));
        s.tasks_done = 3;
        assert!(mood(&s).contains("Snack"));
    }
}
