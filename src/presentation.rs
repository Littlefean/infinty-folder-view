//! Zoom changes presentation only: covers never change layout geometry.
use skia_safe::{
    Canvas, Color, FontStyle,
    textlayout::{
        FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, TextAlign, TextStyle,
    },
};

use crate::camera::Rect;

// Keep source hidden until the smallest body font (14px) appears at least 8px tall.
pub const FILE_DETAIL_SCALE: f64 = 8.0 / 14.0;

pub fn folder_reveal_scale(rect: Rect, depth: usize) -> f64 {
    let depth = depth.min(12) as f64;
    ((260.0 + depth * 24.0) / rect.width).max((140.0 + depth * 16.0) / rect.height)
}

pub fn border_width(rect: Rect, scale: f64, folder: bool, selected: bool) -> f32 {
    let pixels = if selected {
        2.0
    } else {
        ((if folder { 1.4 } else { 1.0 }) + (1.0 / scale).log2().max(0.0) * 0.4).min(3.2)
    };
    (pixels / scale).min(rect.width.min(rect.height) * 0.06) as f32
}

/// Cache in world units; only refit when a folder changes size, never on zoom.
pub struct SummaryTitle {
    name: String,
    color: Color,
    size: (f32, f32),
    paragraph: Option<Paragraph>,
}

impl SummaryTitle {
    pub fn new(name: impl Into<String>, color: Color) -> Self {
        Self {
            name: name.into(),
            color,
            size: (0.0, 0.0),
            paragraph: None,
        }
    }

    fn fit(&mut self, width: f32, height: f32, fonts: &FontCollection) {
        if self.size == (width, height) && self.paragraph.is_some() {
            return;
        }
        // Lay out one complete line along the frame's longer axis. Portrait
        // covers rotate this line at paint time, so their fitting axes swap.
        let line_width = width.max(height);
        let line_height = width.min(height);
        let make = |size| {
            let mut text = TextStyle::new();
            text.set_font_families(&["Helvetica Neue", "PingFang SC", "Apple Color Emoji"])
                .set_font_style(FontStyle::bold())
                .set_font_size(size)
                .set_color(self.color)
                .set_height(1.12)
                .set_height_override(true);
            let mut style = ParagraphStyle::new();
            style
                .set_text_style(&text)
                .set_text_align(TextAlign::Center);
            let mut builder = ParagraphBuilder::new(&style, fonts.clone());
            builder.add_text(&self.name);
            let mut paragraph = builder.build();
            paragraph.layout(line_width.max(0.01));
            paragraph
        };
        let mut low = 0.01;
        let mut high = width.max(height) * 2.0;
        for _ in 0..18 {
            let size = (low + high) / 2.0;
            let paragraph = make(size);
            if paragraph.height() <= line_height
                && paragraph.longest_line() <= line_width
                && paragraph.max_intrinsic_width() <= line_width
                && paragraph.line_number() == 1
            {
                low = size;
            } else {
                high = size;
            }
        }
        // Leave a small margin for SkParagraph's rounded line-breaking metrics.
        self.paragraph = Some(make(low * 0.995));
        self.size = (width, height);
    }

    pub fn paint(&mut self, canvas: &Canvas, rect: Rect, fonts: &FontCollection) {
        let inset = rect.width.min(rect.height) as f32 * 0.1;
        let width = rect.width as f32 - inset * 2.0;
        let height = rect.height as f32 - inset * 2.0;
        self.fit(width, height, fonts);
        let paragraph = self.paragraph.as_ref().unwrap();
        canvas.save();
        canvas.clip_rect(
            skia_safe::Rect::from_xywh(rect.x as f32 + inset, rect.y as f32 + inset, width, height),
            None,
            true,
        );
        canvas.translate((
            rect.x as f32 + rect.width as f32 / 2.0,
            rect.y as f32 + rect.height as f32 / 2.0,
        ));
        if height > width {
            canvas.rotate(90.0, None);
        }
        paragraph.paint(
            canvas,
            (-width.max(height) / 2.0, -paragraph.height() / 2.0),
        );
        canvas.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_names_fit_on_one_line_along_the_longer_axis_without_truncation() {
        let fonts = crate::markdown::font_collection();
        for name in [
            "README.md",
            "代码与文档🧭",
            "001-spatial-reading-01.md",
            "002-camera-coordinates-01.md",
            "a-very-long-unbroken_filename_with_more_than_twenty_four_characters.rs",
            "带有空格的文件名 very long document name with spaces 🧭.md",
        ] {
            for (width, height) in [
                (800.0, 200.0),
                (200.0, 900.0),
                (90.0, 24.0),
                (24.0, 900.0),
                (588.4, 526.0),
                (200.0, 200.0),
            ] {
                let mut title = SummaryTitle::new(name, Color::WHITE);
                title.fit(width, height, &fonts);
                let paragraph = title.paragraph.as_ref().unwrap();
                assert!(paragraph.height() <= width.min(height));
                assert!(paragraph.longest_line() <= width.max(height));
                assert!(paragraph.max_intrinsic_width() <= width.max(height));
                assert!(!paragraph.did_exceed_max_lines());
                assert_eq!(paragraph.line_number(), 1);
            }
        }
    }

    #[test]
    fn summary_paint_runs_along_the_longer_axis_inside_the_inset_frame() {
        let fonts = crate::markdown::font_collection();
        let mut title = SummaryTitle::new("002-camera-coordinates-01.md", Color::WHITE);
        // Reuse the title to also exercise refitting after orientation changes.
        for (width, height) in [(180, 840), (840, 180), (40, 900)] {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((width + 60, height + 80)).unwrap();
            surface.canvas().clear(Color::TRANSPARENT);
            title.paint(
                surface.canvas(),
                Rect::new(20.0, 30.0, width as f64, height as f64),
                &fonts,
            );
            let pixels = surface.peek_pixels().unwrap();
            let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, 0, 0);
            for y in 0..pixels.height() {
                for x in 0..pixels.width() {
                    if pixels.get_color((x, y)).a() != 0 {
                        left = left.min(x);
                        top = top.min(y);
                        right = right.max(x + 1);
                        bottom = bottom.max(y + 1);
                    }
                }
            }
            assert!(left < right && top < bottom, "The name must be visible");
            let inset = width.min(height) / 10;
            assert!(left >= 20 + inset && right <= 20 + width - inset);
            assert!(top >= 30 + inset && bottom <= 30 + height - inset);
            let (span, cross_span) = if height > width {
                (bottom - top, right - left)
            } else {
                (right - left, bottom - top)
            };
            assert!(
                span > cross_span * 5,
                "The single-line name must run along the frame's longer axis"
            );
        }
    }
}
