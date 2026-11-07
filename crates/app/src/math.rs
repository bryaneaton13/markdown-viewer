//! LaTeX math. `$…$` inside a paragraph, and `$$ … $$` or ```` ```math ````
//! blocks, are typeset by latex-rust (pure Rust, STIX Two Math) into SVG
//! while the document parses off the main thread, then drawn as images in
//! the text color.
//!
//! Anything that doesn't typeset stays as the author wrote it: inline math
//! falls back to its literal source (so "$5 and $10" is never mangled), and a
//! block that fails shows as a code block.

use gpui_kit::{
    base::text::{
        InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext,
        MarkdownPlugin, markdown_ast as mdast,
    },
    component::ActiveTheme,
    *,
};
use latex_rust::{Dim, MathBox, MathFont, MathStyle, SvgOptions};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, LazyLock, Mutex},
};

/// Em size the SVG is drawn at. Large enough that the raster stays sharp when
/// scaled down to the reading size on a Retina display at the largest text size.
const SVG_EM_PT: i64 = 48;
/// The placeholder fill latex-rust writes for uncolored glyphs; swapped for
/// the theme's text color when drawn.
const PLACEHOLDER_FILL: &str = "fill=\"#000000\"";
/// STIX sets smaller than the reader's sans-serif at the same size.
const INLINE_SCALE: f32 = 1.15;
const BLOCK_SCALE: f32 = 1.25;

static FONT: LazyLock<Option<MathFont>> = LazyLock::new(|| match MathFont::stix_two_math() {
    Ok(font) => Some(font),
    Err(e) => {
        log::error!("Math font failed to load: {e}");
        None
    }
});

/// A typeset formula. Sizes are in ems of the surrounding text.
#[derive(Debug)]
pub struct Formula {
    svg: String,
    hash: u64,
    width: f32,
    /// Above the baseline.
    height: f32,
    /// Below the baseline.
    depth: f32,
}

/// Typeset `latex`, or `None` when it doesn't parse or uses something
/// latex-rust doesn't support.
pub fn typeset(latex: &str, display: bool) -> Option<Formula> {
    let font = FONT.as_ref()?;
    let ast = latex_rust::parse(latex).ok()?;
    let style = if display {
        MathStyle::Display
    } else {
        MathStyle::Text
    };
    let tree: MathBox = latex_rust::layout(&ast, font, style).ok()?;
    let mut options = SvgOptions::new();
    options.font_size_pt = Dim::from_i64(SVG_EM_PT);
    let svg = latex_rust::render_svg(&tree, font, &options).ok()?;
    let em = |d: &Dim| f32::from_bits(d.to_ieee32_bits());
    let (width, height, depth) = (em(&tree.width), em(&tree.height), em(&tree.depth));
    if !(width.is_finite() && height.is_finite() && depth.is_finite()) || width <= 0. {
        return None;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    svg.hash(&mut hasher);
    Some(Formula {
        hash: hasher.finish(),
        svg,
        width,
        height: height.max(0.),
        depth: depth.max(0.),
    })
}

/// Pandoc's rule for `$…$`: no space just inside either dollar, and no digit
/// right after the closing one. Keeps prices and shell variables literal.
fn looks_like_math(value: &str, source: &str, after: Option<char>) -> bool {
    let single = source.starts_with('$') && !source.starts_with("$$");
    !value.is_empty()
        && !value.starts_with(char::is_whitespace)
        && !value.ends_with(char::is_whitespace)
        && !(single && after.is_some_and(|c| c.is_ascii_digit()))
}

type ImageCache = HashMap<(u64, u32), Arc<Image>>;

/// Recolored images, keyed by formula and color, so each is rasterized once.
static IMAGES: LazyLock<Mutex<ImageCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn image(formula: &Formula, color: Hsla) -> Arc<Image> {
    let rgb = Rgba::from(color);
    let hex = |v: f32| (v.clamp(0., 1.) * 255.).round() as u32;
    let key = (hex(rgb.r) << 16) | (hex(rgb.g) << 8) | hex(rgb.b);
    let mut images = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    if images.len() > 1024 {
        images.clear();
    }
    images
        .entry((formula.hash, key))
        .or_insert_with(|| {
            let svg = formula
                .svg
                .replace(PLACEHOLDER_FILL, &format!("fill=\"#{key:06x}\""));
            Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()))
        })
        .clone()
}

fn formula_element(formula: &Formula, font_size: Pixels, color: Hsla) -> Img {
    img(image(formula, color))
        .w(font_size * formula.width)
        .h(font_size * (formula.height + formula.depth))
        .flex_shrink_0()
}

struct InlineMath;

impl MarkdownPlugin for InlineMath {
    fn name(&self) -> &str {
        "math-inline"
    }

    fn parse(&self, node: &mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::InlineMath(math) = node else {
            return None;
        };
        let source = cx.node_source(node)?;
        let after = node
            .position()
            .and_then(|p| cx.source().get(p.end.offset..))
            .and_then(|rest| rest.chars().next());
        if !looks_like_math(&math.value, source, after) {
            return None;
        }
        let formula = typeset(&math.value, false)?;
        Some(
            MarkdownNode::new(self.name().to_owned(), formula)
                .text(math.value.clone())
                .markdown(source.to_owned()),
        )
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        _: &mut Window,
        _: &mut App,
    ) -> Option<InlineElement> {
        let formula = node.data::<Formula>()?;
        let size = context.font_size() * INLINE_SCALE;
        let color = context.text_style().color;
        Some(
            InlineElement::new(formula_element(formula, size, color))
                .with_baseline(size * formula.height),
        )
    }
}

struct BlockMath;

impl MarkdownPlugin for BlockMath {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "math-block"
    }

    fn parse(&self, node: &mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let value = match node {
            mdast::Node::Math(math) => &math.value,
            mdast::Node::Code(code) if code.lang.as_deref() == Some("math") => &code.value,
            _ => return None,
        };
        let formula = typeset(value.trim(), true)?;
        let mut out = MarkdownNode::new(self.name().to_owned(), formula).text(value.clone());
        if let Some(source) = cx.node_source(node) {
            out = out.markdown(source.to_owned());
        }
        Some(out)
    }

    fn render(&self, node: &MarkdownNode, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(formula) = node.data::<Formula>() else {
            return div().child(node.as_text().to_owned()).into_any_element();
        };
        let size = px(cx
            .try_global::<crate::pane::FontSize>()
            .map_or(16., |f| f.0));
        let id = ElementId::NamedInteger("math".into(), formula.hash);
        div()
            .id(id)
            .w_full()
            .py(size * 0.5)
            .overflow_x_scroll()
            .flex()
            .justify_center()
            .child(formula_element(
                formula,
                size * BLOCK_SCALE,
                cx.theme().foreground,
            ))
            .into_any_element()
    }
}

/// The reader's Markdown extensions. Built once, so its revision never
/// changes and documents aren't re-parsed on every frame.
pub fn extensions() -> MarkdownExtensions {
    static EXTENSIONS: LazyLock<MarkdownExtensions> = LazyLock::new(|| {
        MarkdownExtensions::default()
            .plugin(BlockMath)
            .plugin(InlineMath)
    });
    EXTENSIONS.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn typesets_text_and_display_math() {
        let inline = typeset(r"e^{i\pi}+1=0", false).unwrap();
        assert!(inline.svg.contains(PLACEHOLDER_FILL));
        assert!(inline.width > 3. && inline.height > 0.5);
        let display = typeset(r"\sum_{k=1}^n k", true).unwrap();
        let text = typeset(r"\sum_{k=1}^n k", false).unwrap();
        assert!(display.height + display.depth > text.height + text.depth);
        assert!(typeset(r"\frac{1}{", false).is_none());
        assert!(typeset(r"\notacommand", false).is_none());
    }

    #[test]
    fn dollar_amounts_stay_literal() {
        assert!(looks_like_math("x^2", "$x^2$", None));
        assert!(looks_like_math("x", "$x$", Some(' ')));
        assert!(!looks_like_math("5 and ", "$5 and $", Some('1')));
        assert!(!looks_like_math(" x", "$ x$", None));
        assert!(
            !looks_like_math("5", "$5$", Some('0')),
            "closing $ before a digit"
        );
        assert!(looks_like_math("x", "$$x$$", Some('0')));
    }
}
