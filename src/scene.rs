use std::collections::VecDeque;

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
                "中键 / 空格＋拖动    平移     ·     滚轮    缩放     ·     F    全览     ·     1    原比例     ·     R    起点     ·     L    远景细节",
                12.0,
                MUTED,
                1100.0,
                &fonts,
            ),
            compact_controls: markdown::paragraph(
                "中键拖动 · 滚轮缩放 · F 全览 · 1 原比例 · R 起点 · L 细节",
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
            message: String::new(),
        }
    }

    pub fn ready(&self) -> bool {
        self.pending.is_empty()
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
        for (column_index, column) in self.columns.iter().enumerate() {
            let column_x = column_index as f64 * (CARD_WIDTH + GAP);
            if column_x > visible.right() || column_x + CARD_WIDTH < visible.x {
                continue;
            }
            let start = column.partition_point(|card| card.rect.bottom() < visible.y);
            for card in &column[start..] {
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
