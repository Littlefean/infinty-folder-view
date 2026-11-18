use std::{collections::VecDeque, ops::Range};

use skia_safe::{
    Canvas, Color, Paint, Rect as SkRect,
    textlayout::{FontCollection, Paragraph},
};

use crate::{
    camera::{Camera, Rect},
    documents::SourceDocument,
    markdown::{self, Highlighter, MarkdownDocument},
};

const CARD_WIDTH: f64 = 720.0;
const GAP: f64 = 64.0;
const PADDING: f32 = 30.0;
const CONTENT_TOP: f32 = 88.0;
pub const TOP_BAR: f64 = 88.0;
const INK: Color = Color::from_rgb(221, 231, 243);
const MUTED: Color = Color::from_rgb(128, 150, 173);
const BACKGROUND: Color = Color::from_rgb(10, 16, 24);

struct Card {
    rect: Rect,
    body: MarkdownDocument,
    filename: Paragraph,
    meta: Paragraph,
    summary: Paragraph,
    accent: Color,
}

struct DocumentHit {
    card: (usize, usize),
    offset: usize,
    local: (f32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionUnit {
    Character,
    Word,
    Paragraph,
}

#[derive(Clone, Debug)]
struct Selection {
    card: (usize, usize),
    anchor: usize,
    head: usize,
    initial: Range<usize>,
    unit: SelectionUnit,
    dragging: bool,
    character_count: usize,
}

impl Selection {
    fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

pub struct Scene {
    pending: VecDeque<SourceDocument>,
    columns: Vec<Vec<Card>>,
    heights: Vec<f64>,
    fonts: FontCollection,
    highlighter: Highlighter,
    heading: Paragraph,
    subtitle: Paragraph,
    controls: Paragraph,
    compact_controls: Paragraph,
    status: Option<(String, Paragraph)>,
    selection: Option<Selection>,
    pub camera: Camera,
    pub bounds: Rect,
    pub total: usize,
    pub loaded: usize,
    pub lines: usize,
    pub layout_blocks: usize,
    pub visible_cards: usize,
    pub visible_blocks: usize,
    pub lod: bool,
    pub frame_ms: f64,
    pub message: String,
}

impl Scene {
    pub fn new(documents: Vec<SourceDocument>) -> Self {
        let fonts = markdown::font_collection();
        let total = documents.len();
        let lines: usize = documents.iter().map(|doc| doc.line_count).sum();
        // Long documents need more columns than a square grid of document counts would suggest.
        let estimated_height = lines as f64 / total.max(1) as f64 * 28.0;
        let ncols = ((total as f64 * estimated_height / (CARD_WIDTH + GAP) * 1.25)
            .sqrt()
            .ceil() as usize)
            .clamp(1, 48)
            .min(total.max(1));
        Self {
            heading: markdown::paragraph("MARKDOWN ATLAS", 19.0, INK, 320.0, &fonts),
            subtitle: markdown::paragraph("把文档展开，在空间里阅读。", 12.0, MUTED, 340.0, &fonts),
            controls: markdown::paragraph(
                "左键选字 · ⌘C 复制    |    中键 / 空格＋拖动 平移 · 滚轮缩放 · F 全览 · 1 原比例 · R 起点 · L 远景细节",
                12.0,
                MUTED,
                1100.0,
                &fonts,
            ),
            compact_controls: markdown::paragraph(
                "左键选字 · ⌘C 复制 · 中键平移 · 滚轮缩放 · F 全览 · R 起点",
                12.0,
                MUTED,
                700.0,
                &fonts,
            ),
            pending: documents.into(),
            columns: (0..ncols).map(|_| Vec::new()).collect(),
            heights: vec![0.0; ncols],
            fonts,
            highlighter: Highlighter::new(),
            camera: Camera::default(),
            bounds: Rect::new(0.0, 0.0, ncols as f64 * (CARD_WIDTH + GAP) - GAP, 1.0),
            total,
            loaded: 0,
            lines,
            layout_blocks: 0,
            visible_cards: 0,
            visible_blocks: 0,
            lod: true,
            frame_ms: 0.0,
            status: None,
            selection: None,
            message: String::new(),
        }
    }

    pub fn ready(&self) -> bool {
        self.pending.is_empty()
    }

    fn card(&self, id: (usize, usize)) -> Option<&Card> {
        self.columns.get(id.0)?.get(id.1)
    }

    fn document_hit(&self, x: f64, y: f64, width: f64, height: f64) -> Option<DocumentHit> {
        if ![x, y, width, height].iter().all(|value| value.is_finite())
            || width <= 0.0
            || height <= TOP_BAR + 38.0
            || x < 0.0
            || x > width
            || y < TOP_BAR
            || y >= height - 38.0
            || (self.lod && self.camera.scale < 0.12)
        {
            return None;
        }
        let minimap = self.minimap_rect(width, height);
        if self.ready()
            && x >= minimap.x
            && x <= minimap.right()
            && y >= minimap.y
            && y <= minimap.bottom()
        {
            return None;
        }
        let (world_x, world_y) = self.camera.screen_to_world(x, y);
        if !world_x.is_finite() || !world_y.is_finite() || world_x < 0.0 {
            return None;
        }
        let column_index = (world_x / (CARD_WIDTH + GAP)).floor() as usize;
        let column = self.columns.get(column_index)?;
        let row = column.partition_point(|card| card.rect.bottom() < world_y);
        let card = column.get(row)?;
        let local_x = world_x - card.rect.x - PADDING as f64;
        let local_y = world_y - card.rect.y - CONTENT_TOP as f64;
        let content_width = CARD_WIDTH - PADDING as f64 * 2.0;
        let content_height = card.body.height as f64;
        // Camera inversion can put an exact paragraph edge a few ulps outside
        // the content rectangle. Admit only that subpixel error, then snap it
        // back before handing coordinates to SkParagraph's strict hit test.
        const HIT_EPSILON: f64 = 0.00001;
        if local_x < -HIT_EPSILON
            || local_x > content_width + HIT_EPSILON
            || local_y < -HIT_EPSILON
            || local_y > content_height + HIT_EPSILON
        {
            return None;
        }
        let local = (
            local_x.clamp(0.0, content_width) as f32,
            local_y.clamp(0.0, content_height) as f32,
        );
        card.body
            .hit_test(local.0, local.1, false)
            .map(|offset| DocumentHit {
                card: (column_index, row),
                offset,
                local,
            })
    }

    pub fn text_at(&self, x: f64, y: f64, width: f64, height: f64) -> bool {
        self.document_hit(x, y, width, height).is_some()
    }

    pub fn begin_selection(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        click_count: u8,
        extend: bool,
    ) -> bool {
        self.message.clear();
        let Some(hit) = self.document_hit(x, y, width, height) else {
            self.clear_selection();
            return false;
        };
        let id = hit.card;
        let offset = hit.offset;
        if extend
            && let Some(selection) = &mut self.selection
            && selection.card == id
        {
            selection.head = offset;
            selection.initial = selection.anchor..selection.anchor;
            selection.unit = SelectionUnit::Character;
            selection.dragging = true;
            self.update_selection_count();
            return true;
        }
        let body = &self.card(id).expect("Hit-tested card exists").body;
        let (unit, range) = match click_count {
            0 | 1 => (SelectionUnit::Character, offset..offset),
            2 => {
                let Some(range) = body.hit_test_word(hit.local.0, hit.local.1, false) else {
                    self.clear_selection();
                    return false;
                };
                (SelectionUnit::Word, range)
            }
            _ => (SelectionUnit::Paragraph, body.paragraph_range(offset)),
        };
        self.selection = Some(Selection {
            card: id,
            anchor: range.start,
            head: range.end,
            initial: range,
            unit,
            dragging: true,
            character_count: 0,
        });
        self.update_selection_count();
        true
    }

    pub fn update_selection(&mut self, x: f64, y: f64, width: f64, height: f64) -> bool {
        if ![x, y, width, height].iter().all(|value| value.is_finite())
            || width <= 0.0
            || height <= TOP_BAR + 38.0
        {
            return false;
        }
        let Some(selection) = &self.selection else {
            return false;
        };
        if !selection.dragging {
            return false;
        }
        let Some(card) = self.card(selection.card) else {
            return false;
        };
        // Edge auto-pan moves the camera separately. Keep the drag endpoint at the
        // readable viewport edge so the footer and title bar do not select hidden text.
        let (world_x, world_y) = self
            .camera
            .screen_to_world(x.clamp(0.0, width), y.clamp(TOP_BAR, height - 38.0));
        let local_x = world_x - card.rect.x - PADDING as f64;
        let local_y = world_y - card.rect.y - CONTENT_TOP as f64;
        if !local_x.is_finite() || !local_y.is_finite() {
            return false;
        }
        let (anchor, head) = match selection.unit {
            SelectionUnit::Character => {
                let Some(offset) = card.body.hit_test(local_x as f32, local_y as f32, true) else {
                    return false;
                };
                (selection.anchor, offset)
            }
            unit => {
                let range = match unit {
                    SelectionUnit::Word => {
                        card.body
                            .hit_test_word(local_x as f32, local_y as f32, true)
                    }
                    SelectionUnit::Paragraph => card
                        .body
                        .hit_test(local_x as f32, local_y as f32, true)
                        .map(|offset| card.body.paragraph_range(offset)),
                    SelectionUnit::Character => unreachable!(),
                };
                let Some(range) = range else {
                    return false;
                };
                if range.start < selection.initial.start {
                    (selection.initial.end, range.start)
                } else if range.end > selection.initial.end {
                    (selection.initial.start, range.end)
                } else {
                    (selection.initial.start, selection.initial.end)
                }
            }
        };
        if selection.anchor == anchor && selection.head == head {
            return false;
        }
        let selection = self.selection.as_mut().expect("Selection remains active");
        selection.anchor = anchor;
        selection.head = head;
        self.update_selection_count();
        true
    }

    pub fn pan_selection(&mut self, dx: f64, dy: f64, width: f64, height: f64) -> bool {
        const EDGE_INSET: f64 = 24.0;
        if !self.is_selecting()
            || ![dx, dy, width, height]
                .iter()
                .all(|value| value.is_finite())
            || width <= EDGE_INSET * 2.0
            || height <= TOP_BAR + 38.0 + EDGE_INSET * 2.0
        {
            return false;
        }
        let Some(selection) = &self.selection else {
            return false;
        };
        let Some(card) = self.card(selection.card) else {
            return false;
        };
        let top_left = self.camera.world_to_screen(
            card.rect.x + PADDING as f64,
            card.rect.y + CONTENT_TOP as f64,
        );
        let bottom_right = self.camera.world_to_screen(
            card.rect.right() - PADDING as f64,
            card.rect.y + CONTENT_TOP as f64 + card.body.height as f64,
        );
        if ![top_left.0, top_left.1, bottom_right.0, bottom_right.1]
            .iter()
            .all(|value| value.is_finite())
        {
            return false;
        }
        // A short document may already fit between both limits, and the camera
        // may begin outside them. Clamp only the requested direction, never snap
        // an existing position back into bounds as a side effect of dragging.
        let limit_axis = |delta: f64, start: f64, end: f64, min: f64, max: f64| {
            if delta < 0.0 {
                delta.max((max - end).min(0.0))
            } else {
                delta.min((min - start).max(0.0))
            }
        };
        let dx = limit_axis(
            dx,
            top_left.0,
            bottom_right.0,
            EDGE_INSET,
            width - EDGE_INSET,
        );
        let dy = limit_axis(
            dy,
            top_left.1,
            bottom_right.1,
            TOP_BAR + EDGE_INSET,
            height - 38.0 - EDGE_INSET,
        );
        let before = self.camera;
        self.camera.pan(dx, dy);
        self.camera != before
    }

    fn update_selection_count(&mut self) {
        let count = self
            .selection
            .as_ref()
            .and_then(|selection| {
                self.card(selection.card)
                    .map(|card| card.body.selection_char_count(selection.range()))
            })
            .unwrap_or(0);
        if let Some(selection) = &mut self.selection {
            selection.character_count = count;
        }
    }

    pub fn finish_selection(&mut self) {
        if let Some(selection) = &mut self.selection {
            selection.dragging = false;
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub fn selected_text(&self) -> Option<String> {
        let selection = self.selection.as_ref()?;
        let range = selection.range();
        if range.is_empty() {
            return None;
        }
        let text = self.card(selection.card)?.body.selected_text(range);
        (!text.is_empty()).then_some(text)
    }

    pub fn select_all(&mut self) -> bool {
        let Some(selection) = &self.selection else {
            return false;
        };
        let Some(card) = self.card(selection.card) else {
            return false;
        };
        let len = card.body.text_len();
        if len == 0 {
            return false;
        }
        self.message.clear();
        let selection = self.selection.as_mut().expect("Selection remains active");
        selection.anchor = 0;
        selection.head = len;
        selection.initial = 0..len;
        selection.unit = SelectionUnit::Character;
        selection.dragging = false;
        self.update_selection_count();
        true
    }

    pub fn is_selecting(&self) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|selection| selection.dragging)
    }

    pub fn build_next(&mut self) {
        let Some(source) = self.pending.pop_front() else {
            return;
        };
        let body = MarkdownDocument::new(
            &source.markdown,
            CARD_WIDTH as f32 - PADDING * 2.0,
            &self.fonts,
            &self.highlighter,
        );
        let column = self
            .heights
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .unwrap_or(0);
        let rect = Rect::new(
            column as f64 * (CARD_WIDTH + GAP),
            self.heights[column],
            CARD_WIDTH,
            CONTENT_TOP as f64 + body.height as f64 + 34.0,
        );
        let filename = source
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let title = format!("{:02}   {}", self.loaded + 1, filename);
        let palette = [
            Color::from_rgb(85, 196, 189),
            Color::from_rgb(121, 163, 240),
            Color::from_rgb(203, 164, 112),
            Color::from_rgb(172, 147, 220),
        ];
        let accent = palette[column % palette.len()];
        let card = Card {
            filename: markdown::paragraph(
                &title,
                14.0,
                INK,
                CARD_WIDTH as f32 - 160.0,
                &self.fonts,
            ),
            meta: markdown::paragraph(
                &format!("{} 行", source.line_count),
                12.0,
                MUTED,
                100.0,
                &self.fonts,
            ),
            summary: markdown::paragraph(
                &source.title,
                42.0,
                accent,
                CARD_WIDTH as f32 - 80.0,
                &self.fonts,
            ),
            rect,
            body,
            accent,
        };
        self.layout_blocks += card.body.block_count;
        self.heights[column] = rect.bottom() + GAP;
        self.bounds.height = self.bounds.height.max(rect.bottom());
        self.columns[column].push(card);
        self.loaded += 1;
    }

    pub fn paint(&mut self, canvas: &Canvas, width: f64, height: f64) {
        canvas.clear(BACKGROUND);
        self.draw_grid(canvas, width, height);
        let visible = self
            .camera
            .visible_rect(width, height)
            .expanded(10.0 / self.camera.scale);
        self.visible_cards = 0;
        self.visible_blocks = 0;
        canvas.save();
        canvas.clip_rect(
            SkRect::from_xywh(
                0.0,
                TOP_BAR as f32,
                width as f32,
                (height - TOP_BAR - 38.0).max(0.0) as f32,
            ),
            None,
            false,
        );
        let origin = self.camera.world_to_screen(0.0, 0.0);
        canvas.translate((origin.0 as f32, origin.1 as f32));
        canvas.scale((self.camera.scale as f32, self.camera.scale as f32));
        let selected = self.selection.as_ref().and_then(|selection| {
            let range = selection.range();
            (!range.is_empty()).then_some((selection.card, range))
        });
        for (column_index, column) in self.columns.iter().enumerate() {
            let column_x = column_index as f64 * (CARD_WIDTH + GAP);
            if column_x > visible.right() || column_x + CARD_WIDTH < visible.x {
                continue;
            }
            let start = column.partition_point(|card| card.rect.bottom() < visible.y);
            for (row, card) in column.iter().enumerate().skip(start) {
                if card.rect.y > visible.bottom() {
                    break;
                }
                if !card.rect.intersects(&visible) {
                    continue;
                }
                self.visible_cards += 1;
                canvas.save();
                canvas.translate((card.rect.x as f32, card.rect.y as f32));
                let rect = SkRect::from_xywh(0.0, 0.0, CARD_WIDTH as f32, card.rect.height as f32);
                let mut fill = Paint::default();
                fill.set_color(Color::from_rgb(19, 28, 40))
                    .set_anti_alias(true);
                canvas.draw_round_rect(rect, 10.0, 10.0, &fill);
                let mut border = Paint::default();
                border
                    .set_color(Color::from_rgb(41, 57, 75))
                    .set_style(skia_safe::paint::Style::Stroke)
                    .set_stroke_width((1.0 / self.camera.scale).min(3.0) as f32)
                    .set_anti_alias(true);
                canvas.draw_round_rect(rect, 10.0, 10.0, &border);
                fill.set_color(card.accent);
                canvas.draw_round_rect(SkRect::from_xywh(30.0, 27.0, 5.0, 18.0), 2.0, 2.0, &fill);
                if !self.lod || self.camera.scale >= 0.12 {
                    canvas.save();
                    canvas.clip_rect(
                        SkRect::from_xywh(46.0, 22.0, CARD_WIDTH as f32 - 160.0, 30.0),
                        None,
                        true,
                    );
                    card.filename.paint(canvas, (46.0, 27.0));
                    canvas.restore();
                    card.meta.paint(canvas, (CARD_WIDTH as f32 - 100.0, 29.0));
                    fill.set_color(Color::from_rgb(37, 51, 68));
                    canvas.draw_rect(
                        SkRect::from_xywh(PADDING, 63.0, CARD_WIDTH as f32 - PADDING * 2.0, 1.0),
                        &fill,
                    );
                    canvas.translate((PADDING, CONTENT_TOP));
                    self.visible_blocks += card.body.paint(
                        canvas,
                        (visible.y - card.rect.y) as f32 - CONTENT_TOP,
                        (visible.bottom() - card.rect.y) as f32 - CONTENT_TOP,
                        selected.as_ref().and_then(|(id, range)| {
                            (*id == (column_index, row)).then(|| range.clone())
                        }),
                    );
                } else {
                    card.summary.paint(canvas, (40.0, 100.0));
                    fill.set_color(Color::from_rgb(37, 52, 69));
                    let first_row =
                        (((visible.y - card.rect.y - 254.0) / 76.0).floor().max(0.0)) as u32;
                    let mut y = 240.0 + first_row as f32 * 76.0;
                    let bottom =
                        (card.rect.height - 80.0).min(visible.bottom() - card.rect.y) as f32;
                    while y < bottom {
                        let fraction = if (y as u32 / 76).is_multiple_of(3) {
                            0.68
                        } else {
                            0.9
                        };
                        canvas.draw_round_rect(
                            SkRect::from_xywh(40.0, y, (CARD_WIDTH as f32 - 80.0) * fraction, 14.0),
                            5.0,
                            5.0,
                            &fill,
                        );
                        y += 76.0;
                    }
                }
                canvas.restore();
            }
        }
        canvas.restore();
        self.draw_chrome(canvas, width, height);
    }

    fn draw_grid(&self, canvas: &Canvas, width: f64, height: f64) {
        let mut spacing = 64.0 * self.camera.scale;
        while spacing < 28.0 {
            spacing *= 4.0;
        }
        while spacing > 112.0 {
            spacing /= 4.0;
        }
        let mut paint = Paint::default();
        paint
            .set_color(Color::from_rgb(35, 47, 62))
            .set_anti_alias(true);
        let mut x = self.camera.x.rem_euclid(spacing);
        while x < width {
            let mut y = self.camera.y.rem_euclid(spacing);
            while y < height {
                canvas.draw_circle((x as f32, y as f32), 0.85, &paint);
                y += spacing;
            }
            x += spacing;
        }
    }

    fn draw_chrome(&mut self, canvas: &Canvas, width: f64, height: f64) {
        let mut paint = Paint::default();
        paint.set_color(Color::from_argb(250, 13, 20, 30));
        canvas.draw_rect(
            SkRect::from_xywh(0.0, 0.0, width as f32, TOP_BAR as f32),
            &paint,
        );
        canvas.draw_rect(
            SkRect::from_xywh(0.0, height as f32 - 38.0, width as f32, 38.0),
            &paint,
        );
        paint.set_color(Color::from_rgb(35, 50, 65));
        canvas.draw_line(
            (0.0, TOP_BAR as f32 - 1.0),
            (width as f32, TOP_BAR as f32 - 1.0),
            &paint,
        );
        self.heading.paint(canvas, (28.0, 19.0));
        self.subtitle.paint(canvas, (28.0, 48.0));
        let status = if !self.message.is_empty() {
            self.message.clone()
        } else if let Some(selection) = &self.selection
            && selection.character_count > 0
        {
            format!(
                "已选择 {} 个字符 · ⌘C 复制\nShift＋点击延伸 · 双击选词 · 三击选段落",
                selection.character_count
            )
        } else if !self.ready() {
            format!("正在排版  {}/{}  份 Markdown…", self.loaded, self.total)
        } else if width < 1100.0 {
            format!(
                "{} 份文档 · {} 行 · {:.0}%\n可见 {} 份 · {:.1} ms · {}",
                self.total,
                self.lines,
                self.camera.scale * 100.0,
                self.visible_cards,
                self.frame_ms,
                if self.lod {
                    "智能细节"
                } else {
                    "完整文字"
                }
            )
        } else {
            format!(
                "{} 份文档  ·  {} 行\n可见 {} 份 / {} 块   ·   {:.0}%   ·   CPU 提交 {:.1} ms   ·   {}",
                self.total,
                self.lines,
                self.visible_cards,
                self.visible_blocks,
                self.camera.scale * 100.0,
                self.frame_ms,
                if self.lod {
                    "智能细节"
                } else {
                    "完整文字"
                }
            )
        };
        let status_x = (width as f32 - 635.0).max(340.0);
        let status_width = width as f32 - status_x - 24.0;
        let status_key = format!("{:.0}:{status}", width);
        if self
            .status
            .as_ref()
            .map(|(text, _)| text != &status_key)
            .unwrap_or(true)
        {
            self.status = Some((
                status_key,
                markdown::paragraph(&status, 12.0, MUTED, status_width, &self.fonts),
            ));
        }
        if let Some((_, paragraph)) = &self.status {
            canvas.save();
            canvas.clip_rect(
                SkRect::from_xywh(status_x, 16.0, status_width, 64.0),
                None,
                true,
            );
            paragraph.paint(canvas, (status_x, 22.0));
            canvas.restore();
        }
        if width < 1100.0 {
            self.compact_controls
                .paint(canvas, (24.0, height as f32 - 26.0));
        } else {
            self.controls.paint(canvas, (24.0, height as f32 - 26.0));
        }
        self.draw_minimap(canvas, width, height);
    }

    pub fn minimap_rect(&self, width: f64, height: f64) -> Rect {
        Rect::new(width - 184.0, height - 206.0, 160.0, 144.0)
    }

    pub fn navigate_minimap(&mut self, x: f64, y: f64, width: f64, height: f64) -> bool {
        if !self.ready() {
            return false;
        }
        let rect = self.minimap_rect(width, height);
        if x < rect.x || x > rect.right() || y < rect.y || y > rect.bottom() {
            return false;
        }
        let (s, ox, oy) = self.minimap_transform(rect);
        let world_x = (x - ox) / s;
        let world_y = (y - oy) / s;
        self.camera.x = width / 2.0 - world_x * self.camera.scale;
        self.camera.y = (height - 38.0 + TOP_BAR) / 2.0 - world_y * self.camera.scale;
        true
    }

    fn minimap_transform(&self, rect: Rect) -> (f64, f64, f64) {
        let s = ((rect.width - 20.0) / self.bounds.width.max(1.0))
            .min((rect.height - 20.0) / self.bounds.height.max(1.0));
        (
            s,
            rect.x + (rect.width - self.bounds.width * s) / 2.0,
            rect.y + (rect.height - self.bounds.height * s) / 2.0,
        )
    }

    fn draw_minimap(&self, canvas: &Canvas, width: f64, height: f64) {
        if !self.ready() {
            return;
        }
        let rect = self.minimap_rect(width, height);
        let frame = SkRect::from_xywh(
            rect.x as f32,
            rect.y as f32,
            rect.width as f32,
            rect.height as f32,
        );
        let mut paint = Paint::default();
        paint
            .set_anti_alias(true)
            .set_color(Color::from_argb(245, 12, 20, 29));
        canvas.draw_round_rect(frame, 8.0, 8.0, &paint);
        canvas.save();
        canvas.clip_rect(frame, None, true);
        let (s, ox, oy) = self.minimap_transform(rect);
        for column in &self.columns {
            for card in column {
                paint.set_color(Color::from_rgb(49, 69, 89));
                canvas.draw_rect(
                    SkRect::from_xywh(
                        (ox + card.rect.x * s) as f32,
                        (oy + card.rect.y * s) as f32,
                        (card.rect.width * s).max(1.0) as f32,
                        (card.rect.height * s).max(1.0) as f32,
                    ),
                    &paint,
                );
            }
        }
        let visible = self.camera.visible_rect(width, height);
        paint
            .set_color(Color::from_rgb(100, 209, 198))
            .set_style(skia_safe::paint::Style::Stroke)
            .set_stroke_width(1.5);
        canvas.draw_rect(
            SkRect::from_xywh(
                (ox + visible.x * s) as f32,
                (oy + visible.y * s) as f32,
                (visible.width * s).max(2.0) as f32,
                (visible.height * s).max(2.0) as f32,
            ),
            &paint,
        );
        canvas.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const WIDTH: f64 = 1440.0;
    const HEIGHT: f64 = 940.0;

    fn source(name: &str, text: &str) -> SourceDocument {
        SourceDocument {
            path: PathBuf::from(name),
            title: name.into(),
            markdown: text.into(),
            line_count: text.lines().count(),
        }
    }

    fn scene() -> Scene {
        let mut scene = Scene::new(vec![source(
            "selection.md",
            "hello **world** 中文\n\nA second paragraph.\n",
        )]);
        while !scene.ready() {
            scene.build_next();
        }
        scene
    }

    fn text_point(scene: &Scene, wanted_offset: usize) -> (f64, f64) {
        let card = &scene.columns[0][0];
        for y in (1..card.body.height.ceil() as usize).step_by(2) {
            for x in (1..CARD_WIDTH as usize - PADDING as usize * 2).step_by(2) {
                if card.body.hit_test(x as f32, y as f32, false) == Some(wanted_offset) {
                    return scene.camera.world_to_screen(
                        card.rect.x + PADDING as f64 + x as f64,
                        card.rect.y + CONTENT_TOP as f64 + y as f64,
                    );
                }
            }
        }
        panic!("No glyph hit point for offset {wanted_offset}");
    }

    #[test]
    fn word_and_paragraph_selection_use_rendered_text() {
        let mut scene = scene();
        let point = text_point(&scene, 1);
        assert!(scene.begin_selection(point.0, point.1, WIDTH, HEIGHT, 2, false));
        assert_eq!(scene.selected_text().as_deref(), Some("hello"));
        assert!(scene.is_selecting());
        scene.finish_selection();
        assert!(!scene.is_selecting());
        assert!(scene.begin_selection(point.0, point.1, WIDTH, HEIGHT, 3, false));
        let text = scene.selected_text().unwrap();
        assert!(text.contains("hello world 中文"));
        assert!(!text.contains("**"));
        assert!(!text.contains("second paragraph"));
    }

    #[test]
    fn word_selection_uses_glyph_affinity_while_character_selection_keeps_caret() {
        let mut scene = scene();
        // The first hit at this caret is the trailing half of the final "o".
        // A plain word_range(5) would instead choose the following space.
        let hello_end = text_point(&scene, 5);
        let world_end = text_point(&scene, 11);
        let body = &scene.columns[0][0].body;
        assert_eq!(body.selected_text(body.word_range(5)), " ");

        assert!(scene.begin_selection(hello_end.0, hello_end.1, WIDTH, HEIGHT, 2, false));
        assert_eq!(scene.selected_text().as_deref(), Some("hello"));
        assert!(!scene.update_selection(hello_end.0, hello_end.1, WIDTH, HEIGHT));
        assert!(scene.update_selection(world_end.0, world_end.1, WIDTH, HEIGHT));
        assert_eq!(scene.selected_text().as_deref(), Some("hello world"));
        assert!(scene.update_selection(hello_end.0, hello_end.1, WIDTH, HEIGHT));
        assert_eq!(scene.selected_text().as_deref(), Some("hello"));

        assert!(scene.begin_selection(world_end.0, world_end.1, WIDTH, HEIGHT, 2, false));
        assert_eq!(scene.selected_text().as_deref(), Some("world"));
        assert!(scene.update_selection(hello_end.0, hello_end.1, WIDTH, HEIGHT));
        assert_eq!(scene.selected_text().as_deref(), Some("hello world"));

        assert!(scene.begin_selection(hello_end.0, hello_end.1, WIDTH, HEIGHT, 1, false));
        assert_eq!(scene.selection.as_ref().unwrap().anchor, 5);
        assert!(scene.selected_text().is_none());
        assert!(scene.update_selection(world_end.0, world_end.1, WIDTH, HEIGHT));
        assert_eq!(scene.selected_text().as_deref(), Some(" world"));
    }

    #[test]
    fn selection_and_hit_testing_survive_camera_pan_and_zoom() {
        let mut scene = scene();
        let point = text_point(&scene, 1);
        let world = scene.camera.screen_to_world(point.0, point.1);
        assert!(scene.begin_selection(point.0, point.1, WIDTH, HEIGHT, 3, false));
        scene.finish_selection();
        let selected = scene.selected_text();
        scene.camera.zoom_at(0.0, 0.0, 1.5);
        scene.camera.pan(60.0, 20.0);
        let transformed = scene.camera.world_to_screen(world.0, world.1);
        assert!(scene.text_at(transformed.0, transformed.1, WIDTH, HEIGHT));
        assert_eq!(scene.selected_text(), selected);
    }

    #[test]
    fn paragraph_edges_tolerate_camera_roundoff_without_selecting_header_or_padding() {
        let scene = scene();
        let card = &scene.columns[0][0];
        for local in [(6.0, 0.0), (0.0, 2.0)] {
            let expected = card.body.hit_test(local.0 as f32, local.1 as f32, false);
            assert!(expected.is_some());
            let screen = scene.camera.world_to_screen(
                card.rect.x + PADDING as f64 + local.0,
                card.rect.y + CONTENT_TOP as f64 + local.1,
            );
            assert_eq!(
                scene
                    .document_hit(screen.0, screen.1, WIDTH, HEIGHT)
                    .map(|hit| (hit.card, hit.offset)),
                expected.map(|offset| ((0, 0), offset))
            );
        }
        for local in [(6.0, -0.01), (-0.01, 2.0)] {
            let screen = scene.camera.world_to_screen(
                card.rect.x + PADDING as f64 + local.0,
                card.rect.y + CONTENT_TOP as f64 + local.1,
            );
            assert!(!scene.text_at(screen.0, screen.1, WIDTH, HEIGHT));
        }
    }

    #[test]
    fn drag_stays_in_its_original_card_and_blank_click_clears_active_card() {
        let mut scene = Scene::new(vec![
            source("first.md", "First card only.\n"),
            source("second.md", "Do not select this other card.\n"),
        ]);
        while !scene.ready() {
            scene.build_next();
        }
        let point = text_point(&scene, 0);
        assert!(scene.begin_selection(point.0, point.1, WIDTH, HEIGHT, 1, false));
        assert!(scene.update_selection(WIDTH + 100.0, HEIGHT + 100.0, WIDTH, HEIGHT));
        assert_eq!(scene.selection.as_ref().unwrap().card, (0, 0));
        assert_eq!(
            scene.selected_text(),
            Some(
                scene.columns[0][0]
                    .body
                    .selected_text(0..scene.columns[0][0].body.text_len())
            )
        );
        assert!(!scene.selected_text().unwrap().contains("other card"));
        scene.finish_selection();
        assert!(!scene.begin_selection(WIDTH - 10.0, TOP_BAR + 10.0, WIDTH, HEIGHT, 1, false));
        assert!(scene.selected_text().is_none());
        assert!(!scene.select_all());
    }

    #[test]
    fn headers_overlays_and_hidden_lod_text_cannot_start_selection() {
        let mut scene = scene();
        let point = text_point(&scene, 1);
        assert!(scene.text_at(point.0, point.1, WIDTH, HEIGHT));
        let header = scene.camera.world_to_screen(60.0, 30.0);
        assert!(!scene.text_at(header.0, header.1, WIDTH, HEIGHT));
        assert!(!scene.text_at(point.0, 30.0, WIDTH, HEIGHT));
        assert!(!scene.text_at(point.0, HEIGHT - 10.0, WIDTH, HEIGHT));
        let minimap = scene.minimap_rect(WIDTH, HEIGHT);
        assert!(!scene.text_at(minimap.x + 20.0, minimap.y + 20.0, WIDTH, HEIGHT));
        scene.camera.zoom_at(point.0, point.1, 0.1);
        assert!(!scene.text_at(point.0, point.1, WIDTH, HEIGHT));
        scene.lod = false;
        assert!(scene.text_at(point.0, point.1, WIDTH, HEIGHT));
    }

    #[test]
    fn shift_extension_retains_anchor_and_select_all_uses_active_card() {
        let mut scene = scene();
        let start = text_point(&scene, 1);
        let end = text_point(&scene, 9);
        assert!(scene.begin_selection(start.0, start.1, WIDTH, HEIGHT, 1, false));
        scene.finish_selection();
        assert!(scene.begin_selection(end.0, end.1, WIDTH, HEIGHT, 1, true));
        let selection = scene.selection.as_ref().unwrap();
        assert_eq!(selection.anchor, 1);
        assert_eq!(selection.head, 9);
        scene.message = "已复制旧选区".into();
        assert!(scene.select_all());
        assert!(scene.message.is_empty());
        assert!(!scene.is_selecting());
        let body = &scene.columns[0][0].body;
        assert_eq!(
            scene.selected_text(),
            Some(body.selected_text(0..body.text_len()))
        );
    }

    #[test]
    fn selection_auto_pan_stops_at_both_ends_of_a_large_document() {
        let text = "A long document paragraph with enough content for scrolling.\n\n".repeat(100);
        let mut scene = Scene::new(vec![source("long.md", &text)]);
        scene.build_next();
        let point = text_point(&scene, 1);
        assert!(scene.begin_selection(point.0, point.1, WIDTH, HEIGHT, 1, false));
        scene.camera = Camera {
            x: -400.0,
            y: -600.0,
            scale: 3.0,
        };
        assert!(scene.pan_selection(-1.0e6, -1.0e6, WIDTH, HEIGHT));
        let card = &scene.columns[0][0];
        let bottom_right = scene.camera.world_to_screen(
            card.rect.right() - PADDING as f64,
            card.rect.y + CONTENT_TOP as f64 + card.body.height as f64,
        );
        assert!((bottom_right.0 - (WIDTH - 24.0)).abs() < 1.0e-8);
        assert!((bottom_right.1 - (HEIGHT - 38.0 - 24.0)).abs() < 1.0e-8);
        assert!(!scene.pan_selection(-100.0, -100.0, WIDTH, HEIGHT));
        assert!(scene.pan_selection(1.0e6, 1.0e6, WIDTH, HEIGHT));
        let card = &scene.columns[0][0];
        let top_left = scene.camera.world_to_screen(
            card.rect.x + PADDING as f64,
            card.rect.y + CONTENT_TOP as f64,
        );
        assert!((top_left.0 - 24.0).abs() < 1.0e-8);
        assert!((top_left.1 - (TOP_BAR + 24.0)).abs() < 1.0e-8);
        assert!(!scene.pan_selection(100.0, 100.0, WIDTH, HEIGHT));
    }

    #[test]
    fn selection_auto_pan_does_not_push_short_content_away_or_snap_existing_offsets() {
        let mut scene = scene();
        let point = text_point(&scene, 1);
        assert!(scene.begin_selection(point.0, point.1, WIDTH, HEIGHT, 1, false));
        let before = scene.camera;
        assert!(!scene.pan_selection(-1000.0, -1000.0, WIDTH, HEIGHT));
        assert!(!scene.pan_selection(1000.0, 1000.0, WIDTH, HEIGHT));
        assert_eq!(scene.camera, before);

        scene.camera.x = -200.0;
        scene.camera.y = -100.0;
        let outside = scene.camera;
        assert!(!scene.pan_selection(-1000.0, -1000.0, WIDTH, HEIGHT));
        assert_eq!(scene.camera, outside);
        assert!(scene.pan_selection(10.0, 10.0, WIDTH, HEIGHT));
        assert_eq!(scene.camera.x, outside.x + 10.0);
        assert_eq!(scene.camera.y, outside.y + 10.0);
        assert!(scene.pan_selection(1000.0, 1000.0, WIDTH, HEIGHT));
        let card = &scene.columns[0][0];
        let top_left = scene.camera.world_to_screen(
            card.rect.x + PADDING as f64,
            card.rect.y + CONTENT_TOP as f64,
        );
        assert!((top_left.0 - 24.0).abs() < 1.0e-8);
        assert!((top_left.1 - (TOP_BAR + 24.0)).abs() < 1.0e-8);
        assert!(!scene.pan_selection(1000.0, 1000.0, WIDTH, HEIGHT));
        assert!(!scene.pan_selection(-1000.0, -1000.0, WIDTH, HEIGHT));
    }

    #[test]
    fn selection_auto_pan_requires_an_active_drag_and_valid_dimensions() {
        let mut scene = scene();
        let before = scene.camera;
        assert!(!scene.pan_selection(100.0, 100.0, WIDTH, HEIGHT));
        assert_eq!(scene.camera, before);
        let point = text_point(&scene, 1);
        assert!(scene.begin_selection(point.0, point.1, WIDTH, HEIGHT, 1, false));
        scene.camera.x = -200.0;
        let before = scene.camera;
        assert!(!scene.pan_selection(f64::NAN, 0.0, WIDTH, HEIGHT));
        assert!(!scene.pan_selection(100.0, 0.0, 10.0, HEIGHT));
        assert!(!scene.pan_selection(100.0, 0.0, WIDTH, 100.0));
        assert_eq!(scene.camera, before);
        scene.finish_selection();
        assert!(!scene.pan_selection(100.0, 0.0, WIDTH, HEIGHT));
        assert_eq!(scene.camera, before);
    }
}
