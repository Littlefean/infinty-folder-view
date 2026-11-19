use super::*;
use crate::documents::load_workspace;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

const WIDTH: f64 = 1440.0;
const HEIGHT: f64 = 940.0;
const RUST_SOURCE: &str = "fn main() {\n    println!(\"**literal**\");\n}\n";
const TEXT_SOURCE: &str = "# **This stays literal**\n\n  Keep indentation.\n";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    directory: PathBuf,
    scene: Scene,
    cards: BTreeMap<String, usize>,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "infinity-workspace-scene-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        for folder in ["src/nested", "other", "empty"] {
            fs::create_dir_all(directory.join(folder)).unwrap();
        }
        for (name, source) in [
            ("README.md", "# A workspace\n\nReadable **Markdown**."),
            ("src/a.rs", RUST_SOURCE),
            ("src/b.rs", "fn second() {}\n"),
            ("src/nested/read.txt", TEXT_SOURCE),
            ("other/out.txt", "An unrelated document.\n"),
        ] {
            fs::write(directory.join(name), source).unwrap();
        }
        let import = load_workspace(&directory).unwrap();
        let cards = import
            .documents
            .iter()
            .enumerate()
            .map(|(index, document)| {
                (
                    document
                        .path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    index,
                )
            })
            .collect();
        let mut scene = Scene::from_import(import);
        while !scene.ready() {
            scene.build_next();
        }
        Self {
            directory,
            scene,
            cards,
        }
    }

    fn card(&self, name: &str) -> usize {
        self.cards[name]
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn focus(scene: &mut Scene, node: usize) {
    let rect = scene.layout.nodes[node].rect;
    scene.camera = Camera {
        x: 180.0 - rect.x * 0.75,
        y: 200.0 - rect.y * 0.75,
        scale: 0.75,
    };
}

fn text_point(scene: &Scene, card: usize) -> (f64, f64) {
    let card = &scene.cards[card];
    for y in (1..card.body.height.ceil() as usize).step_by(2) {
        for x in (1..(card.rect.width - PADDING as f64 * 2.0) as usize).step_by(2) {
            if card.body.hit_test(x as f32, y as f32, false).is_some() {
                return scene.camera.world_to_screen(
                    card.rect.x + PADDING as f64 + x as f64,
                    card.rect.y + CONTENT_TOP as f64 + y as f64,
                );
            }
        }
    }
    panic!("Document has no hittable text");
}

fn select_document(scene: &mut Scene, card: usize) {
    focus(scene, scene.cards[card].node);
    let point = text_point(scene, card);
    assert!(scene.begin_selection(point.0, point.1, WIDTH, HEIGHT, 1, false));
    assert!(scene.select_all());
}

fn header_point(scene: &Scene, node: usize) -> (f64, f64) {
    let rect = scene.layout.nodes[node].rect;
    scene.camera.world_to_screen(rect.x + 80.0, rect.y + 28.0)
}

fn descendents(scene: &Scene, root: usize) -> Vec<usize> {
    let mut result = vec![root];
    let mut cursor = 0;
    while cursor < result.len() {
        let node = result[cursor];
        result.extend(scene.layout.nodes[node].children.iter().copied());
        cursor += 1;
    }
    result
}

#[test]
fn imported_tree_preserves_nested_empty_folders_and_literal_code_and_text_copy() {
    let mut fixture = Fixture::new();
    let code = fixture.card("a.rs");
    let text = fixture.card("read.txt");
    let markdown = fixture.card("README.md");
    let scene = &mut fixture.scene;
    assert_eq!(scene.loaded, 5);
    assert_eq!(scene.total, 5);
    assert_eq!(scene.folders.len(), 5);
    assert_eq!(scene.layout.roots.len(), 1);
    let root = scene.layout.roots[0];
    let src = scene.layout.nodes[scene.cards[code].node].parent.unwrap();
    let nested = scene.layout.nodes[scene.cards[text].node].parent.unwrap();
    assert_eq!(scene.layout.nodes[nested].parent, Some(src));
    assert_eq!(scene.layout.nodes[src].parent, Some(root));
    assert!(
        scene
            .layout
            .nodes
            .iter()
            .any(|node| node.is_folder && node.children.is_empty())
    );
    for (card, expected) in [(code, RUST_SOURCE), (text, TEXT_SOURCE)] {
        select_document(scene, card);
        assert_eq!(scene.selected_text().as_deref(), Some(expected));
        assert_eq!(scene.cards[card].rect.width, CODE_WIDTH);
    }
    select_document(scene, markdown);
    let displayed = scene.selected_text().unwrap();
    assert!(displayed.contains("Readable Markdown."));
    assert!(!displayed.contains("**"));
    assert_eq!(scene.cards[markdown].rect.width, CARD_WIDTH);
}

#[test]
fn body_selection_and_screen_overlays_do_not_begin_node_drags() {
    let mut fixture = Fixture::new();
    let card = fixture.card("a.rs");
    let scene = &mut fixture.scene;
    focus(scene, scene.cards[card].node);
    let point = text_point(scene, card);
    assert!(scene.text_at(point.0, point.1, WIDTH, HEIGHT));
    assert!(!scene.draggable_at(point.0, point.1, WIDTH, HEIGHT));
    assert!(!scene.begin_node_drag(point.0, point.1, WIDTH, HEIGHT));
    assert!(scene.begin_selection(point.0, point.1, WIDTH, HEIGHT, 2, false));
    assert!(scene.is_selecting());
    assert!(!scene.is_dragging_node());
    scene.finish_selection();
    for (index, action) in [
        ToolbarAction::Import,
        ToolbarAction::Arrange,
        ToolbarAction::Fit,
    ]
    .into_iter()
    .enumerate()
    {
        let button = Scene::toolbar_rect(index, WIDTH);
        let point = (
            button.x + button.width / 2.0,
            button.y + button.height / 2.0,
        );
        assert_eq!(scene.toolbar_action(point.0, point.1, WIDTH), Some(action));
        assert!(!scene.begin_node_drag(point.0, point.1, WIDTH, HEIGHT));
    }
    let map = scene.minimap_rect(WIDTH, HEIGHT);
    for (x, y) in [
        (map.x + 20.0, map.y + 20.0),
        (180.0, HEIGHT - 20.0),
        (180.0, 10.0),
    ] {
        assert!(!scene.begin_node_drag(x, y, WIDTH, HEIGHT));
        assert!(!scene.begin_selection(x, y, WIDTH, HEIGHT, 1, false));
    }
    assert!(!scene.is_dragging_node());
}

#[test]
fn header_drag_pushes_a_sibling_and_keeps_selection_and_hit_geometry_in_sync() {
    let mut fixture = Fixture::new();
    let a = fixture.card("a.rs");
    let b = fixture.card("b.rs");
    let scene = &mut fixture.scene;
    select_document(scene, a);
    let selected = scene.selected_text();
    let before_a = scene.cards[a].rect;
    let before_b = scene.cards[b].rect;
    let node = scene.cards[a].node;
    let start = header_point(scene, node);
    assert!(scene.begin_node_drag(start.0, start.1, WIDTH, HEIGHT));
    scene.update_node_drag(start.0 + 1.0, start.1 + 1.0);
    assert_eq!(
        scene.cards[a].rect, before_a,
        "A click must not nudge geometry"
    );
    let dx = before_b.x - before_a.x;
    let dy = before_b.y - before_a.y;
    scene.update_node_drag(
        start.0 + dx * scene.camera.scale,
        start.1 + dy * scene.camera.scale,
    );
    scene.finish_node_drag();
    near(scene.cards[a].rect.x, before_a.x + dx);
    near(scene.cards[a].rect.y, before_a.y + dy);
    assert_ne!(
        scene.cards[b].rect, before_b,
        "The contacted sibling should be pushed"
    );
    assert!(!scene.cards[a].rect.intersects(&scene.cards[b].rect));
    assert_eq!(scene.selection.as_ref().unwrap().card, a);
    assert_eq!(scene.selected_text(), selected);
    for card in [a, b] {
        assert_eq!(
            scene.cards[card].rect,
            scene.layout.nodes[scene.cards[card].node].rect
        );
        focus(scene, scene.cards[card].node);
        let point = text_point(scene, card);
        assert_eq!(
            scene
                .document_hit(point.0, point.1, WIDTH, HEIGHT)
                .unwrap()
                .card,
            card
        );
    }
}

#[test]
fn dragging_folder_moves_its_entire_subtree_without_changing_internal_offsets() {
    let mut fixture = Fixture::new();
    let code = fixture.card("a.rs");
    let scene = &mut fixture.scene;
    let folder = scene.layout.nodes[scene.cards[code].node].parent.unwrap();
    let nodes = descendents(scene, folder);
    assert!(nodes.len() >= 5);
    let before: Vec<_> = nodes
        .iter()
        .map(|&id| scene.layout.nodes[id].rect)
        .collect();
    focus(scene, folder);
    let start = header_point(scene, folder);
    assert!(scene.begin_node_drag(start.0, start.1, WIDTH, HEIGHT));
    scene.update_node_drag(start.0 + 180.0, start.1 + 90.0);
    scene.finish_node_drag();
    for (&id, original) in nodes.iter().zip(before) {
        let moved = scene.layout.nodes[id].rect;
        near(moved.x, original.x + 240.0);
        near(moved.y, original.y + 120.0);
        near(moved.width, original.width);
        near(moved.height, original.height);
        if let Some(card) = scene.card_nodes[id] {
            assert_eq!(scene.cards[card].rect, moved);
        }
    }
}

#[test]
fn minimap_centers_negative_world_bounds_after_a_root_drag() {
    let mut fixture = Fixture::new();
    let scene = &mut fixture.scene;
    let root = scene.layout.roots[0];
    focus(scene, root);
    let start = header_point(scene, root);
    assert!(scene.begin_node_drag(start.0, start.1, WIDTH, HEIGHT));
    scene.update_node_drag(start.0 - 6000.0, start.1 - 4500.0);
    scene.finish_node_drag();
    assert!(scene.bounds.x < 0.0 && scene.bounds.y < 0.0);
    let bounds_center = (
        scene.bounds.x + scene.bounds.width / 2.0,
        scene.bounds.y + scene.bounds.height / 2.0,
    );
    let map = scene.minimap_rect(WIDTH, HEIGHT);
    let (scale, ox, oy) = scene.minimap_transform(map);
    near(bounds_center.0 * scale + ox, map.x + map.width / 2.0);
    near(bounds_center.1 * scale + oy, map.y + map.height / 2.0);
    assert!(scene.navigate_minimap(
        map.x + map.width / 2.0,
        map.y + map.height / 2.0,
        WIDTH,
        HEIGHT
    ));
    let screen = scene
        .camera
        .world_to_screen(bounds_center.0, bounds_center.1);
    near(screen.0, WIDTH / 2.0);
    near(screen.1, (HEIGHT - 38.0 + TOP_BAR) / 2.0);
}

#[test]
fn arranging_selected_folder_preserves_unrelated_branch_and_nested_internal_layout() {
    let mut fixture = Fixture::new();
    let a = fixture.card("a.rs");
    let nested_card = fixture.card("read.txt");
    let other_card = fixture.card("out.txt");
    let scene = &mut fixture.scene;
    let src = scene.layout.nodes[scene.cards[a].node].parent.unwrap();
    let nested = scene.layout.nodes[scene.cards[nested_card].node]
        .parent
        .unwrap();
    let other = scene.layout.nodes[scene.cards[other_card].node]
        .parent
        .unwrap();
    scene.layout.move_node(other, 10000.0, 0.0);
    scene.layout.move_node(scene.cards[a].node, 1200.0, 200.0);
    scene.sync_layout();
    let scattered = scene.cards[a].rect;
    let other_before = scene.layout.nodes[other].rect;
    let other_card_before = scene.cards[other_card].rect;
    let nested_offset = (
        scene.cards[nested_card].rect.x - scene.layout.nodes[nested].rect.x,
        scene.cards[nested_card].rect.y - scene.layout.nodes[nested].rect.y,
    );
    focus(scene, src);
    let point = header_point(scene, src);
    assert!(scene.begin_node_drag(point.0, point.1, WIDTH, HEIGHT));
    scene.finish_node_drag();
    assert_eq!(scene.selected_nodes, BTreeSet::from([src]));
    scene.arrange();
    assert_ne!(scene.cards[a].rect, scattered);
    assert_eq!(scene.layout.nodes[other].rect, other_before);
    assert_eq!(scene.cards[other_card].rect, other_card_before);
    near(
        scene.cards[nested_card].rect.x - scene.layout.nodes[nested].rect.x,
        nested_offset.0,
    );
    near(
        scene.cards[nested_card].rect.y - scene.layout.nodes[nested].rect.y,
        nested_offset.1,
    );
    assert!(
        scene
            .cards
            .iter()
            .all(|card| card.rect == scene.layout.nodes[card.node].rect)
    );
}

fn zoom_to(scene: &mut Scene, node: usize, scale: f64) {
    let rect = scene.layout.nodes[node].rect;
    scene.camera = Camera {
        x: 200.0 - rect.x * scale,
        y: 200.0 - rect.y * scale,
        scale,
    };
}

fn click_node(scene: &mut Scene, node: usize, additive: bool) {
    let point = header_point(scene, node);
    assert!(scene.begin_node_pointer(point.0, point.1, WIDTH, HEIGHT, additive));
    assert!(scene.end_node_pointer(WIDTH, HEIGHT).is_none());
}

#[test]
fn zoom_covers_obey_folder_depth_and_exclude_hidden_descendants_from_hits() {
    let mut fixture = Fixture::new();
    let scene = &mut fixture.scene;
    let root = scene.layout.roots[0];
    zoom_to(scene, root, 0.015);
    let before: Vec<_> = scene.layout.nodes.iter().map(|n| n.rect).collect();
    assert!(scene.is_summary(root));
    assert_eq!(scene.visible_nodes(scene.bounds), vec![root]);
    let child = scene.cards[0].rect;
    assert_eq!(scene.hit_node(child.x + 20.0, child.y + 20.0), Some(root));
    scene.camera.scale = 1.0;
    assert!(!scene.is_summary(root));
    assert!(scene.visible_nodes(scene.bounds).len() > 1);
    assert_eq!(
        scene
            .layout
            .nodes
            .iter()
            .map(|n| n.rect)
            .collect::<Vec<_>>(),
        before
    );
}

#[test]
fn folder_click_opens_one_layer_and_double_click_opens_subtree_without_zooming() {
    let mut fixture = Fixture::new();
    let scene = &mut fixture.scene;
    let root = scene.layout.roots[0];
    zoom_to(scene, root, 0.015);
    let before = scene.camera;
    // Place the click over a child: after the first click it no longer hits the root.
    let child = scene.layout.nodes[root].children[0];
    let rect = scene.layout.nodes[child].rect;
    let point = scene.camera.world_to_screen(rect.x + 10.0, rect.y + 10.0);
    assert!(scene.begin_node_pointer(point.0, point.1, WIDTH, HEIGHT, false));
    assert!(scene.is_summary(root), "Folder clicks commit on release");
    scene.end_node_pointer(WIDTH, HEIGHT);
    assert!(!scene.is_summary(root));
    assert!(
        scene.layout.nodes[root]
            .children
            .iter()
            .all(|&id| scene.is_summary(id))
    );
    assert!(scene.begin_node_pointer(point.0, point.1, WIDTH, HEIGHT, false));
    assert!(scene.end_node_pointer(WIDTH, HEIGHT).is_none());
    assert_eq!(scene.camera, before);
    assert!(
        scene
            .layout
            .nodes
            .iter()
            .enumerate()
            .all(|(id, _)| !scene.is_summary(id))
    );
    scene.clear_selection();
    assert!(scene.is_summary(root));
}

#[test]
fn click_file_reveals_its_source_and_ancestors_only_and_double_click_focuses() {
    let mut fixture = Fixture::new();
    let card = fixture.card("a.rs");
    let other = fixture.card("b.rs");
    let scene = &mut fixture.scene;
    let node = scene.cards[card].node;
    zoom_to(scene, node, 0.3);
    let camera = scene.camera;
    let before: Vec<_> = scene.layout.nodes.iter().map(|n| n.rect).collect();
    assert!(scene.is_summary(node));
    click_node(scene, node, false);
    assert!(!scene.is_summary(node));
    assert!(scene.is_summary(scene.cards[other].node));
    assert_eq!(scene.camera, camera);
    assert_eq!(
        scene
            .layout
            .nodes
            .iter()
            .map(|n| n.rect)
            .collect::<Vec<_>>(),
        before
    );
    let text = text_point(scene, card);
    assert!(scene.text_at(text.0, text.1, WIDTH, HEIGHT));
    let point = header_point(scene, node);
    assert!(scene.begin_node_pointer(point.0, point.1, WIDTH, HEIGHT, false));
    let destination = scene.end_node_pointer(WIDTH, HEIGHT).unwrap();
    assert!(destination.scale >= presentation::FILE_DETAIL_SCALE);
    scene.clear_selection();
    assert!(scene.is_summary(node));
}

#[test]
fn shift_click_toggles_nodes_and_drag_preserves_multiple_selection() {
    let mut fixture = Fixture::new();
    let a = fixture.card("a.rs");
    let b = fixture.card("b.rs");
    let scene = &mut fixture.scene;
    let (a, b) = (scene.cards[a].node, scene.cards[b].node);
    zoom_to(scene, a, 0.3);
    click_node(scene, a, false);
    click_node(scene, b, true);
    assert_eq!(scene.selected_nodes, BTreeSet::from([a, b]));
    click_node(scene, b, true);
    assert_eq!(scene.selected_nodes, BTreeSet::from([a]));
    click_node(scene, b, true);
    let original_a = scene.layout.nodes[a].rect;
    let original_b = scene.layout.nodes[b].rect;
    let point = header_point(scene, a);
    assert!(scene.begin_node_pointer(point.0, point.1, WIDTH, HEIGHT, false));
    scene.update_node_drag(point.0 + 24.0, point.1 + 18.0);
    scene.end_node_pointer(WIDTH, HEIGHT);
    for (id, before) in [(a, original_a), (b, original_b)] {
        near(scene.layout.nodes[id].rect.x, before.x + 80.0);
        near(scene.layout.nodes[id].rect.y, before.y + 60.0);
    }
    assert_eq!(scene.selected_nodes, BTreeSet::from([a, b]));
}

#[test]
fn marquee_selects_visible_covers_without_disclosing_folder_children() {
    let mut fixture = Fixture::new();
    let scene = &mut fixture.scene;
    let root = scene.layout.roots[0];
    zoom_to(scene, root, 0.015);
    assert!(scene.begin_node_pointer(160.0, 160.0, WIDTH, HEIGHT, false));
    scene.update_marquee(225.0, 225.0, WIDTH, HEIGHT);
    assert_eq!(scene.selected_nodes, BTreeSet::from([root]));
    assert!(scene.marquee.as_ref().unwrap().moved);
    scene.end_node_pointer(WIDTH, HEIGHT);
    assert!(scene.folder_disclosures.is_empty());
    assert!(scene.is_summary(root));
    assert_eq!(scene.visible_nodes(scene.bounds), vec![root]);
}

#[test]
fn reverse_marquee_inside_folder_selects_intersecting_files_without_selecting_parent() {
    let mut fixture = Fixture::new();
    let card = fixture.card("a.rs");
    let scene = &mut fixture.scene;
    let node = scene.cards[card].node;
    let parent = scene.layout.nodes[node].parent.unwrap();
    zoom_to(scene, node, 0.3);
    let r = scene.layout.nodes[node].rect;
    let start = scene
        .camera
        .world_to_screen(r.right() + 10.0, r.bottom() + 10.0);
    let end = scene
        .camera
        .world_to_screen(r.x + r.width / 2.0, r.y + r.height / 2.0);
    assert!(scene.begin_node_pointer(start.0, start.1, WIDTH, HEIGHT, false));
    assert!(scene.is_marquee_selecting());
    scene.update_marquee(start.0 + 1.0, start.1 + 1.0, WIDTH, HEIGHT);
    assert!(scene.selected_nodes.is_empty());
    scene.update_marquee(end.0, end.1, WIDTH, HEIGHT);
    assert!(scene.selected_nodes.contains(&node));
    assert!(!scene.selected_nodes.contains(&parent));
    scene.end_node_pointer(WIDTH, HEIGHT);
    assert!(!scene.is_summary(node));
    assert!(scene.folder_disclosures.is_empty());
}

#[test]
fn additive_marquee_keeps_existing_selection_and_empty_click_clears_it() {
    let mut fixture = Fixture::new();
    let card = fixture.card("a.rs");
    let second = fixture.card("b.rs");
    let scene = &mut fixture.scene;
    let (node, second) = (scene.cards[card].node, scene.cards[second].node);
    zoom_to(scene, node, 0.3);
    click_node(scene, node, false);
    let rect = scene.layout.nodes[second].rect;
    let start = scene
        .camera
        .world_to_screen(rect.right() + 10.0, rect.bottom() + 10.0);
    let end = scene.camera.world_to_screen(rect.x + 30.0, rect.y + 30.0);
    assert!(scene.begin_node_pointer(start.0, start.1, WIDTH, HEIGHT, true));
    scene.update_marquee(end.0, end.1, WIDTH, HEIGHT);
    scene.end_node_pointer(WIDTH, HEIGHT);
    assert_eq!(scene.selected_nodes, BTreeSet::from([node, second]));
    assert!(scene.begin_node_pointer(10.0, TOP_BAR + 10.0, WIDTH, HEIGHT, false));
    scene.end_node_pointer(WIDTH, HEIGHT);
    assert!(scene.selected_nodes.is_empty());
}

#[test]
fn cancelled_or_dragged_folder_does_not_commit_a_disclosure() {
    let mut fixture = Fixture::new();
    let scene = &mut fixture.scene;
    let root = scene.layout.roots[0];
    zoom_to(scene, root, 0.015);
    let start = header_point(scene, root);
    scene.begin_node_pointer(start.0, start.1, WIDTH, HEIGHT, false);
    scene.finish_node_drag();
    assert!(scene.is_summary(root));
    scene.begin_node_pointer(start.0, start.1, WIDTH, HEIGHT, false);
    scene.update_node_drag(start.0 + 30.0, start.1 + 30.0);
    scene.end_node_pointer(WIDTH, HEIGHT);
    assert!(scene.is_summary(root));
    assert!(scene.folder_disclosures.is_empty());
}

#[test]
fn drilling_into_child_retains_parent_until_release_and_reveals_only_that_branch() {
    let mut fixture = Fixture::new();
    let scene = &mut fixture.scene;
    let root = scene.layout.roots[0];
    zoom_to(scene, root, 0.015);
    click_node(scene, root, false);
    scene.last_node_click = None;
    let folder = scene.layout.nodes[root]
        .children
        .iter()
        .copied()
        .find(|&id| scene.layout.nodes[id].is_folder && !scene.layout.nodes[id].children.is_empty())
        .unwrap();
    let point = header_point(scene, folder);
    scene.begin_node_pointer(point.0, point.1, WIDTH, HEIGHT, false);
    assert!(!scene.is_summary(root));
    assert!(scene.is_summary(folder));
    scene.end_node_pointer(WIDTH, HEIGHT);
    assert!(!scene.is_summary(root));
    assert!(!scene.is_summary(folder));
    assert_eq!(scene.selected_nodes, BTreeSet::from([folder]));
}
