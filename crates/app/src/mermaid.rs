//! Mermaid diagrams. A ```` ```mermaid ```` block is laid out by rusty-mermaid
//! (pure Rust, no JavaScript or WebView) while the document parses off the
//! main thread, then drawn as an image in the reader's theme colors.
//!
//! Laying out at parse time means the diagram's size is known before it is
//! drawn, so nothing jumps when the image arrives. Colors are chosen at draw
//! time: rusty-mermaid takes well under a millisecond per diagram, so a theme
//! change just renders the SVG again and caches it by diagram and colors.
//!
//! A block that rusty-mermaid can't read, or a diagram type it doesn't
//! support, stays as the code block the author wrote.

use gpui_kit::{
    base::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast as mdast},
    component::{ActiveTheme, Sizable, button::*},
    prelude::FluentBuilder,
    *,
};
use rusty_mermaid::{Color, Theme};
use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Larger sources are left as code: a reader shouldn't stall on one block.
const MAX_SOURCE_BYTES: usize = 32 * 1024;
/// rusty-mermaid sets labels at 14px; the reader's body is 16px at the
/// default size. Diagrams scale with the reading size from there.
const BODY_PX: f32 = 16.;
/// A wide diagram shrinks to fit the page down to this fraction of its
/// natural size, then scrolls sideways so its labels stay legible.
const MIN_FIT: f32 = 0.75;
/// Upper bound on the rasterized image, in pixels, so a huge diagram can't
/// take hundreds of megabytes of texture memory.
const MAX_RASTER_PIXELS: f32 = 24_000_000.;

/// A diagram that lays out. Sizes are SVG pixels at the natural size.
#[derive(Debug)]
pub struct Diagram {
    source: String,
    hash: u64,
    kind: rusty_mermaid::DiagramKind,
    width: f32,
    height: f32,
}

fn hash_of(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Render `source` to SVG, treating a panic in the renderer as a failure:
/// a bad diagram must never take the document (or the app) down with it.
/// rusty-mermaid reads most malformed input as an empty diagram rather than
/// an error; that counts as a failure too, so the author sees their code.
fn to_svg(source: &str, theme: &Theme) -> Option<String> {
    catch_unwind(AssertUnwindSafe(|| {
        let scene = rusty_mermaid::render(source, theme).ok()?;
        (!scene.is_empty()).then(|| {
            rusty_mermaid::svg::SvgRenderer::with_theme(theme).render_themed(&scene, theme)
        })
    }))
    .ok()?
}

/// The `width` and `height` on the SVG's root element.
fn svg_size(svg: &str) -> Option<(f32, f32)> {
    let root = &svg[svg.find("<svg")?..];
    let root = &root[..root.find('>')?];
    let attr = |name: &str| -> Option<f32> {
        let start = root.find(&format!(" {name}=\""))? + name.len() + 3;
        let len = root[start..].find('"')?;
        root[start..start + len].parse().ok()
    };
    Some((attr("width")?, attr("height")?))
}

/// Set the root `width`/`height` to `factor` times their size, keeping the
/// view box, so GPUI rasterizes the diagram at a higher resolution.
fn scale_svg(svg: &str, (width, height): (f32, f32), factor: f32) -> String {
    let Some(start) = svg.find("<svg") else {
        return svg.to_owned();
    };
    let Some(end) = svg[start..].find('>').map(|e| start + e) else {
        return svg.to_owned();
    };
    let root = &svg[start..end];
    let set = |root: String, name: &str, value: f32| -> String {
        let key = format!(" {name}=\"");
        let Some(at) = root.find(&key).map(|i| i + key.len()) else {
            return root;
        };
        let Some(len) = root[at..].find('"') else {
            return root;
        };
        format!("{}{:.2}{}", &root[..at], value, &root[at + len..])
    };
    let root = set(root.to_owned(), "width", width * factor);
    let root = set(root, "height", height * factor);
    format!("{}{}{}", &svg[..start], root, &svg[end..])
}

/// Lay out `source`, or `None` when it isn't a diagram rusty-mermaid can
/// draw. Colors don't change the layout, so the light theme measures it.
pub fn layout(source: &str) -> Option<Diagram> {
    let source = source.trim_end();
    if source.trim().is_empty() || source.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let kind = rusty_mermaid::detect(source)?;
    let svg = to_svg(source, &Theme::light())?;
    let (width, height) = svg_size(&svg)?;
    if !(width.is_finite() && height.is_finite() && width >= 1. && height >= 1.) {
        return None;
    }
    Some(Diagram {
        source: source.to_owned(),
        hash: hash_of(source),
        kind,
        width,
        height,
    })
}

fn color(hsla: Hsla) -> Color {
    let rgba = Rgba::from(hsla);
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    Color::rgba(byte(rgba.r), byte(rgba.g), byte(rgba.b), byte(rgba.a))
}

/// `a` with `amount` of `b` mixed in, opaque.
fn mix(a: Hsla, b: Hsla, amount: f32) -> Hsla {
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    let t = amount.clamp(0., 1.);
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.,
    }
    .into()
}

/// The reader's colors that a diagram is drawn with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    dark: bool,
    background: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    border: Hsla,
    accent: Hsla,
}

impl Palette {
    pub fn from_theme(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            dark: theme.is_dark(),
            background: theme.background,
            foreground: theme.foreground,
            muted_foreground: theme.muted_foreground,
            border: theme.border,
            accent: theme.primary,
        }
    }

    fn key(&self) -> u64 {
        let bits = |c: Hsla| [c.h, c.s, c.l, c.a].map(f32::to_bits);
        hash_of((
            self.dark,
            [
                self.background,
                self.foreground,
                self.muted_foreground,
                self.border,
                self.accent,
            ]
            .map(bits),
        ))
    }

    /// A rusty-mermaid theme in these colors. Nodes are a light wash of the
    /// theme's accent with an accent outline, so the editor themes give calm
    /// diagrams and the fun themes colorful ones. Notes, pie slices and the
    /// like keep rusty-mermaid's own colors for that mode.
    fn theme(&self) -> Theme {
        let mut t = if self.dark {
            Theme::dark()
        } else {
            Theme::light()
        };
        let (bg, fg, accent) = (self.background, self.foreground, self.accent);
        let line = mix(fg, bg, 0.25);
        let outline = mix(accent, bg, 0.2);
        t.node_fill = color(mix(bg, accent, if self.dark { 0.16 } else { 0.08 }));
        t.node_stroke = color(outline);
        t.node_text = color(fg);
        t.edge_stroke = color(line);
        t.edge_label_text = color(fg);
        t.edge_label_bg = color(bg);
        t.start_fill = color(fg);
        t.end_inner_fill = color(fg);
        t.composite_fill = color(mix(bg, fg, 0.03));
        t.composite_stroke = color(mix(accent, bg, 0.5));
        t.composite_label = color(fg);
        t.subgraph_fill = color(mix(bg, fg, if self.dark { 0.05 } else { 0.035 }));
        t.subgraph_stroke = color(self.border);
        t.subgraph_label = color(self.muted_foreground);
        t.divider_stroke = color(self.border);
        t.lifeline_stroke = color(mix(self.muted_foreground, bg, 0.4));
        t.activation_fill = color(mix(bg, accent, 0.25));
        t.activation_stroke = color(outline);
        t.grid_stroke = color(self.border);
        t.muted_text = color(self.muted_foreground);
        t.background = color(bg);
        t
    }
}

type ImageCache = HashMap<(u64, u64, u32), Arc<Image>>;

/// Drawn diagrams by diagram, colors and resolution. GPUI rasterizes each
/// image once, off the main thread, and keeps it while it's in use.
static IMAGES: LazyLock<Mutex<ImageCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Diagrams the reader has flipped to their source, by diagram.
static SHOWING_SOURCE: LazyLock<Mutex<HashSet<u64>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Shows every diagram's source and toolbar, for screenshots of states that
/// otherwise need a hover.
static SHOW_ALL_SOURCE: AtomicBool = AtomicBool::new(false);

#[cfg_attr(not(feature = "screenshot"), allow(dead_code))]
pub fn show_all_source(show: bool) {
    SHOW_ALL_SOURCE.store(show, Ordering::Relaxed);
}

fn showing_source(hash: u64) -> bool {
    SHOW_ALL_SOURCE.load(Ordering::Relaxed)
        || SHOWING_SOURCE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&hash)
}

#[cfg(test)]
pub fn showing_source_of(source: &str) -> bool {
    showing_source(hash_of(source.trim_end()))
}

fn toggle_source(hash: u64) {
    let mut set = SHOWING_SOURCE.lock().unwrap_or_else(|e| e.into_inner());
    if !set.remove(&hash) {
        set.insert(hash);
    }
}

/// How many times the natural size to rasterize at. GPUI draws an SVG at
/// twice its size; drawing it `scale` times larger again keeps it sharp on a
/// Retina screen at the reading size, within the pixel budget.
fn resolution(diagram: &Diagram, scale: f32) -> f32 {
    let wanted = (scale * 4.).ceil() / 4.;
    let budget = (MAX_RASTER_PIXELS / (4. * diagram.width * diagram.height)).sqrt();
    wanted.max(1.).min(budget)
}

fn image(diagram: &Diagram, palette: &Palette, resolution: f32) -> Option<Arc<Image>> {
    let key = (diagram.hash, palette.key(), (resolution * 100.) as u32);
    if let Some(image) = IMAGES.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Some(image.clone());
    }
    let svg = to_svg(&diagram.source, &palette.theme())?;
    let svg = scale_svg(&svg, (diagram.width, diagram.height), resolution);
    let image = Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()));
    let mut images = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    if images.len() > 256 {
        images.clear();
    }
    images.insert(key, image.clone());
    Some(image)
}

/// A spoken name for the diagram, e.g. "Mermaid sequence diagram".
fn label(kind: rusty_mermaid::DiagramKind) -> String {
    use rusty_mermaid::DiagramKind as K;
    let name = match kind {
        K::Flowchart => "flowchart",
        K::State => "state diagram",
        K::Sequence => "sequence diagram",
        K::Class => "class diagram",
        K::Er => "entity relationship diagram",
        K::Gantt => "Gantt chart",
        K::Pie => "pie chart",
        K::Mindmap => "mind map",
        K::GitGraph => "Git graph",
        K::UserJourney => "user journey",
        K::Timeline => "timeline",
        _ => "diagram",
    };
    format!("Mermaid {name}")
}

struct MermaidBlock;

impl MarkdownPlugin for MermaidBlock {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "mermaid"
    }

    fn parse(&self, node: &mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Code(code) = node else {
            return None;
        };
        if !code
            .lang
            .as_deref()
            .is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"))
        {
            return None;
        }
        let diagram = layout(&code.value)?;
        let label = label(diagram.kind);
        let mut out = MarkdownNode::new(self.name().to_owned(), diagram)
            .text(code.value.clone())
            .accessibility_label(label);
        if let Some(source) = cx.node_source(node) {
            out = out.markdown(source.to_owned());
        }
        Some(out)
    }

    fn render(&self, node: &MarkdownNode, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(diagram) = node.data::<Diagram>() else {
            return div().child(node.as_text().to_owned()).into_any_element();
        };
        let font_size = cx
            .try_global::<crate::pane::FontSize>()
            .map_or(BODY_PX, |f| f.0);
        let scale = font_size / BODY_PX;
        let palette = Palette::from_theme(cx);
        let theme = cx.theme();
        let hash = diagram.hash;
        let source = showing_source(hash);
        let (width, height) = (diagram.width * scale, diagram.height * scale);

        let toolbar = div()
            .flex()
            .absolute()
            .top(px(5.))
            .right_2()
            .gap_1()
            .items_center()
            .child(
                div()
                    .flex()
                    .gap_1()
                    .when(!SHOW_ALL_SOURCE.load(Ordering::Relaxed), |d| d.opacity(0.))
                    .group_hover("mermaid", |s| s.opacity(1.))
                    .child(
                        Button::new("mermaid-copy")
                            .ghost()
                            .xsmall()
                            .label("Copy")
                            .tooltip("Copy Mermaid source")
                            .on_click({
                                let text = diagram.source.clone();
                                move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                                }
                            }),
                    )
                    .child(
                        Button::new("mermaid-toggle")
                            .ghost()
                            .xsmall()
                            .label(if source { "Diagram" } else { "Code" })
                            .tooltip(if source {
                                "Show the diagram"
                            } else {
                                "Show the Mermaid source"
                            })
                            .on_click(move |_, window, _| {
                                toggle_source(hash);
                                window.refresh();
                            }),
                    ),
            )
            .child(
                div()
                    .px_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("mermaid"),
            );

        let body = if source {
            div()
                .id("mermaid-source")
                .w_full()
                .overflow_x_scroll()
                .px_4()
                .pt(px(36.))
                .pb(px(14.))
                .bg(theme.muted)
                .font_family(theme.mono_font_family.clone())
                .text_size(theme.mono_font_size)
                .line_height(relative(1.6))
                .whitespace_nowrap()
                .children(
                    diagram
                        .source
                        .lines()
                        .map(|line| div().child(line.to_owned())),
                )
                .into_any_element()
        } else {
            let picture = match image(diagram, &palette, resolution(diagram, scale)) {
                // As wide as the page allows between the bounds, and as
                // tall as the proportions say. The height is top padding,
                // which resolves against the clamped width; `aspect_ratio`
                // would size from the unclamped width, and an image left to
                // itself takes its natural height.
                Some(image) => div()
                    .flex_shrink_0()
                    .w_full()
                    .max_w(px(width))
                    .min_w(px(width * MIN_FIT))
                    .mx_auto()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .pt(relative(height / width))
                            .child(img(image).absolute().inset_0().size_full()),
                    )
                    .into_any_element(),
                None => div().child(diagram.source.clone()).into_any_element(),
            };
            div()
                .id("mermaid-diagram")
                .w_full()
                .overflow_x_scroll()
                .px_4()
                .pt(px(32.))
                .pb(px(20.))
                .flex()
                .flex_col()
                .child(picture)
                .into_any_element()
        };

        div()
            .id(ElementId::NamedInteger("mermaid".into(), hash))
            .group("mermaid")
            .relative()
            .w_full()
            .min_w_0()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .overflow_hidden()
            .child(body)
            .child(toolbar)
            .into_any_element()
    }
}

/// The plugin, for the reader's Markdown extensions.
pub fn plugin() -> impl MarkdownPlugin {
    MermaidBlock
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    const FLOW: &str = "flowchart TD\n  a[Start] --> b{Ok?}\n  b -->|yes| c[Ship]\n  b -->|no| a";

    #[test]
    fn lays_out_supported_diagrams() {
        let flow = layout(FLOW).unwrap();
        assert_eq!(flow.kind, rusty_mermaid::DiagramKind::Flowchart);
        assert!(flow.width > 50. && flow.height > 100., "{flow:?}");
        for source in [
            "sequenceDiagram\n  A->>B: hi\n  B-->>A: hello",
            "stateDiagram-v2\n  [*] --> On\n  On --> [*]",
            "classDiagram\n  A <|-- B",
            "erDiagram\n  A ||--o{ B : has",
            "pie title Pets\n  \"Dogs\" : 3\n  \"Cats\" : 2",
            "gantt\n  dateFormat YYYY-MM-DD\n  section A\n  T :a1, 2026-01-01, 3d",
        ] {
            assert!(layout(source).is_some(), "{source}");
        }
    }

    #[test]
    fn leaves_other_text_as_code() {
        assert!(layout("").is_none());
        assert!(layout("   \n").is_none());
        assert!(layout("not a diagram at all").is_none());
        assert!(layout("sequenceDiagram\n  A->>").is_none(), "empty diagram");
        assert!(layout("pie\n  \"x\" : abc").is_none(), "empty diagram");
        assert!(layout(&format!("{FLOW}\n%%{}", "x".repeat(MAX_SOURCE_BYTES))).is_none());
    }

    #[test]
    fn same_layout_in_every_mode() {
        let light = Palette {
            dark: false,
            background: gpui_kit::white(),
            foreground: gpui_kit::black(),
            muted_foreground: gpui_kit::black(),
            border: gpui_kit::black(),
            accent: gpui_kit::blue(),
        };
        let dark = Palette {
            dark: true,
            background: gpui_kit::black(),
            foreground: gpui_kit::white(),
            ..light
        };
        assert_ne!(light.key(), dark.key());
        let diagram = layout(FLOW).unwrap();
        let svg = to_svg(FLOW, &dark.theme()).unwrap();
        assert_eq!(svg_size(&svg), Some((diagram.width, diagram.height)));
        assert!(svg.contains("#000000") || svg.contains("#ffffff"));
    }

    #[test]
    fn scales_the_root_size_only() {
        let svg = r#"<svg xmlns="x" viewBox="0 0 10.5 20" width="10.5" height="20"><rect width="3" height="4"/></svg>"#;
        assert_eq!(svg_size(svg), Some((10.5, 20.)));
        let big = scale_svg(svg, (10.5, 20.), 2.);
        assert_eq!(svg_size(&big), Some((21., 40.)));
        assert!(big.contains(r#"viewBox="0 0 10.5 20""#));
        assert!(big.contains(r#"<rect width="3" height="4"/>"#));
    }

    #[test]
    fn resolution_stays_within_budget() {
        let small = layout(FLOW).unwrap();
        assert_eq!(resolution(&small, 1.), 1.);
        assert_eq!(resolution(&small, 1.3), 1.5);
        let huge = Diagram {
            width: 4000.,
            height: 4000.,
            ..layout(FLOW).unwrap()
        };
        let r = resolution(&huge, 2.);
        assert!(4. * r * r * 4000. * 4000. <= MAX_RASTER_PIXELS * 1.001);
    }
}
