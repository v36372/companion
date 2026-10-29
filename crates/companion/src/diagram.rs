//! SVG is untrusted: validate before handing bytes to GPUI/resvg.
use gpui::{
    Context, Entity, Image, ImageFormat, ImageSource, Render, StyledImage, Window, div, img,
    prelude::*, px,
};
use gpui_base::text::{MarkdownNode, markdown_ast::Node};
use gpui_component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    text::TextView,
};
use std::sync::Arc;

const MAX_BYTES: usize = 256 * 1024;
#[derive(Clone)]
pub struct Diagram {
    source: String,
    image: Arc<Image>,
    ratio: f32,
}

pub fn svg_code(lang: Option<&str>, body: &str) -> bool {
    lang == Some("svg") || (lang == Some("xml") && svg_start(body))
}
fn svg_start(body: &str) -> bool {
    let mut body = body.trim_start();
    if body.starts_with("<?xml") {
        let Some((_, rest)) = body.split_once("?>") else {
            return false;
        };
        body = rest.trim_start();
    }
    body.strip_prefix("<svg")
        .is_some_and(|rest| rest.starts_with([' ', '\t', '\r', '\n', '>', '/']))
}
fn local_urls(value: &str) -> bool {
    // CSS escapes and imports can disguise resource references. Diagrams need
    // neither; rejecting them keeps the accepted CSS subset auditable.
    let lower = value.to_ascii_lowercase();
    if lower.contains('\\') || lower.contains('@') {
        return false;
    }
    let mut rest = lower.as_str();
    while let Some((_, after)) = rest.split_once("url(") {
        let Some((url, tail)) = after.split_once(')') else {
            return false;
        };
        let url = url.trim().trim_matches(['\'', '"']);
        if !url.starts_with('#') {
            return false;
        }
        rest = tail;
    }
    true
}
/// Returns the intrinsic aspect ratio. XML parsing expands attribute character
/// references before checking hrefs; DTDs are rejected before parsing.
pub fn validate(source: &str) -> Option<f32> {
    if source.len() > MAX_BYTES || !svg_start(source) {
        return None;
    }
    let lower = source.to_ascii_lowercase();
    if ["<!doctype", "<!entity"].iter().any(|s| lower.contains(s)) {
        return None;
    }
    let document = roxmltree::Document::parse(source.trim_start()).ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return None;
    }
    for node in document.descendants().filter(|node| node.is_element()) {
        if ["script", "foreignobject", "image"]
            .contains(&node.tag_name().name().to_ascii_lowercase().as_str())
        {
            return None;
        }
        for attribute in node.attributes() {
            let name = attribute.name().to_ascii_lowercase();
            if name.starts_with("on")
                || (name == "href" && !attribute.value().starts_with('#'))
                || !local_urls(attribute.value())
            {
                return None;
            }
        }
        if node.tag_name().name() == "style"
            && node
                .descendants()
                .filter(|n| n.is_text())
                .any(|n| !local_urls(n.text().unwrap_or_default()))
        {
            return None;
        }
    }
    let viewbox = root.attribute("viewBox").and_then(|v| {
        let values: Vec<f32> = v
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        (values.len() == 4).then(|| (values[2], values[3]))
    });
    let dimensions = viewbox.or_else(|| {
        let length = |name| {
            root.attribute(name)?
                .trim_end_matches("px")
                .parse::<f32>()
                .ok()
        };
        Some((length("width")?, length("height")?))
    });
    let (width, height) = dimensions.unwrap_or((720., 400.));
    (width.is_finite() && height.is_finite() && width > 0. && height > 0.).then_some(width / height)
}
fn parse(node: &Node) -> Option<MarkdownNode> {
    let Node::Code(code) = node else {
        return None;
    };
    if !svg_code(code.lang.as_deref(), &code.value) {
        return None;
    }
    let ratio = validate(&code.value)?;
    let diagram = Diagram {
        source: code.value.clone(),
        image: Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            code.value.trim_start().as_bytes().to_vec(),
        )),
        ratio,
    };
    Some(
        MarkdownNode::new("svg-diagram", diagram)
            .text("Diagram")
            .accessibility_label("SVG diagram. Click to enlarge."),
    )
}
pub fn markdown(
    id: impl Into<gpui::ElementId>,
    content: String,
    viewer: Entity<DiagramViewer>,
) -> TextView {
    TextView::markdown(id, content)
        .selectable(true)
        .markdown_block_parser(|node, context| {
            parse(node).or_else(|| {
                if matches!(node, Node::Paragraph(_)) {
                    let source = context.node_source(node)?.to_owned();
                    Some(
                        MarkdownNode::new("chat-prose", source.clone())
                            .text(node.to_string())
                            .markdown(source),
                    )
                } else {
                    None
                }
            })
        })
        .markdown_block_renderer("chat-prose", |node, _, _| {
            TextView::markdown(
                "chat-paragraph",
                node.data::<String>().expect("prose node").clone(),
            )
            .selectable(true)
            .w_full()
            .max_w(px(576.))
        })
        .markdown_block_renderer("svg-diagram", move |node, _, cx| {
            let diagram = node.data::<Diagram>().expect("diagram node").clone();
            card(diagram, viewer.clone(), cx)
        })
}
fn fallback(source: String) -> gpui::AnyElement {
    TextView::markdown("svg-render-fallback", format!("```svg\n{source}\n```"))
        .selectable(true)
        .into_any_element()
}
fn card(
    diagram: Diagram,
    viewer: Entity<DiagramViewer>,
    cx: &mut gpui::App,
) -> gpui::Stateful<gpui::Div> {
    let source = diagram.source.clone();
    let copy = source.clone();
    let open = diagram.clone();
    let card_id = diagram.image.id();
    div()
        .id(("svg-card", card_id))
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .id(("inline-diagram", diagram.image.id()))
                .w_full()
                .rounded_md()
                .overflow_hidden()
                .bg(gpui::rgb(0xf7f8fa))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    viewer.update(cx, |viewer, cx| {
                        viewer.open = Some(open.clone());
                        cx.notify();
                    })
                })
                .child(
                    img(ImageSource::Image(diagram.image.clone()))
                        .w_full()
                        .aspect_ratio(diagram.ratio)
                        .with_fallback(move || fallback(source.clone())),
                ),
        )
        .child(
            Button::new("copy-svg")
                .ghost()
                .small()
                .label("Copy SVG")
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()))
                }),
        )
}
/// Hide incomplete SVG/XML source while tokens are arriving. Finished blocks
/// are also deferred until the durable transcript contains the complete turn.
pub fn streaming_text(source: &str) -> String {
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```svg") || trimmed.starts_with("```xml") {
            return format!("{}\n\nDrawing diagram…", &source[..offset]);
        }
        offset += line.len();
    }
    source.to_owned()
}
#[derive(Default)]
pub struct DiagramViewer {
    open: Option<Diagram>,
}
impl DiagramViewer {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        cx.notify();
    }
}
impl Render for DiagramViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .children(self.open.clone().map(|diagram| {
                let source = diagram.source.clone();
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(cx.theme().overlay)
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_4()
                            .rounded_lg()
                            .bg(cx.theme().background)
                            .w(window.viewport_size().width - px(80.))
                            .max_h(window.viewport_size().height - px(80.))
                            .child(
                                Button::new("close-diagram")
                                    .ghost()
                                    .small()
                                    .label("Close · Esc")
                                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                            )
                            .child(
                                img(ImageSource::Image(diagram.image))
                                    .w_full()
                                    .max_h(window.viewport_size().height - px(160.))
                                    .aspect_ratio(diagram.ratio)
                                    .with_fallback(move || fallback(source.clone())),
                            ),
                    )
            }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_sizes_and_fences() {
        assert_eq!(
            validate("  <?xml version='1.0'?>\n<svg width='600' height='200'/>"),
            Some(3.)
        );
        assert_eq!(
            validate(
                "<svg viewBox='0 0 720 360'><rect width='720' height='360' fill='white'/></svg>"
            ),
            Some(2.)
        );
        assert_eq!(
            validate("<?xml version='1.0'?>\n<svg width='600px' height='200'/ >"),
            None
        );
        assert_eq!(
            validate("<?xml version='1.0'?>\n<svg width='600px' height='200'/>"),
            Some(3.)
        );
        assert!(svg_code(Some("svg"), "<svg/>"));
        assert!(svg_code(Some("xml"), "<svg/>"));
        assert!(!svg_code(Some("xml"), "<other/>"));
        assert!(!svg_code(None, "<svg/>"));
        assert_eq!(
            validate(
                "<svg><defs><linearGradient id='g'/></defs><rect fill='url(#g)'/><use href='#g'/></svg>"
            ),
            Some(1.8)
        );
    }
    #[test]
    fn rejects_untrusted_resources_and_bad_xml() {
        for source in [
            "<not-svg/>",
            "<!DOCTYPE svg><svg/>",
            "<svg><script/></svg>",
            "<svg><foreignObject/></svg>",
            "<svg><image/></svg>",
            "<svg><use href='file:///etc/passwd'/></svg>",
            "<svg xmlns:xlink='http://www.w3.org/1999/xlink'><use xlink:href='https://example.com'/></svg>",
            "<svg><use href='&#102;ile:///x'/></svg>",
            "<svg fill='url(https://example.com)'/>",
            "<svg><style>@import 'x';</style></svg>",
            "<svg style='fill:u\\72l(x)'/>",
            "<svg onload='x'/>",
            "<svg viewBox='0 0 0 0'/>",
            "<svg>",
            "<!ENTITY x 'y'><svg/>",
        ] {
            assert!(validate(source).is_none(), "{source}");
        }
        assert!(validate(&format!("<svg>{}</svg>", " ".repeat(MAX_BYTES))).is_none());
    }
    #[test]
    fn streaming_hides_unfinished_diagrams() {
        assert_eq!(
            streaming_text("Hello\n```svg\n<svg"),
            "Hello\n\n\nDrawing diagram…"
        );
        assert_eq!(streaming_text("Hello"), "Hello");
    }
}
