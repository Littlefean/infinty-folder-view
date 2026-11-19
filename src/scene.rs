use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Range,
    path::PathBuf,
};

use skia_safe::{
    Canvas, Color, Paint, Rect as SkRect,
    textlayout::{FontCollection, Paragraph},
};

use crate::{
    camera::{Camera, Rect},
    documents::{DocumentKind, ImportResult, SourceDocument},
    layout::{FOLDER_HEADER, Layout},
    markdown::{self, Highlighter, MarkdownDocument},
    presentation::{self, SummaryTitle},
};

const CARD_WIDTH: f64 = 720.0;
const CODE_WIDTH: f64 = 1024.0;
const PADDING: f32 = 30.0;
const CONTENT_TOP: f32 = 88.0;
pub const TOP_BAR: f64 = 88.0;
const INK: Color = Color::from_rgb(221, 231, 243);
const MUTED: Color = Color::from_rgb(128, 150, 173);
const BACKGROUND: Color = Color::from_rgb(10, 16, 24);

struct Card {
    node: usize,
    rect: Rect,
    body: MarkdownDocument,
    filename: Paragraph,
    meta: Paragraph,
    summary: SummaryTitle,
    accent: Color,
}

struct DocumentHit {
    card: usize,
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
    card: usize,
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

struct Folder {
    node: usize,
    label: Paragraph,
    summary: SummaryTitle,
}

struct NodeDrag {
    node: usize,
    nodes: Vec<usize>,
    double_click: bool,
    start: (f64, f64),
    last: (f64, f64),
    moved: bool,
}

struct Marquee {
    start: (f64, f64),
    screen_start: (f64, f64),
    rect: Rect,
    additive: BTreeSet<usize>,
    folder: Option<usize>,
    candidates: Vec<(usize, Rect, bool)>,
    moved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    Import,
    Arrange,
    Fit,
}

pub struct Scene {
    pending: VecDeque<(SourceDocument, usize)>,
    cards: Vec<Card>,
    folders: Vec<Folder>,
    layout: Layout,
    card_nodes: Vec<Option<usize>>,
    folder_nodes: Vec<Option<usize>>,
    selected_nodes: BTreeSet<usize>,
    folder_disclosures: BTreeMap<usize, bool>,
    revealed_nodes: BTreeSet<usize>,
    retained_reveals: BTreeSet<usize>,
    marquee: Option<Marquee>,
    last_node_click: Option<(usize, (f64, f64), std::time::Instant)>,
    node_drag: Option<NodeDrag>,
    buttons: [Paragraph; 3],
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
    #[cfg(test)]
    pub fn new(documents: Vec<SourceDocument>) -> Self {
        Self::from_import(ImportResult {
            documents,
            folders: Vec::new(),
            warnings: Vec::new(),
        })
    }

    pub fn from_import(import: ImportResult) -> Self {
        let fonts = markdown::font_collection();
        let total = import.documents.len();
        let lines = import.documents.iter().map(|doc| doc.line_count).sum();
        let mut paths: BTreeSet<PathBuf> = import.folders.into_iter().collect();
        for doc in &import.documents {
            let mut parent = doc.relative_path.parent();
            while let Some(path) = parent {
                if !path.as_os_str().is_empty() {
                    paths.insert(path.to_owned());
                }
                parent = path.parent();
            }
        }
        let mut layout = Layout::new();
        let mut ids = BTreeMap::new();
        let mut folders = Vec::new();
        for path in paths {
            let parent = path.parent().and_then(|path| ids.get(path)).copied();
            let node = layout.add_folder(parent);
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            folders.push(Folder {
                node,
                summary: SummaryTitle::new(name.as_ref(), Color::from_rgb(134, 185, 218)),
                label: markdown::paragraph(
                    &format!("▱  {name}"),
                    20.0,
                    Color::from_rgb(134, 185, 218),
                    1400.0,
                    &fonts,
                ),
            });
            ids.insert(path, node);
        }
        let mut pending = VecDeque::new();
        for source in import.documents {
            let parent = source
                .relative_path
                .parent()
                .and_then(|path| ids.get(path))
                .copied();
            let width = if matches!(source.kind, DocumentKind::Markdown) {
                CARD_WIDTH
            } else {
                CODE_WIDTH
            };
            let node = layout.add(
                parent,
                width,
                CONTENT_TOP as f64 + (source.line_count.max(1) as f64 * 22.0) + 34.0,
            );
            pending.push_back((source, node));
        }
        layout.arrange();
        let mut folder_nodes = vec![None; layout.nodes.len()];
        for (index, folder) in folders.iter().enumerate() {
            folder_nodes[folder.node] = Some(index);
        }
        let message = if import.warnings.is_empty() {
            String::new()
        } else {
            format!(
                "导入提示 {} 项：{}",
                import.warnings.len(),
                import.warnings[0]
            )
        };
        Self {
            heading: markdown::paragraph("MARKDOWN ATLAS", 19.0, INK, 320.0, &fonts),
            subtitle: markdown::paragraph(
                "文件夹、代码与文档，在空间里阅读。",
                12.0,
                MUTED,
                340.0,
                &fonts,
            ),
            controls: markdown::paragraph(
                "空白框选 · Shift 多选 · 单击展开 / 双击定位 · 标题拖动 · 正文选字 ⌘C   |   空格平移 · 滚轮缩放 · A 排版 · F 全览 · L 细节",
                12.0,
                MUTED,
                1200.0,
                &fonts,
            ),
            compact_controls: markdown::paragraph(
                "空白框选 · Shift 多选 · 单击展开 · 正文选字 · 空格平移 · 滚轮缩放 · ⌘O 导入",
                12.0,
                MUTED,
                900.0,
                &fonts,
            ),
            buttons: ["导入  ⌘O", "自动排版  A", "全览  F"]
                .map(|label| markdown::paragraph(label, 12.0, INK, 105.0, &fonts)),
            card_nodes: vec![None; layout.nodes.len()],
            folder_nodes,
            bounds: layout.bounds(),
            layout,
            folders,
            pending,
            cards: Vec::new(),
            fonts,
            highlighter: Highlighter::new(),
            camera: Camera::default(),
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
            selected_nodes: BTreeSet::new(),
            folder_disclosures: BTreeMap::new(),
            revealed_nodes: BTreeSet::new(),
            retained_reveals: BTreeSet::new(),
            marquee: None,
            last_node_click: None,
            node_drag: None,
            message,
        }
    }

    pub fn ready(&self) -> bool {
        self.pending.is_empty()
    }

    fn card(&self, id: usize) -> Option<&Card> {
        self.cards.get(id)
    }

    fn document_hit(&self, x: f64, y: f64, width: f64, height: f64) -> Option<DocumentHit> {
        if !self.ready()
            || ![x, y, width, height].iter().all(|value| value.is_finite())
            || width <= 0.0
            || height <= TOP_BAR + 38.0
            || x < 0.0
            || x > width
            || y < TOP_BAR
            || y >= height - 38.0
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
        if !world_x.is_finite() || !world_y.is_finite() {
            return None;
        }
        let node = self.hit_node(world_x, world_y)?;
        if self.is_summary(node) {
            return None;
        }
        let id = self.card_nodes[node]?;
        let card = &self.cards[id];
        let local_x = world_x - card.rect.x - PADDING as f64;
        let local_y = world_y - card.rect.y - CONTENT_TOP as f64;
        let content_width = card.rect.width - PADDING as f64 * 2.0;
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
                card: id,
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
        self.selected_nodes = BTreeSet::from([self.cards[id].node]);
        self.refresh_reveals();
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
        self.selected_nodes.clear();
        self.folder_disclosures.clear();
        self.revealed_nodes.clear();
        self.retained_reveals.clear();
        self.node_drag = None;
        self.marquee = None;
        self.last_node_click = None;
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
        let Some((source, node)) = self.pending.pop_front() else {
            return;
        };
        let width = self.layout.nodes[node].rect.width;
        let body_width = width as f32 - PADDING * 2.0;
        let (body, language, accent) = match &source.kind {
            DocumentKind::Markdown => (
                MarkdownDocument::new(&source.markdown, body_width, &self.fonts, &self.highlighter),
                "Markdown",
                Color::from_rgb(85, 196, 189),
            ),
            DocumentKind::Code(language) => (
                MarkdownDocument::from_code(
                    &source.markdown,
                    language,
                    body_width,
                    &self.fonts,
                    &self.highlighter,
                ),
                language.as_str(),
                Color::from_rgb(121, 163, 240),
            ),
            DocumentKind::Text => (
                MarkdownDocument::from_text(&source.markdown, body_width, &self.fonts),
                "Text",
                Color::from_rgb(203, 164, 112),
            ),
        };
        self.layout.nodes[node].rect.height = CONTENT_TOP as f64 + body.height as f64 + 34.0;
        let filename = source
            .path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| source.title.as_str().into());
        let card = Card {
            node,
            filename: markdown::paragraph(&filename, 14.0, INK, width as f32 - 220.0, &self.fonts),
            meta: markdown::paragraph(
                &format!("{language} · {} 行", source.line_count),
                12.0,
                MUTED,
                170.0,
                &self.fonts,
            ),
            summary: SummaryTitle::new(filename.as_ref(), accent),
            rect: self.layout.nodes[node].rect,
            body,
            accent,
        };
        self.layout_blocks += card.body.block_count;
        self.card_nodes[node] = Some(self.cards.len());
        self.cards.push(card);
        self.loaded += 1;
        self.layout.arrange();
        self.sync_layout();
    }

    fn sync_layout(&mut self) {
        for card in &mut self.cards {
            card.rect = self.layout.nodes[card.node].rect;
        }
        self.bounds = self.layout.bounds();
    }

    pub fn arrange(&mut self) {
        if !self.ready() {
            return;
        }
        self.finish_node_drag();
        self.finish_selection();
        if let Some(node) = self
            .selected_nodes
            .iter()
            .copied()
            .next()
            .filter(|id| self.selected_nodes.len() == 1 && self.folder_nodes[*id].is_some())
        {
            self.layout.arrange_folder(node);
            self.message = "已整理选中文件夹的直接子项".into();
        } else {
            self.layout.arrange();
            self.message = "已按目录层级自动排版".into();
        }
        self.sync_layout();
    }

    fn hit_node(&self, x: f64, y: f64) -> Option<usize> {
        fn visit(scene: &Scene, nodes: &[usize], x: f64, y: f64) -> Option<usize> {
            for &id in nodes.iter().rev() {
                let node = &scene.layout.nodes[id];
                let r = node.rect;
                if x >= r.x && x <= r.right() && y >= r.y && y <= r.bottom() {
                    return if scene.is_summary(id) {
                        Some(id)
                    } else {
                        visit(scene, &node.children, x, y).or(Some(id))
                    };
                }
            }
            None
        }
        visit(self, &self.layout.roots, x, y)
    }

    fn canvas_point(&self, x: f64, y: f64, width: f64, height: f64) -> Option<(f64, f64)> {
        if ![x, y, width, height].iter().all(|v| v.is_finite())
            || x < 0.0
            || x > width
            || y < TOP_BAR
            || y >= height - 38.0
        {
            return None;
        }
        let map = self.minimap_rect(width, height);
        if self.ready() && x >= map.x && x <= map.right() && y >= map.y && y <= map.bottom() {
            return None;
        }
        Some(self.camera.screen_to_world(x, y))
    }

    fn draggable_node(&self, x: f64, y: f64, width: f64, height: f64) -> Option<usize> {
        if !self.ready() {
            return None;
        }
        let (wx, wy) = self.canvas_point(x, y, width, height)?;
        let id = self.hit_node(wx, wy)?;
        let node = &self.layout.nodes[id];
        let folder = self.folder_nodes[id].is_some();
        let header = if folder { FOLDER_HEADER } else { 64.0 };
        (wy < node.rect.y + header || self.is_summary(id)).then_some(id)
    }

    pub fn draggable_at(&self, x: f64, y: f64, width: f64, height: f64) -> bool {
        self.draggable_node(x, y, width, height).is_some()
    }

    #[cfg(test)]
    pub fn begin_node_drag(&mut self, x: f64, y: f64, width: f64, height: f64) -> bool {
        if self.draggable_node(x, y, width, height).is_none() {
            return false;
        }
        self.begin_node_pointer(x, y, width, height, false)
    }

    fn refresh_reveals(&mut self) {
        self.folder_disclosures
            .retain(|id, _| self.selected_nodes.contains(id));
        let mut revealed = self.retained_reveals.clone();
        for &id in &self.selected_nodes {
            if self.layout.nodes[id].is_folder {
                let Some(&recursive) = self.folder_disclosures.get(&id) else {
                    continue;
                };
                if recursive {
                    let mut stack = vec![id];
                    while let Some(child) = stack.pop() {
                        revealed.insert(child);
                        stack.extend(self.layout.nodes[child].children.iter().copied());
                    }
                }
            }
            let mut ancestor = Some(id);
            while let Some(node) = ancestor {
                revealed.insert(node);
                ancestor = self.layout.nodes[node].parent;
            }
        }
        self.revealed_nodes = revealed;
    }

    fn is_summary(&self, id: usize) -> bool {
        if !self.lod || self.revealed_nodes.contains(&id) {
            return false;
        }
        let node = &self.layout.nodes[id];
        if !node.is_folder {
            return self.camera.scale < presentation::FILE_DETAIL_SCALE;
        }
        let mut depth = 0;
        let mut parent = node.parent;
        while let Some(id) = parent {
            depth += 1;
            parent = self.layout.nodes[id].parent;
        }
        node.children.is_empty()
            || self.camera.scale < presentation::folder_reveal_scale(node.rect, depth)
    }

    pub fn begin_node_pointer(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        additive: bool,
    ) -> bool {
        if !self.ready() {
            return false;
        }
        let Some(point) = self.canvas_point(x, y, width, height) else {
            return false;
        };
        // The first click can remove a cover and expose a child or text under the pointer.
        let repeated = self
            .last_node_click
            .filter(|(id, previous, time)| {
                !additive
                    && self.selected_nodes.contains(id)
                    && time.elapsed().as_millis() <= 500
                    && (x - previous.0).hypot(y - previous.1) <= 6.0
            })
            .map(|(id, _, _)| id);
        let hit = repeated.or_else(|| self.hit_node(point.0, point.1));
        if repeated.is_none() && self.text_at(x, y, width, height) {
            self.last_node_click = None;
            return false;
        }
        self.finish_selection();
        self.retained_reveals = self.revealed_nodes.clone();
        self.message.clear();
        let folder_blank = hit.is_some_and(|id| {
            self.layout.nodes[id].is_folder
                && !self.is_summary(id)
                && point.1 >= self.layout.nodes[id].rect.y + FOLDER_HEADER
        });
        if repeated.is_none() && (hit.is_none() || folder_blank) {
            // Freeze candidates before selection reveals files or closes folders.
            let candidates = self
                .visible_nodes(self.bounds)
                .into_iter()
                .map(|id| {
                    (
                        id,
                        self.layout.nodes[id].rect,
                        self.layout.nodes[id].is_folder && !self.is_summary(id),
                    )
                })
                .collect();
            let additive = if additive {
                self.selected_nodes.clone()
            } else {
                BTreeSet::new()
            };
            self.selected_nodes = additive.clone();
            self.selection = None;
            self.marquee = Some(Marquee {
                start: point,
                screen_start: (x, y),
                rect: Rect::new(point.0, point.1, 0.0, 0.0),
                additive,
                folder: hit,
                candidates,
                moved: false,
            });
            self.last_node_click = None;
        } else if let Some(node) = hit {
            if additive && !self.selected_nodes.insert(node) {
                self.selected_nodes.remove(&node);
                self.retained_reveals.clear();
                self.last_node_click = None;
                self.refresh_reveals();
                return true;
            }
            if !additive && !self.selected_nodes.contains(&node) {
                self.selected_nodes = BTreeSet::from([node]);
            }
            // Preserve copied text only while moving its original file.
            if self
                .selection
                .as_ref()
                .is_some_and(|selection| self.cards[selection.card].node != node)
            {
                self.selection = None;
            }
            self.node_drag = Some(NodeDrag {
                node,
                nodes: self.selected_nodes.iter().copied().collect(),
                double_click: repeated == Some(node),
                start: (x, y),
                last: point,
                moved: false,
            });
        }
        self.refresh_reveals();
        true
    }

    pub fn update_node_drag(&mut self, x: f64, y: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let Some(drag) = &mut self.node_drag else {
            return;
        };
        if !drag.moved && (x - drag.start.0).hypot(y - drag.start.1) < 4.0 {
            return;
        }
        drag.moved = true;
        self.last_node_click = None;
        let point = self.camera.screen_to_world(x, y);
        self.layout
            .move_nodes(&drag.nodes, point.0 - drag.last.0, point.1 - drag.last.1);
        drag.last = point;
        self.sync_layout();
    }

    pub fn update_marquee(&mut self, x: f64, y: f64, width: f64, height: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let Some(marquee) = &mut self.marquee else {
            return;
        };
        if !marquee.moved && (x - marquee.screen_start.0).hypot(y - marquee.screen_start.1) < 4.0 {
            return;
        }
        marquee.moved = true;
        let end = self
            .camera
            .screen_to_world(x.clamp(0.0, width), y.clamp(TOP_BAR, height - 38.0));
        marquee.rect = Rect::new(
            marquee.start.0.min(end.0),
            marquee.start.1.min(end.1),
            (end.0 - marquee.start.0).abs(),
            (end.1 - marquee.start.1).abs(),
        );
        let rect = marquee.rect;
        self.selected_nodes = marquee.additive.clone();
        for &(id, node, enclosed) in &marquee.candidates {
            let hit = if enclosed {
                node.x >= rect.x
                    && node.y >= rect.y
                    && node.right() <= rect.right()
                    && node.bottom() <= rect.bottom()
            } else {
                node.intersects(&rect)
            };
            if hit {
                self.selected_nodes.insert(id);
            }
        }
        self.refresh_reveals();
    }

    pub fn end_node_pointer(&mut self, width: f64, height: f64) -> Option<Camera> {
        let mut focus = None;
        let mut clicked = None;
        if let Some(drag) = self.node_drag.take()
            && !drag.moved
        {
            clicked = Some((drag.node, drag.start, drag.double_click));
        }
        if let Some(marquee) = self.marquee.take()
            && !marquee.moved
            && let Some(folder) = marquee.folder
        {
            if !self.selected_nodes.insert(folder) {
                self.selected_nodes.remove(&folder);
            }
            if self.selected_nodes.contains(&folder) {
                clicked = Some((folder, marquee.screen_start, false));
            }
        }
        if let Some((node, point, double)) = clicked {
            if self.layout.nodes[node].is_folder {
                self.folder_disclosures
                    .entry(node)
                    .and_modify(|recursive| *recursive |= double)
                    .or_insert(double);
            } else if double {
                let rect = self.layout.nodes[node].rect;
                let scale = ((width - 140.0) / rect.width)
                    .min((height - TOP_BAR - 100.0) / rect.height.min(900.0))
                    .clamp(presentation::FILE_DETAIL_SCALE * 1.05, 1.0);
                focus = Some(Camera {
                    x: ((width - rect.width * scale) / 2.0).max(40.0) - rect.x * scale,
                    y: TOP_BAR + 32.0 - rect.y * scale,
                    scale,
                });
            }
            self.last_node_click = if double {
                None
            } else {
                Some((node, point, std::time::Instant::now()))
            };
        } else {
            self.last_node_click = None;
        }
        self.retained_reveals.clear();
        self.refresh_reveals();
        focus
    }

    /// Cancellation must never be interpreted as a folder click.
    pub fn finish_node_drag(&mut self) {
        self.node_drag = None;
        self.marquee = None;
        self.last_node_click = None;
        self.retained_reveals.clear();
        self.refresh_reveals();
    }
    pub fn is_dragging_node(&self) -> bool {
        self.node_drag.is_some()
    }
    pub fn is_marquee_selecting(&self) -> bool {
        self.marquee.is_some()
    }

    fn toolbar_rect(index: usize, width: f64) -> Rect {
        Rect::new(width - 360.0 + index as f64 * 112.0, 24.0, 104.0, 36.0)
    }

    pub fn toolbar_action(&self, x: f64, y: f64, width: f64) -> Option<ToolbarAction> {
        [
            ToolbarAction::Import,
            ToolbarAction::Arrange,
            ToolbarAction::Fit,
        ]
        .into_iter()
        .enumerate()
        .find_map(|(i, action)| {
            let r = Self::toolbar_rect(i, width);
            (x >= r.x && x <= r.right() && y >= r.y && y <= r.bottom()).then_some(action)
        })
    }

    fn visible_nodes(&self, visible: Rect) -> Vec<usize> {
        let mut found = Vec::new();
        let mut stack: Vec<_> = self.layout.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let node = &self.layout.nodes[id];
            if node.rect.intersects(&visible) {
                found.push(id);
                if !self.is_summary(id) {
                    stack.extend(node.children.iter().rev().copied());
                }
            }
        }
        found
    }

    fn draw_folder(&mut self, canvas: &Canvas, id: usize) {
        let summary = self.is_summary(id);
        let folder = &mut self.folders[self.folder_nodes[id].unwrap()];
        let r = self.layout.nodes[id].rect;
        let frame = SkRect::from_xywh(r.x as f32, r.y as f32, r.width as f32, r.height as f32);
        let mut paint = Paint::default();
        paint
            .set_anti_alias(true)
            .set_color(Color::from_rgb(14, 24, 35));
        canvas.draw_round_rect(frame, 14.0, 14.0, &paint);
        paint
            .set_color(if self.selected_nodes.contains(&id) {
                Color::from_rgb(100, 209, 198)
            } else {
                Color::from_rgb(57, 88, 114)
            })
            .set_style(skia_safe::paint::Style::Stroke)
            .set_stroke_width(presentation::border_width(
                r,
                self.camera.scale,
                true,
                self.selected_nodes.contains(&id),
            ));
        canvas.draw_round_rect(frame, 14.0, 14.0, &paint);
        if summary {
            folder.summary.paint(canvas, r, &self.fonts);
            return;
        }
        canvas.save();
        canvas.clip_rect(
            SkRect::from_xywh(
                r.x as f32 + 20.0,
                r.y as f32 + 8.0,
                (r.width - 40.0) as f32,
                (FOLDER_HEADER - 12.0) as f32,
            ),
            None,
            true,
        );
        folder
            .label
            .paint(canvas, (r.x as f32 + 24.0, r.y as f32 + 20.0));
        canvas.restore();
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
        let visible_nodes = self.visible_nodes(visible);
        for node in visible_nodes {
            if self.folder_nodes[node].is_some() {
                self.draw_folder(canvas, node);
            }
            if let Some(id) = self.card_nodes[node] {
                let card_index = id;
                let summary = self.is_summary(node);
                let card = &mut self.cards[id];
                self.visible_cards += 1;
                canvas.save();
                canvas.translate((card.rect.x as f32, card.rect.y as f32));
                let rect =
                    SkRect::from_xywh(0.0, 0.0, card.rect.width as f32, card.rect.height as f32);
                let mut fill = Paint::default();
                fill.set_color(Color::from_rgb(19, 28, 40))
                    .set_anti_alias(true);
                canvas.draw_round_rect(rect, 10.0, 10.0, &fill);
                let mut border = Paint::default();
                border
                    .set_color(if self.selected_nodes.contains(&node) {
                        Color::from_rgb(100, 209, 198)
                    } else {
                        Color::from_rgb(41, 57, 75)
                    })
                    .set_style(skia_safe::paint::Style::Stroke)
                    .set_stroke_width(presentation::border_width(
                        card.rect,
                        self.camera.scale,
                        false,
                        self.selected_nodes.contains(&node),
                    ))
                    .set_anti_alias(true);
                canvas.draw_round_rect(rect, 10.0, 10.0, &border);
                fill.set_color(card.accent);
                canvas.draw_round_rect(SkRect::from_xywh(30.0, 27.0, 5.0, 18.0), 2.0, 2.0, &fill);
                if !summary {
                    canvas.save();
                    canvas.clip_rect(
                        SkRect::from_xywh(46.0, 22.0, card.rect.width as f32 - 220.0, 30.0),
                        None,
                        true,
                    );
                    card.filename.paint(canvas, (46.0, 27.0));
                    canvas.restore();
                    card.meta
                        .paint(canvas, (card.rect.width as f32 - 180.0, 29.0));
                    fill.set_color(Color::from_rgb(37, 51, 68));
                    canvas.draw_rect(
                        SkRect::from_xywh(
                            PADDING,
                            63.0,
                            card.rect.width as f32 - PADDING * 2.0,
                            1.0,
                        ),
                        &fill,
                    );
                    canvas.translate((PADDING, CONTENT_TOP));
                    self.visible_blocks += card.body.paint(
                        canvas,
                        (visible.y - card.rect.y) as f32 - CONTENT_TOP,
                        (visible.bottom() - card.rect.y) as f32 - CONTENT_TOP,
                        selected
                            .as_ref()
                            .and_then(|(id, range)| (*id == card_index).then(|| range.clone())),
                    );
                } else {
                    card.summary.paint(
                        canvas,
                        Rect::new(0.0, 0.0, card.rect.width, card.rect.height),
                        &self.fonts,
                    );
                }
                canvas.restore();
            }
        }
        if let Some(marquee) = &self.marquee
            && marquee.moved
        {
            let r = marquee.rect;
            let rect = SkRect::from_xywh(r.x as f32, r.y as f32, r.width as f32, r.height as f32);
            let mut paint = Paint::default();
            paint
                .set_color(Color::from_argb(32, 100, 209, 198))
                .set_anti_alias(true);
            canvas.draw_rect(rect, &paint);
            paint
                .set_color(Color::from_rgb(100, 209, 198))
                .set_style(skia_safe::paint::Style::Stroke)
                .set_stroke_width((1.0 / self.camera.scale) as f32);
            canvas.draw_rect(rect, &paint);
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
        } else if !self.selected_nodes.is_empty() {
            format!(
                "已选择 {} 项 · {:.0}%\nShift 点选增减 · 拖动标题一起移动",
                self.selected_nodes.len(),
                self.camera.scale * 100.0
            )
        } else if !self.ready() {
            format!("正在排版  {}/{}  份文件…", self.loaded, self.total)
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
        let status_x = 350.0;
        let narrow = width < 900.0;
        let status_width = if narrow {
            width as f32 - 48.0
        } else {
            width as f32 - 730.0
        };
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
        if let Some((_, paragraph)) = &self.status
            && !narrow
        {
            canvas.save();
            canvas.clip_rect(
                SkRect::from_xywh(status_x, 16.0, status_width, 64.0),
                None,
                true,
            );
            paragraph.paint(canvas, (status_x, 22.0));
            canvas.restore();
        }
        for (index, label) in self.buttons.iter().enumerate() {
            let r = Self::toolbar_rect(index, width);
            paint.set_color(if index == 0 {
                Color::from_rgb(38, 74, 79)
            } else {
                Color::from_rgb(27, 41, 57)
            });
            canvas.draw_round_rect(
                SkRect::from_xywh(r.x as f32, r.y as f32, r.width as f32, r.height as f32),
                7.0,
                7.0,
                &paint,
            );
            label.paint(canvas, (r.x as f32 + 12.0, r.y as f32 + 10.0));
        }
        if narrow {
            if let Some((_, paragraph)) = &self.status {
                canvas.save();
                canvas.clip_rect(
                    SkRect::from_xywh(24.0, height as f32 - 30.0, status_width, 25.0),
                    None,
                    true,
                );
                paragraph.paint(canvas, (24.0, height as f32 - 26.0));
                canvas.restore();
            }
        } else if width < 1100.0 {
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
            rect.x + (rect.width - self.bounds.width * s) / 2.0 - self.bounds.x * s,
            rect.y + (rect.height - self.bounds.height * s) / 2.0 - self.bounds.y * s,
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
        for node in &self.layout.nodes {
            paint
                .set_color(if node.is_folder {
                    Color::from_rgb(63, 100, 127)
                } else {
                    Color::from_rgb(49, 69, 89)
                })
                .set_style(if node.is_folder {
                    skia_safe::paint::Style::Stroke
                } else {
                    skia_safe::paint::Style::Fill
                })
                .set_stroke_width(0.8);
            canvas.draw_rect(
                SkRect::from_xywh(
                    (ox + node.rect.x * s) as f32,
                    (oy + node.rect.y * s) as f32,
                    (node.rect.width * s).max(1.0) as f32,
                    (node.rect.height * s).max(1.0) as f32,
                ),
                &paint,
            );
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
#[path = "scene_tests.rs"]
mod workspace_tests;

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
            kind: DocumentKind::Markdown,
            relative_path: PathBuf::from(name),
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
        let card = &scene.cards[0];
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
        let body = &scene.cards[0].body;
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
        let card = &scene.cards[0];
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
                expected.map(|offset| (0, offset))
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
        assert_eq!(scene.selection.as_ref().unwrap().card, 0);
        assert_eq!(
            scene.selected_text(),
            Some(
                scene.cards[0]
                    .body
                    .selected_text(0..scene.cards[0].body.text_len())
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
        let body = &scene.cards[0].body;
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
        let card = &scene.cards[0];
        let bottom_right = scene.camera.world_to_screen(
            card.rect.right() - PADDING as f64,
            card.rect.y + CONTENT_TOP as f64 + card.body.height as f64,
        );
        assert!((bottom_right.0 - (WIDTH - 24.0)).abs() < 1.0e-8);
        assert!((bottom_right.1 - (HEIGHT - 38.0 - 24.0)).abs() < 1.0e-8);
        assert!(!scene.pan_selection(-100.0, -100.0, WIDTH, HEIGHT));
        assert!(scene.pan_selection(1.0e6, 1.0e6, WIDTH, HEIGHT));
        let card = &scene.cards[0];
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
        let card = &scene.cards[0];
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
