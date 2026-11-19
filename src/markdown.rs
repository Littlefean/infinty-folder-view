use std::{
    cell::{Ref, RefCell},
    ops::Range,
};

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use skia_safe::{
    Canvas, Color, FontMgr, FontStyle, Paint, Rect,
    textlayout::{
        Affinity, FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, RectHeightStyle,
        RectWidthStyle, TextDecoration, TextStyle,
    },
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle as SyntaxFontStyle, Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};
use unicode_segmentation::UnicodeSegmentation;

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

#[derive(Debug)]
struct TextBoundary {
    byte: usize,
    utf16: usize,
}

#[derive(Debug)]
struct TextMapping {
    range: Range<usize>,
    boundaries: Vec<TextBoundary>,
    selection_cache: RefCell<Option<SelectionGeometry>>,
}

#[derive(Debug)]
struct SelectionGeometry {
    range: Range<usize>,
    rects: Vec<Rect>,
    prefix_bottom: Vec<f32>,
}

impl SelectionGeometry {
    fn new(range: Range<usize>, mut rects: Vec<Rect>) -> Self {
        rects.sort_by(|a, b| a.top.total_cmp(&b.top));
        let mut bottom = f32::NEG_INFINITY;
        let prefix_bottom = rects
            .iter()
            .map(|rect| {
                bottom = bottom.max(rect.bottom);
                bottom
            })
            .collect();
        Self {
            range,
            rects,
            prefix_bottom,
        }
    }

    fn visible(&self, top: f32, bottom: f32) -> impl Iterator<Item = &Rect> {
        let start = self.prefix_bottom.partition_point(|last| *last < top);
        self.rects[start..]
            .iter()
            .take_while(move |rect| rect.top <= bottom)
            .filter(move |rect| rect.bottom >= top)
    }
}

impl TextMapping {
    fn new(text: &str, start: usize) -> Self {
        let mut boundaries = Vec::new();
        let mut utf16 = 0;
        for (byte, grapheme) in text.grapheme_indices(true) {
            boundaries.push(TextBoundary { byte, utf16 });
            utf16 += grapheme.encode_utf16().count();
        }
        boundaries.push(TextBoundary {
            byte: text.len(),
            utf16,
        });
        Self {
            range: start..start + text.len(),
            boundaries,
            selection_cache: RefCell::new(None),
        }
    }

    fn byte_to_utf16(&self, byte: usize) -> usize {
        let index = self
            .boundaries
            .partition_point(|boundary| boundary.byte <= byte);
        self.boundaries[index.saturating_sub(1)].utf16
    }

    fn utf16_to_byte(&self, utf16: usize, ceil: bool) -> usize {
        let utf16 = utf16.min(self.boundaries.last().unwrap().utf16);
        let index = self
            .boundaries
            .partition_point(|boundary| boundary.utf16 < utf16);
        let upper = index.min(self.boundaries.len() - 1);
        if ceil || self.boundaries[upper].utf16 == utf16 {
            self.boundaries[upper].byte
        } else {
            self.boundaries[upper.saturating_sub(1)].byte
        }
    }

    fn nearest_byte(&self, utf16: usize, downstream: bool) -> usize {
        let index = self
            .boundaries
            .partition_point(|boundary| boundary.utf16 < utf16);
        let upper = &self.boundaries[index.min(self.boundaries.len() - 1)];
        let lower = &self.boundaries[index.saturating_sub(1)];
        let before = utf16.saturating_sub(lower.utf16);
        let after = upper.utf16.saturating_sub(utf16);
        if before < after || (before == after && !downstream) {
            lower.byte
        } else {
            upper.byte
        }
    }

    fn selection_geometry(
        &self,
        paragraph: &Paragraph,
        range: Range<usize>,
    ) -> Ref<'_, SelectionGeometry> {
        if self
            .selection_cache
            .borrow()
            .as_ref()
            .is_none_or(|cached| cached.range != range)
        {
            let rects = paragraph
                .get_rects_for_range(range.clone(), RectHeightStyle::Max, RectWidthStyle::Tight)
                .into_iter()
                .map(|text_box| text_box.rect)
                .collect();
            *self.selection_cache.borrow_mut() = Some(SelectionGeometry::new(range, rects));
        }
        Ref::map(self.selection_cache.borrow(), |cached| {
            cached.as_ref().unwrap()
        })
    }
}

struct TextCell {
    paragraph: Paragraph,
    text: TextMapping,
}

#[derive(Clone, Copy)]
struct TextLocation {
    block: usize,
    cell: usize,
}

struct Selectable<'a> {
    paragraph: &'a Paragraph,
    text: &'a TextMapping,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Selectable<'_> {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    fn hit(&self, x: f32, y: f32) -> usize {
        let position = self
            .paragraph
            .get_glyph_position_at_coordinate((x - self.x, y - self.y));
        self.text.range.start
            + self.text.nearest_byte(
                position.position.max(0) as usize,
                position.affinity == Affinity::Downstream,
            )
    }
}

enum LayoutKind {
    Text {
        paragraph: Paragraph,
        text: TextMapping,
        x: f32,
        quote: bool,
        marker: Option<Paragraph>,
    },
    Code {
        paragraph: Paragraph,
        text: Option<TextMapping>,
        number: Option<Paragraph>,
        label: bool,
    },
    Rule,
    Table {
        cells: Vec<TextCell>,
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
    text: String,
    text_started: bool,
    text_boundaries: Vec<usize>,
    selectable: Vec<TextLocation>,
}

impl MarkdownDocument {
    fn empty(width: f32) -> Self {
        Self {
            height: 0.0,
            block_count: 0,
            width: width.max(64.0),
            blocks: Vec::new(),
            text: String::new(),
            text_started: false,
            text_boundaries: Vec::new(),
            selectable: Vec::new(),
        }
    }

    fn finish(mut self) -> Self {
        self.height = self.height.max(24.0);
        self.block_count = self.blocks.len();
        self.text_boundaries = self
            .text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .collect();
        self.text_boundaries.push(self.text.len());
        self
    }

    /// A source file is literal text, so Markdown delimiters never affect its layout.
    /// Keep original line endings in the selection buffer, including the final newline.
    pub fn from_code(
        source: &str,
        language: &str,
        width: f32,
        fonts: &FontCollection,
        highlighter: &Highlighter,
    ) -> Self {
        let mut document = Self::empty(width);
        document.code(source, language, fonts, highlighter, true);
        document.finish()
    }

    /// Plain text keeps its line breaks and whitespace without interpreting markup.
    pub fn from_text(source: &str, width: f32, fonts: &FontCollection) -> Self {
        let mut document = Self::empty(width);
        for line in source
            .split_inclusive('\n')
            .chain((source.is_empty() || source.ends_with('\n')).then_some(""))
        {
            let content = line.strip_suffix('\n').unwrap_or(line);
            let content = if line.ends_with('\n') {
                content.strip_suffix('\r').unwrap_or(content)
            } else {
                content
            };
            let paragraph = paragraph(content, 16.0, TEXT, document.width, fonts);
            let height = paragraph.height().max(16.0 * 1.45);
            let text = document.append_text(content);
            document.text.push_str(&line[content.len()..]);
            document.push(
                height,
                LayoutKind::Text {
                    paragraph,
                    text,
                    x: 0.0,
                    quote: false,
                    marker: None,
                },
            );
        }
        document.finish()
    }

    pub fn new(
        source: &str,
        width: f32,
        fonts: &FontCollection,
        highlighter: &Highlighter,
    ) -> Self {
        let width = width.max(64.0);
        let mut document = Self::empty(width);
        let mut after_table = false;
        for raw in parse(source) {
            let continues_table = after_table;
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
                    document.begin_text_section("\n\n");
                    let content: String = runs.iter().map(|run| run.text.as_str()).collect();
                    let text = document.append_text(&content);
                    document.push(
                        height,
                        LayoutKind::Text {
                            paragraph,
                            text,
                            x,
                            quote: quote_depth > 0,
                            marker,
                        },
                    );
                    document.height += if list_depth > 0 { 6.0 } else { 13.0 };
                }
                RawBlock::Code { source, language } => {
                    document.begin_text_section("\n\n");
                    document.code(&source, &language, fonts, highlighter, false)
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
                    document.begin_text_section(if continues_table { "\n" } else { "\n\n" });
                    let mut laid_cells = Vec::new();
                    for (index, runs) in cells.iter().enumerate() {
                        if index > 0 {
                            document.text.push('\t');
                        }
                        let content: String = runs.iter().map(|run| run.text.as_str()).collect();
                        let text = document.append_text(&content);
                        let paragraph = styled_paragraph(
                            runs,
                            14.0,
                            if header { HEADING } else { TEXT },
                            cell_width - 24.0,
                            fonts,
                            header,
                        );
                        laid_cells.push(TextCell { paragraph, text });
                    }
                    let height = laid_cells
                        .iter()
                        .map(|cell| cell.paragraph.height())
                        .fold(20.0_f32, f32::max)
                        + 20.0;
                    let cells = laid_cells;
                    document.push(height, LayoutKind::Table { cells, header });
                }
            }
        }
        document.finish()
    }

    fn begin_text_section(&mut self, separator: &str) {
        if self.text_started {
            self.text.push_str(separator);
        }
        self.text_started = true;
    }

    fn append_text(&mut self, text: &str) -> TextMapping {
        let mapping = TextMapping::new(text, self.text.len());
        self.text.push_str(text);
        mapping
    }

    fn push(&mut self, height: f32, kind: LayoutKind) {
        let block = self.blocks.len();
        match &kind {
            LayoutKind::Text { .. } | LayoutKind::Code { text: Some(_), .. } => {
                self.selectable.push(TextLocation { block, cell: 0 })
            }
            LayoutKind::Table { cells, .. } => self
                .selectable
                .extend((0..cells.len()).map(|cell| TextLocation { block, cell })),
            _ => {}
        }
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
        preserve_source: bool,
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
                text: None,
                number: None,
                label: true,
            },
        );
        let final_line = preserve_source && (source.is_empty() || source.ends_with('\n'));
        for (index, line) in LinesWithEndings::from(source)
            .chain(final_line.then_some(""))
            .enumerate()
        {
            if index > 0 && !preserve_source {
                self.text.push('\n');
            }
            let content = line.trim_end_matches(['\r', '\n']);
            let text = self.append_text(content);
            if preserve_source {
                self.text.push_str(&line[content.len()..]);
            }
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
                    text: Some(text),
                    number,
                    label: false,
                },
            );
        }
        self.push(
            12.0,
            LayoutKind::Code {
                paragraph: paragraph("", 1.0, TEXT, 1.0, fonts),
                text: None,
                number: None,
                label: false,
            },
        );
        self.height += 16.0;
    }

    fn selectable_at(&self, location: TextLocation) -> Selectable<'_> {
        let block = &self.blocks[location.block];
        match &block.kind {
            LayoutKind::Text {
                paragraph, text, x, ..
            } => Selectable {
                paragraph,
                text,
                x: *x,
                y: block.y,
                width: (self.width - x).max(1.0),
                height: block.height,
            },
            LayoutKind::Code {
                paragraph,
                text: Some(text),
                ..
            } => Selectable {
                paragraph,
                text,
                x: 50.0,
                y: block.y,
                width: (self.width - 66.0).max(1.0),
                height: block.height,
            },
            LayoutKind::Table { cells, .. } => {
                let cell = &cells[location.cell];
                let width = self.width / cells.len() as f32;
                Selectable {
                    paragraph: &cell.paragraph,
                    text: &cell.text,
                    x: location.cell as f32 * width + 12.0,
                    y: block.y + 10.0,
                    width: (width - 24.0).max(1.0),
                    height: block.height - 20.0,
                }
            }
            _ => unreachable!("only text-bearing paragraphs enter the selection index"),
        }
    }

    pub fn hit_test(&self, x: f32, y: f32, clamp: bool) -> Option<usize> {
        let text = self.hit_target(x, y, clamp)?;
        if clamp {
            let first = self.selectable_at(self.selectable[0]);
            let last = self.selectable_at(*self.selectable.last().unwrap());
            if y < first.y {
                return Some(0);
            }
            if y > last.y + last.height {
                return Some(self.text.len());
            }
        }
        Some(text.hit(x, y))
    }

    pub fn hit_test_word(&self, x: f32, y: f32, clamp: bool) -> Option<Range<usize>> {
        let text = self.hit_target(x, y, clamp)?;
        if clamp {
            let first = self.selectable_at(self.selectable[0]);
            let last = self.selectable_at(*self.selectable.last().unwrap());
            if y < first.y {
                return Some(self.word_range(0));
            }
            if y > last.y + last.height {
                return Some(self.word_range(self.text.len()));
            }
        }
        let hit = text
            .paragraph
            .get_glyph_position_at_coordinate((x - text.x, y - text.y));
        let mut utf16 = hit.position.max(0) as usize;
        // A trailing-edge caret belongs to the glyph before it. Keeping this affinity
        // avoids treating the right half of a word's final glyph as the following space.
        if hit.affinity == Affinity::Upstream {
            utf16 = utf16.saturating_sub(1);
        }
        let local = text.text.utf16_to_byte(utf16, false);
        Some(self.word_range(text.text.range.start + local))
    }

    fn hit_target(&self, x: f32, y: f32, clamp: bool) -> Option<Selectable<'_>> {
        if self.selectable.is_empty() || !x.is_finite() || !y.is_finite() {
            return None;
        }
        let mut index = self
            .selectable
            .partition_point(|location| {
                let text = self.selectable_at(*location);
                text.y + text.height < y
            })
            .min(self.selectable.len() - 1);
        if clamp && index > 0 {
            let next = self.selectable_at(self.selectable[index]);
            let previous = self.selectable_at(self.selectable[index - 1]);
            if y < next.y && y - (previous.y + previous.height) < next.y - y {
                index -= 1;
            }
        }
        let block = self.selectable[index].block;
        let first = self
            .selectable
            .partition_point(|location| location.block < block);
        let end = self
            .selectable
            .partition_point(|location| location.block <= block);
        let mut closest: Option<(f32, TextLocation)> = None;
        for location in &self.selectable[first..end] {
            let text = self.selectable_at(*location);
            if text.contains(x, y) {
                return Some(text);
            }
            if clamp {
                let dx = (text.x - x).max(0.0).max(x - (text.x + text.width));
                if closest.as_ref().is_none_or(|(distance, _)| dx < *distance) {
                    closest = Some((dx, *location));
                }
            }
        }
        closest.map(|(_, location)| self.selectable_at(location))
    }

    fn text_at_position(&self, position: usize) -> Option<Selectable<'_>> {
        if self.selectable.is_empty() {
            return None;
        }
        let position = position.min(self.text.len());
        let index = self
            .selectable
            .partition_point(|location| self.selectable_at(*location).text.range.end < position)
            .min(self.selectable.len() - 1);
        Some(self.selectable_at(self.selectable[index]))
    }

    pub fn word_range(&self, position: usize) -> Range<usize> {
        let Some(text) = self.text_at_position(position) else {
            return 0..0;
        };
        let local = position
            .saturating_sub(text.text.range.start)
            .min(text.text.range.len());
        let local = if local == text.text.range.len() && local > 0 {
            text.text.boundaries[text.text.boundaries.len().saturating_sub(2)].byte
        } else {
            local
        };
        let utf16 = text.text.byte_to_utf16(local);
        let word = text.paragraph.get_word_boundary(utf16 as u32);
        let mut range = text.text.range.start + text.text.utf16_to_byte(word.start, false)
            ..text.text.range.start + text.text.utf16_to_byte(word.end, true);
        if range.is_empty() && !text.text.range.is_empty() {
            let index = text
                .text
                .boundaries
                .partition_point(|boundary| boundary.byte <= local)
                .saturating_sub(1);
            range = text.text.range.start + text.text.boundaries[index].byte
                ..text.text.range.start
                    + text.text.boundaries[(index + 1).min(text.text.boundaries.len() - 1)].byte;
        }
        range
    }

    pub fn paragraph_range(&self, position: usize) -> Range<usize> {
        self.text_at_position(position)
            .map(|text| text.text.range.clone())
            .unwrap_or(0..0)
    }

    pub fn text_len(&self) -> usize {
        self.text.len()
    }

    fn normalized_range(&self, range: Range<usize>) -> Range<usize> {
        let start = range.start.min(range.end).min(self.text.len());
        let end = range.start.max(range.end).min(self.text.len());
        let first = self
            .text_boundaries
            .partition_point(|boundary| *boundary <= start)
            .saturating_sub(1);
        let start = self.text_boundaries[first];
        if range.start == range.end {
            return start..start;
        }
        let last = self
            .text_boundaries
            .partition_point(|boundary| *boundary < end)
            .min(self.text_boundaries.len() - 1);
        start..self.text_boundaries[last]
    }

    pub fn selected_text(&self, range: Range<usize>) -> String {
        self.text[self.normalized_range(range)].to_owned()
    }

    pub fn selection_char_count(&self, range: Range<usize>) -> usize {
        let range = self.normalized_range(range);
        let first = self
            .text_boundaries
            .partition_point(|boundary| *boundary < range.start);
        let last = self
            .text_boundaries
            .partition_point(|boundary| *boundary < range.end);
        last - first
    }

    fn paint_selection(
        canvas: &Canvas,
        paragraph: &Paragraph,
        text: &TextMapping,
        origin: (f32, f32),
        selection: &Option<Range<usize>>,
        visible: Range<f32>,
    ) {
        let Some(selection) = selection else {
            return;
        };
        let start = selection.start.max(text.range.start);
        let end = selection.end.min(text.range.end);
        if start >= end {
            return;
        }
        let range = text.byte_to_utf16(start - text.range.start)
            ..text.byte_to_utf16(end - text.range.start);
        let mut paint = Paint::default();
        paint.set_color(Color::from_argb(88, 76, 147, 245));
        let (x, y) = origin;
        let geometry = text.selection_geometry(paragraph, range);
        for rect in geometry.visible(visible.start - y, visible.end - y) {
            canvas.draw_rect(
                Rect::new(rect.left + x, rect.top + y, rect.right + x, rect.bottom + y),
                &paint,
            );
        }
    }

    pub fn paint(
        &self,
        canvas: &Canvas,
        visible_top: f32,
        visible_bottom: f32,
        selection: Option<Range<usize>>,
    ) -> usize {
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
        let selection = selection.map(|range| self.normalized_range(range));
        for block in &self.blocks[start..] {
            if block.y > visible_bottom {
                break;
            }
            match &block.kind {
                LayoutKind::Text {
                    paragraph,
                    text,
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
                    Self::paint_selection(
                        canvas,
                        paragraph,
                        text,
                        (*x, block.y),
                        &selection,
                        top..bottom,
                    );
                }
                LayoutKind::Code {
                    paragraph,
                    text,
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
                    if let Some(text) = text {
                        Self::paint_selection(
                            canvas,
                            paragraph,
                            text,
                            (50.0, block.y),
                            &selection,
                            top..bottom,
                        );
                    }
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
                        let x = index as f32 * width + 12.0;
                        let y = block.y + 10.0;
                        cell.paragraph.paint(canvas, (x, y));
                        Self::paint_selection(
                            canvas,
                            &cell.paragraph,
                            &cell.text,
                            (x, y),
                            &selection,
                            top..bottom,
                        );
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

    fn laid_document(source: &str, width: f32) -> MarkdownDocument {
        MarkdownDocument::new(source, width, &font_collection(), &Highlighter::new())
    }

    #[test]
    fn utf16_offsets_snap_to_whole_graphemes_and_rust_boundaries() {
        let source = "A中👩‍💻e\u{301}Z";
        let map = TextMapping::new(source, 7);
        let boundaries: Vec<_> = map.boundaries.iter().map(|b| (b.byte, b.utf16)).collect();
        assert_eq!(
            boundaries,
            vec![(0, 0), (1, 1), (4, 2), (15, 7), (18, 9), (19, 10)]
        );
        assert_eq!(map.utf16_to_byte(3, false), 4);
        assert_eq!(map.utf16_to_byte(3, true), 15);
        assert_eq!(map.nearest_byte(6, true), 15);
        assert_eq!(map.byte_to_utf16(16), 7);
        for utf16 in 0..=12 {
            let byte = map.nearest_byte(utf16, true);
            assert!(source.is_char_boundary(byte));
            assert!(map.boundaries.iter().any(|boundary| boundary.byte == byte));
        }
    }

    #[test]
    fn copying_normalizes_reverse_ranges_and_preserves_display_text() {
        let document = laid_document("# 中文 **bold**\n\n第二段 👩‍💻 e\u{301}", 320.0);
        let expected = "中文 bold\n\n第二段 👩‍💻 e\u{301}";
        assert_eq!(document.selected_text(0..document.text_len()), expected);
        assert_eq!(
            document.selected_text(Range {
                start: document.text_len(),
                end: 0
            }),
            expected
        );
        let accent = expected.find("e\u{301}").unwrap();
        assert_eq!(document.selected_text(accent + 1..accent + 2), "e\u{301}");
        let emoji = expected.find('👩').unwrap();
        assert_eq!(document.selected_text(emoji + 2..emoji + 6), "👩‍💻");
        assert_eq!(document.selection_char_count(emoji + 2..emoji + 6), 1);
        assert_eq!(document.selection_char_count(accent + 1..accent + 2), 1);
        assert_eq!(
            document.selection_char_count(Range {
                start: document.text_len(),
                end: 0
            }),
            expected.graphemes(true).count()
        );
        assert_eq!(document.selected_text(accent + 1..accent + 1), "");
        assert_eq!(document.selection_char_count(accent + 1..accent + 1), 0);
        let second = expected.find("第二段").unwrap();
        assert_eq!(
            document.selected_text(document.paragraph_range(second)),
            "第二段 👩‍💻 e\u{301}"
        );
        assert_eq!(
            document.selected_text(document.word_range(expected.find("bold").unwrap() + 1)),
            "bold"
        );
    }

    #[test]
    fn code_copy_keeps_indentation_empty_lines_and_excludes_chrome() {
        let document = laid_document("before\n\n```text\n  alpha\n\n\tbeta\n```\n\nafter", 320.0);
        assert_eq!(
            document.selected_text(0..document.text_len()),
            "before\n\n  alpha\n\n\tbeta\n\nafter"
        );
        let code = document.text.find("  alpha").unwrap();
        assert_eq!(
            document.selected_text(document.paragraph_range(code)),
            "  alpha"
        );
        let code_block = document
            .blocks
            .iter()
            .find(|block| matches!(&block.kind, LayoutKind::Code { text: Some(_), .. }))
            .unwrap();
        assert_eq!(document.hit_test(16.0, code_block.y + 5.0, false), None);
        assert!(document.hit_test(55.0, code_block.y + 5.0, false).is_some());
    }

    #[test]
    fn source_code_is_literal_and_copies_original_line_endings() {
        let source = "# literal heading\r\nconst text = `**中👩‍💻 e\u{301}**`;\r\n\r\n\t```\r\n";
        let document = MarkdownDocument::from_code(
            source,
            "typescript",
            320.0,
            &font_collection(),
            &Highlighter::new(),
        );
        assert_eq!(document.selected_text(0..document.text_len()), source);
        assert!(
            document
                .blocks
                .iter()
                .all(|block| matches!(block.kind, LayoutKind::Code { .. }))
        );
        let lines = document
            .blocks
            .iter()
            .filter(|block| matches!(block.kind, LayoutKind::Code { text: Some(_), .. }))
            .count();
        assert_eq!(lines, 5);
        let second = source.find("const").unwrap();
        assert_eq!(
            document.selected_text(document.paragraph_range(second)),
            "const text = `**中👩‍💻 e\u{301}**`;"
        );
        let first = document
            .blocks
            .iter()
            .find(|block| matches!(block.kind, LayoutKind::Code { text: Some(_), .. }))
            .unwrap();
        assert_eq!(document.hit_test(50.0, first.y + 5.0, false), Some(0));
    }

    #[test]
    fn literal_text_keeps_markup_blank_lines_and_terminal_newline() {
        let source = "# **原样** [link](url)\r\n\r\n  中👩‍💻 e\u{301}\n";
        let document = MarkdownDocument::from_text(source, 240.0, &font_collection());
        assert_eq!(document.selected_text(0..document.text_len()), source);
        assert_eq!(document.block_count, 4);
        assert!(
            document
                .blocks
                .iter()
                .all(|block| matches!(block.kind, LayoutKind::Text { .. }))
        );
        assert_eq!(
            document.selected_text(document.paragraph_range(0)),
            "# **原样** [link](url)"
        );
    }

    #[test]
    fn empty_and_unterminated_source_files_have_a_readable_line() {
        let fonts = font_collection();
        let highlighter = Highlighter::new();
        for source in ["", "  hello\t中", "\n", "\n\n"] {
            let code = MarkdownDocument::from_code(source, "unknown", 300.0, &fonts, &highlighter);
            let text = MarkdownDocument::from_text(source, 300.0, &fonts);
            assert_eq!(code.selected_text(0..code.text_len()), source);
            assert_eq!(text.selected_text(0..text.text_len()), source);
            assert!(code.height > 0.0 && text.height > 0.0);
            assert!(!code.selectable.is_empty() && !text.selectable.is_empty());
        }
    }

    #[test]
    fn table_copy_uses_tabs_and_rows_and_cell_paragraph_ranges() {
        let document = laid_document(
            "| A | B |\n|---|---|\n| **中** | e\u{301} |\n| C | D |",
            320.0,
        );
        assert_eq!(
            document.selected_text(0..document.text_len()),
            "A\tB\n中\te\u{301}\nC\tD"
        );
        let cell = document.text.find('中').unwrap();
        assert_eq!(document.selected_text(document.paragraph_range(cell)), "中");
        assert_eq!(
            document.selected_text(cell..document.text_len()),
            "中\te\u{301}\nC\tD"
        );
    }

    #[test]
    fn skparagraph_rects_and_hits_round_trip_after_wrapping() {
        let document = laid_document(
            "Hello 中文 e\u{301} 👩‍💻 with enough words to wrap onto another line.",
            140.0,
        );
        let text = document.selectable_at(document.selectable[0]);
        assert!(text.paragraph.line_number() > 1);
        let byte = document.text.find('中').unwrap();
        let utf16 = text.text.byte_to_utf16(byte);
        let boxes = text.paragraph.get_rects_for_range(
            utf16..utf16 + 1,
            RectHeightStyle::Max,
            RectWidthStyle::Tight,
        );
        let rect = boxes
            .first()
            .expect("Chinese glyph has a real selection rectangle")
            .rect;
        assert!(rect.width() > 0.0 && rect.height() > 0.0);
        let y = text.y + (rect.top + rect.bottom) * 0.5;
        let left = document
            .hit_test(text.x + rect.left + rect.width() * 0.1, y, false)
            .unwrap();
        let right = document
            .hit_test(text.x + rect.right - rect.width() * 0.1, y, false)
            .unwrap();
        assert_eq!(left, byte);
        assert_eq!(right, byte + '中'.len_utf8());
        assert_eq!(document.selected_text(left..right), "中");
        for grapheme in ["e\u{301}", "👩‍💻"] {
            let start = document.text.find(grapheme).unwrap();
            let end = start + grapheme.len();
            let boxes = text.paragraph.get_rects_for_range(
                text.text.byte_to_utf16(start)..text.text.byte_to_utf16(end),
                RectHeightStyle::Max,
                RectWidthStyle::Tight,
            );
            for area in boxes {
                let rect = area.rect;
                for fraction in [0.1, 0.4, 0.6, 0.9] {
                    let hit = document
                        .hit_test(
                            text.x + rect.left + rect.width() * fraction,
                            text.y + (rect.top + rect.bottom) * 0.5,
                            false,
                        )
                        .unwrap();
                    assert!(hit == start || hit == end, "hit {hit} splits {grapheme}");
                }
            }
        }
        assert_eq!(document.hit_test(0.0, -100.0, false), None);
        assert_eq!(document.hit_test(0.0, -100.0, true), Some(0));
        assert_eq!(
            document.hit_test(0.0, document.height + 100.0, true),
            Some(document.text_len())
        );
        let word = document.word_range(left);
        assert!(
            document.text.is_char_boundary(word.start) && document.text.is_char_boundary(word.end)
        );
    }

    #[test]
    fn selection_geometry_reuses_local_rects_and_replaces_only_changed_range() {
        let source = "Long selected paragraph with 中文 and emoji 👩‍💻. ".repeat(100);
        let document = laid_document(&source, 180.0);
        let text = document.selectable_at(document.selectable[0]);
        let end = text.text.boundaries.last().unwrap().utf16;
        {
            let first = text.text.selection_geometry(text.paragraph, 0..end);
            let second = text.text.selection_geometry(text.paragraph, 0..end);
            assert!(std::ptr::eq(&*first, &*second));
            assert!(first.rects.len() > 20);
            assert!(first.visible(500.0, 540.0).count() < first.rects.len() / 2);
        }
        let replacement = text.text.selection_geometry(text.paragraph, 0..4);
        assert_eq!(replacement.range, 0..4);
        assert!(replacement.rects.len() <= 2);
    }

    #[test]
    fn selection_visibility_index_handles_overlapping_rect_heights() {
        let geometry = SelectionGeometry::new(
            0..4,
            vec![
                Rect::new(0.0, 110.0, 20.0, 120.0),
                Rect::new(0.0, 30.0, 20.0, 40.0),
                Rect::new(0.0, 0.0, 20.0, 100.0),
                Rect::new(0.0, 10.0, 20.0, 20.0),
            ],
        );
        let middle: Vec<_> = geometry.visible(50.0, 60.0).map(|rect| rect.top).collect();
        assert_eq!(middle, vec![0.0]);
        let end: Vec<_> = geometry
            .visible(115.0, 119.0)
            .map(|rect| rect.top)
            .collect();
        assert_eq!(end, vec![110.0]);
        assert_eq!(geometry.visible(130.0, 140.0).count(), 0);
    }

    #[test]
    fn word_hit_on_trailing_glyph_half_keeps_affinity_without_changing_caret() {
        let document = laid_document("hello world", 320.0);
        let text = document.selectable_at(document.selectable[0]);
        let boxes =
            text.paragraph
                .get_rects_for_range(4..5, RectHeightStyle::Max, RectWidthStyle::Tight);
        let rect = boxes.first().unwrap().rect;
        let x = text.x + rect.left + rect.width() * 0.8;
        let y = text.y + (rect.top + rect.bottom) * 0.5;
        assert_eq!(document.hit_test(x, y, false), Some(5));
        let word = document.hit_test_word(x, y, false).unwrap();
        assert_eq!(word, 0..5);
        assert_eq!(document.selected_text(word), "hello");

        let boxes =
            text.paragraph
                .get_rects_for_range(6..7, RectHeightStyle::Max, RectWidthStyle::Tight);
        let rect = boxes.first().unwrap().rect;
        let word = document
            .hit_test_word(text.x + rect.left + rect.width() * 0.2, y, false)
            .unwrap();
        assert_eq!(document.selected_text(word), "world");
    }
}
