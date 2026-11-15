use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use skia_safe::{
    Canvas, Color, FontMgr, FontStyle, Paint, Rect,
    textlayout::{
        FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, TextDecoration, TextStyle,
    },
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle as SyntaxFontStyle, Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

const TEXT: Color = Color::from_rgb(204, 212, 222);
const HEADING: Color = Color::from_rgb(240, 244, 249);
const MUTED: Color = Color::from_rgb(139, 154, 173);
const LINK: Color = Color::from_rgb(129, 185, 255);
const CODE_BG: Color = Color::from_rgb(20, 25, 34);
const RULE: Color = Color::from_rgb(51, 63, 78);

pub fn font_collection() -> FontCollection {
    let mut fonts = FontCollection::new();
    fonts.set_default_font_manager(FontMgr::new(), Some("Helvetica Neue"));
    fonts.enable_font_fallback();
    fonts
}

fn text_style(size: f32, color: Color, monospace: bool) -> TextStyle {
    let mut style = TextStyle::new();
    style
        .set_font_families(if monospace {
            &["Menlo", "PingFang SC", "Apple Color Emoji"]
        } else {
            &["Helvetica Neue", "PingFang SC", "Apple Color Emoji"]
        })
        .set_font_size(size)
        .set_color(color)
        .set_height(1.45)
        .set_height_override(true);
    style
}

pub fn paragraph(
    text: &str,
    size: f32,
    color: Color,
    width: f32,
    fonts: &FontCollection,
) -> Paragraph {
    let mut style = ParagraphStyle::new();
    style.set_text_style(&text_style(size, color, false));
    let mut builder = ParagraphBuilder::new(&style, fonts.clone());
    builder.add_text(text);
    let mut result = builder.build();
    result.layout(width.max(1.0));
    result
}

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
}

impl Highlighter {
    pub fn new() -> Self {
        let themes = ThemeSet::load_defaults();
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            theme: themes.themes["base16-ocean.dark"].clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: bool,
}

#[derive(Clone, Debug)]
struct Run {
    text: String,
    style: InlineStyle,
}

#[derive(Debug)]
enum RawBlock {
    Text {
        runs: Vec<Run>,
        heading: u8,
        quote_depth: usize,
        list_depth: usize,
        list_indent: f32,
        prefix: Option<String>,
    },
    Code {
        source: String,
        language: String,
    },
    Rule,
    TableRow {
        cells: Vec<Vec<Run>>,
        header: bool,
    },
}

struct ListState {
    next: Option<u64>,
    gutter: f32,
}

struct MarkdownParser {
    blocks: Vec<RawBlock>,
    runs: Vec<Run>,
    inline: Vec<InlineStyle>,
    heading: u8,
    quote_depth: usize,
    lists: Vec<ListState>,
    prefix: Option<String>,
    code: Option<(String, String)>,
    table: bool,
    cells: Vec<Vec<Run>>,
    cell: Vec<Run>,
}

impl MarkdownParser {
    fn style(&self) -> InlineStyle {
        self.inline.last().copied().unwrap_or_default()
    }

    fn push(&mut self, text: &str, style: InlineStyle) {
        let runs = if self.table {
            &mut self.cell
        } else {
            &mut self.runs
        };
        if let Some(last) = runs.last_mut().filter(|run| run.style == style) {
            last.text.push_str(text);
        } else if !text.is_empty() {
            runs.push(Run {
                text: text.to_owned(),
                style,
            });
        }
    }

    fn flush(&mut self) {
        if self.runs.is_empty() {
            return;
        }
        self.blocks.push(RawBlock::Text {
            runs: std::mem::take(&mut self.runs),
            heading: self.heading,
            quote_depth: self.quote_depth,
            list_depth: self.lists.len(),
            list_indent: self.lists.iter().map(|list| list.gutter).sum(),
            prefix: self.prefix.take(),
        });
    }
}

fn parse(source: &str) -> Vec<RawBlock> {
    let mut state = MarkdownParser {
        blocks: Vec::new(),
        runs: Vec::new(),
        inline: Vec::new(),
        heading: 0,
        quote_depth: 0,
        lists: Vec::new(),
        prefix: None,
        code: None,
        table: false,
        cells: Vec::new(),
        cell: Vec::new(),
    };
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                state.flush();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                state.code = Some((String::new(), language));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((source, language)) = state.code.take() {
                    state.blocks.push(RawBlock::Code { source, language });
                }
            }
            Event::Text(text) if state.code.is_some() => {
                state.code.as_mut().unwrap().0.push_str(&text);
            }
            Event::Start(Tag::Paragraph) => state.flush(),
            Event::End(TagEnd::Paragraph) => state.flush(),
            Event::Start(Tag::Heading { level, .. }) => {
                state.flush();
                state.heading = level as u8;
            }
            Event::End(TagEnd::Heading(_)) => {
                state.flush();
                state.heading = 0;
            }
            Event::Start(Tag::BlockQuote(_)) => {
                state.flush();
                state.quote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                state.flush();
                state.quote_depth = state.quote_depth.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => {
                state.flush();
                state.lists.push(ListState {
                    next: start,
                    gutter: 25.0,
                });
            }
            Event::End(TagEnd::List(_)) => {
                state.flush();
                state.lists.pop();
            }
            Event::Start(Tag::Item) => {
                state.flush();
                let prefix = match state.lists.last_mut().and_then(|list| list.next.as_mut()) {
                    Some(number) => {
                        let prefix = format!("{}.", number);
                        *number += 1;
                        prefix
                    }
                    _ => "•".to_owned(),
                };
                if let Some(list) = state.lists.last_mut() {
                    list.gutter = list.gutter.max(prefix.chars().count() as f32 * 10.0 + 8.0);
                }
                state.prefix = Some(prefix);
            }
            Event::End(TagEnd::Item) => {
                state.flush();
                state.prefix = None;
            }
            Event::Start(Tag::Emphasis) => {
                let mut style = state.style();
                style.italic = true;
                state.inline.push(style);
            }
            Event::Start(Tag::Strong) => {
                let mut style = state.style();
                style.bold = true;
                state.inline.push(style);
            }
            Event::Start(Tag::Strikethrough) => {
                let mut style = state.style();
                style.strike = true;
                state.inline.push(style);
            }
            Event::Start(Tag::Link { .. }) => {
                let mut style = state.style();
                style.link = true;
                state.inline.push(style);
            }
            Event::End(
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link,
            ) => {
                state.inline.pop();
            }
            Event::Start(Tag::Table(_)) => {
                state.flush();
                state.table = true;
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => {
                state.cells.clear();
                state.cell.clear();
            }
            Event::End(TagEnd::TableCell) => {
                state.cells.push(std::mem::take(&mut state.cell));
            }
            Event::End(TagEnd::TableHead) => {
                state.blocks.push(RawBlock::TableRow {
                    cells: std::mem::take(&mut state.cells),
                    header: true,
                });
            }
            Event::End(TagEnd::TableRow) => {
                state.blocks.push(RawBlock::TableRow {
                    cells: std::mem::take(&mut state.cells),
                    header: false,
                });
            }
            Event::End(TagEnd::Table) => {
                state.table = false;
            }
            Event::Text(text) => state.push(&text, state.style()),
            Event::Code(text) => {
                let mut style = state.style();
                style.code = true;
                state.push(&text, style);
            }
            Event::SoftBreak => state.push(" ", state.style()),
            Event::HardBreak => state.push("\n", state.style()),
            Event::TaskListMarker(checked) => {
                state.prefix = Some(if checked { "☑" } else { "☐" }.to_owned());
            }
            Event::Rule => {
                state.flush();
                state.blocks.push(RawBlock::Rule);
            }
            Event::Html(text) | Event::InlineHtml(text) => state.push(&text, state.style()),
            Event::FootnoteReference(text) => state.push(&format!("[{text}]"), state.style()),
            _ => {}
        }
    }
    state.flush();
    state.blocks
}

fn styled_paragraph(
    runs: &[Run],
    size: f32,
    color: Color,
    width: f32,
    fonts: &FontCollection,
    bold: bool,
) -> Paragraph {
    let mut base = text_style(size, color, false);
    if bold {
        base.set_font_style(FontStyle::bold());
    }
    let mut para_style = ParagraphStyle::new();
    para_style.set_text_style(&base);
    let mut builder = ParagraphBuilder::new(&para_style, fonts.clone());
    for run in runs {
        let mut style = text_style(
            if run.style.code { size * 0.92 } else { size },
            if run.style.link {
                LINK
            } else if run.style.code {
                Color::from_rgb(196, 180, 246)
            } else {
                color
            },
            run.style.code,
        );
        style.set_font_style(match (bold || run.style.bold, run.style.italic) {
            (true, true) => FontStyle::bold_italic(),
            (true, false) => FontStyle::bold(),
            (false, true) => FontStyle::italic(),
            _ => FontStyle::normal(),
        });
        let mut decorations = TextDecoration::default();
        if run.style.strike {
            decorations |= TextDecoration::LINE_THROUGH;
        }
        if run.style.link {
            decorations |= TextDecoration::UNDERLINE;
        }
        style.set_decoration_type(decorations);
        if run.style.code {
            let mut bg = Paint::default();
            bg.set_color(CODE_BG);
            style.set_background_paint(&bg);
        }
        builder.push_style(&style).add_text(&run.text).pop();
    }
    let mut result = builder.build();
    result.layout(width.max(1.0));
    result
}

enum LayoutKind {
    Text {
        paragraph: Paragraph,
        x: f32,
        quote: bool,
        marker: Option<Paragraph>,
    },
    Code {
        paragraph: Paragraph,
        number: Option<Paragraph>,
        label: bool,
    },
    Rule,
    Table {
        cells: Vec<Paragraph>,
        header: bool,
    },
}

struct LayoutBlock {
    y: f32,
    height: f32,
    kind: LayoutKind,
}

pub struct MarkdownDocument {
    pub height: f32,
    pub block_count: usize,
    width: f32,
    blocks: Vec<LayoutBlock>,
}

impl MarkdownDocument {
    pub fn new(
        source: &str,
        width: f32,
        fonts: &FontCollection,
        highlighter: &Highlighter,
    ) -> Self {
        let width = width.max(64.0);
        let mut document = Self {
            height: 0.0,
            block_count: 0,
            width,
            blocks: Vec::new(),
        };
        let mut after_table = false;
        for raw in parse(source) {
            if after_table && !matches!(&raw, RawBlock::TableRow { .. }) {
                document.height += 16.0;
            }
            after_table = matches!(&raw, RawBlock::TableRow { .. });
            match raw {
                RawBlock::Text {
                    runs,
                    heading,
                    quote_depth,
                    list_depth,
                    list_indent,
                    prefix,
                } => {
                    let size = match heading {
                        1 => 31.0,
                        2 => 24.0,
                        3 => 20.0,
                        4 => 18.0,
                        5 | 6 => 16.0,
                        _ => 16.0,
                    };
                    let x = quote_depth as f32 * 18.0 + list_indent;
                    let color = if heading > 0 {
                        HEADING
                    } else if quote_depth > 0 {
                        MUTED
                    } else {
                        TEXT
                    };
                    let paragraph =
                        styled_paragraph(&runs, size, color, width - x, fonts, heading > 0);
                    let marker =
                        prefix.map(|text| self::paragraph(&text, 15.0, MUTED, 128.0, fonts));
                    let height = paragraph.height().max(size * 1.45);
                    if heading > 0 && document.height > 0.0 {
                        document.height += 13.0;
                    }
                    document.push(
                        height,
                        LayoutKind::Text {
                            paragraph,
                            x,
                            quote: quote_depth > 0,
                            marker,
                        },
                    );
                    document.height += if list_depth > 0 { 6.0 } else { 13.0 };
                }
                RawBlock::Code { source, language } => {
                    document.code(&source, &language, fonts, highlighter)
                }
                RawBlock::Rule => {
                    document.height += 6.0;
                    document.push(1.0, LayoutKind::Rule);
                    document.height += 20.0;
                }
                RawBlock::TableRow { cells, header } => {
                    if cells.is_empty() {
                        continue;
                    }
                    let cell_width = width / cells.len() as f32;
                    let cells: Vec<_> = cells
                        .iter()
                        .map(|runs| {
                            styled_paragraph(
                                runs,
                                14.0,
                                if header { HEADING } else { TEXT },
                                cell_width - 24.0,
                                fonts,
                                header,
                            )
                        })
                        .collect();
                    let height =
                        cells.iter().map(Paragraph::height).fold(20.0_f32, f32::max) + 20.0;
                    document.push(height, LayoutKind::Table { cells, header });
                }
            }
        }
        document.height = document.height.max(24.0);
        document.block_count = document.blocks.len();
        document
    }

    fn push(&mut self, height: f32, kind: LayoutKind) {
        self.blocks.push(LayoutBlock {
            y: self.height,
            height,
            kind,
        });
        self.height += height;
    }

    fn code(
        &mut self,
        source: &str,
        language: &str,
        fonts: &FontCollection,
        highlighter: &Highlighter,
    ) {
        let token = match language.to_ascii_lowercase().as_str() {
            "typescript" | "ts" | "tsx" => "js",
            "rust" => "rs",
            "python" => "py",
            "shell" | "bash" => "sh",
            _ => language,
        };
        let syntax = highlighter
            .syntaxes
            .find_syntax_by_token(token)
            .unwrap_or_else(|| highlighter.syntaxes.find_syntax_plain_text());
        let mut highlight = HighlightLines::new(syntax, &highlighter.theme);
        let label = paragraph(
            if language.is_empty() {
                "CODE"
            } else {
                language
            },
            11.0,
            MUTED,
            self.width - 32.0,
            fonts,
        );
        self.push(
            32.0,
            LayoutKind::Code {
                paragraph: label,
                number: None,
                label: true,
            },
        );
        for (index, line) in LinesWithEndings::from(source).enumerate() {
            let mut style = ParagraphStyle::new();
            style.set_text_style(&text_style(14.0, TEXT, true));
            let mut builder = ParagraphBuilder::new(&style, fonts.clone());
            match highlight.highlight_line(line, &highlighter.syntaxes) {
                Ok(tokens) => {
                    for (token_style, text) in tokens {
                        let foreground = token_style.foreground;
                        let mut style = text_style(
                            14.0,
                            Color::from_argb(
                                foreground.a,
                                foreground.r,
                                foreground.g,
                                foreground.b,
                            ),
                            true,
                        );
                        style.set_font_style(
                            match (
                                token_style.font_style.contains(SyntaxFontStyle::BOLD),
                                token_style.font_style.contains(SyntaxFontStyle::ITALIC),
                            ) {
                                (true, true) => FontStyle::bold_italic(),
                                (true, false) => FontStyle::bold(),
                                (false, true) => FontStyle::italic(),
                                _ => FontStyle::normal(),
                            },
                        );
                        builder
                            .push_style(&style)
                            .add_text(text.trim_end_matches(['\r', '\n']))
                            .pop();
                    }
                }
                Err(_) => {
                    builder.add_text(line.trim_end_matches(['\r', '\n']));
                }
            }
            let mut paragraph = builder.build();
            paragraph.layout((self.width - 66.0).max(1.0));
            let height = paragraph.height().max(20.5);
            let number = Some(self::paragraph(
                &format!("{}", index + 1),
                11.0,
                Color::from_rgb(91, 106, 126),
                34.0,
                fonts,
            ));
            self.push(
                height,
                LayoutKind::Code {
                    paragraph,
                    number,
                    label: false,
                },
            );
        }
        self.push(
            12.0,
            LayoutKind::Code {
                paragraph: paragraph("", 1.0, TEXT, 1.0, fonts),
                number: None,
                label: false,
            },
        );
        self.height += 16.0;
    }

    pub fn paint(&self, canvas: &Canvas, visible_top: f32, visible_bottom: f32) -> usize {
        let top = visible_top.max(0.0);
        let bottom = visible_bottom.min(self.height);
        if bottom <= top {
            return 0;
        }
        canvas.save();
        canvas.clip_rect(Rect::new(0.0, top, self.width, bottom), None, false);
        let start = self
            .blocks
            .partition_point(|block| block.y + block.height < visible_top);
        let mut count = 0;
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        for block in &self.blocks[start..] {
            if block.y > visible_bottom {
                break;
            }
            match &block.kind {
                LayoutKind::Text {
                    paragraph,
                    x,
                    quote,
                    marker,
                } => {
                    if *quote {
                        paint.set_color(Color::from_rgb(87, 117, 158));
                        canvas.draw_rect(
                            Rect::from_xywh(0.0, block.y + 1.0, 3.0, block.height - 2.0),
                            &paint,
                        );
                    }
                    if let Some(marker) = marker {
                        marker.paint(
                            canvas,
                            (*x - marker.max_intrinsic_width() - 8.0, block.y + 1.0),
                        );
                    }
                    paragraph.paint(canvas, (*x, block.y));
                }
                LayoutKind::Code {
                    paragraph,
                    number,
                    label,
                } => {
                    paint.set_color(CODE_BG);
                    canvas.draw_rect(
                        Rect::from_xywh(0.0, block.y, self.width, block.height + 0.3),
                        &paint,
                    );
                    if let Some(number) = number {
                        number.paint(canvas, (13.0, block.y + 2.0));
                    }
                    paragraph.paint(
                        canvas,
                        (
                            if *label { 14.0 } else { 50.0 },
                            block.y + if *label { 8.0 } else { 0.0 },
                        ),
                    );
                }
                LayoutKind::Rule => {
                    paint.set_color(RULE);
                    canvas.draw_rect(Rect::from_xywh(0.0, block.y, self.width, 1.0), &paint);
                }
                LayoutKind::Table { cells, header } => {
                    paint.set_color(if *header {
                        Color::from_rgb(36, 46, 60)
                    } else {
                        Color::from_rgb(27, 35, 47)
                    });
                    canvas.draw_rect(
                        Rect::from_xywh(0.0, block.y, self.width, block.height),
                        &paint,
                    );
                    paint.set_color(RULE);
                    canvas.draw_rect(
                        Rect::from_xywh(0.0, block.y + block.height - 1.0, self.width, 1.0),
                        &paint,
                    );
                    let width = self.width / cells.len() as f32;
                    for (index, cell) in cells.iter().enumerate() {
                        cell.paint(canvas, (index as f32 * width + 12.0, block.y + 10.0));
                    }
                }
            }
            count += 1;
        }
        canvas.restore();
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_inline_styles_survive_heading_and_unicode() {
        let blocks = parse("# 标题 **粗体 *强调*** 和 `代码`\n\n普通段落。");
        let RawBlock::Text { runs, heading, .. } = &blocks[0] else {
            panic!("heading expected")
        };
        assert_eq!(*heading, 1);
        assert!(
            runs.iter()
                .any(|run| run.style.bold && run.style.italic && run.text == "强调")
        );
        assert!(runs.iter().any(|run| run.style.code && run.text == "代码"));
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn ordered_nested_lists_keep_counters_and_depth() {
        let blocks = parse("3. first\n   - nested\n4. second\n");
        let items: Vec<_> = blocks
            .iter()
            .filter_map(|block| match block {
                RawBlock::Text {
                    prefix, list_depth, ..
                } => Some((prefix.as_deref(), *list_depth)),
                _ => None,
            })
            .collect();
        assert_eq!(
            items,
            vec![(Some("3."), 1), (Some("•"), 2), (Some("4."), 1)]
        );
    }

    #[test]
    fn fenced_code_preserves_whitespace_and_tables_keep_cells() {
        let blocks =
            parse("```rust\n  let x = 1;\n\n```\n\n| A | B |\n|---|---|\n| **中** | x |\n");
        let RawBlock::Code { source, language } = &blocks[0] else {
            panic!("code expected")
        };
        assert_eq!(source, "  let x = 1;\n\n");
        assert_eq!(language, "rust");
        assert!(
            matches!(&blocks[1], RawBlock::TableRow { cells, header: true } if cells.len() == 2)
        );
        assert!(
            matches!(&blocks[2], RawBlock::TableRow { cells, header: false } if cells[0][0].style.bold)
        );
    }

    #[test]
    fn tasks_replace_bullets_and_wide_number_gutters_persist_across_paragraphs() {
        let tasks = parse("- [x] done\n- [ ] pending\n");
        assert!(
            matches!(&tasks[0], RawBlock::Text { prefix: Some(marker), runs, .. }
            if marker == "☑" && runs[0].text == "done")
        );
        assert!(
            matches!(&tasks[1], RawBlock::Text { prefix: Some(marker), .. }
            if marker == "☐")
        );

        let numbered = parse("12345. first\n\n       continuation\n");
        let RawBlock::Text {
            list_indent: first,
            prefix: Some(marker),
            ..
        } = &numbered[0]
        else {
            panic!("numbered item expected")
        };
        let RawBlock::Text {
            list_indent: second,
            prefix: None,
            ..
        } = &numbered[1]
        else {
            panic!("continuation expected")
        };
        assert_eq!(marker, "12345.");
        assert!(*first >= 68.0);
        assert_eq!(first, second);
    }
}
