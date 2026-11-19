//! Stable tree geometry shared by rendering, hit testing and pointer interaction.
//!
//! Rectangles are in world coordinates. A folder's frame is fitted around its
//! children without moving them; translating a folder moves its whole subtree.

use std::{cmp::Ordering, collections::BinaryHeap};

use crate::camera::Rect;

pub const FOLDER_PADDING: f64 = 32.0;
pub const FOLDER_HEADER: f64 = 64.0;
pub const NODE_GAP: f64 = 40.0;
const EMPTY_WIDTH: f64 = 240.0;
const EMPTY_HEIGHT: f64 = 128.0;
const CONTACT_EPSILON: f64 = 0.00001;

#[derive(Clone, Debug)]
pub struct LayoutNode {
    pub rect: Rect,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub is_folder: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub nodes: Vec<LayoutNode>,
    pub roots: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Direction {
    horizontal: bool,
    positive: bool,
}

impl Direction {
    fn start(self, rect: Rect) -> f64 {
        match (self.horizontal, self.positive) {
            (true, true) => rect.x,
            (true, false) => -rect.right(),
            (false, true) => rect.y,
            (false, false) => -rect.bottom(),
        }
    }

    fn end(self, rect: Rect) -> f64 {
        self.start(rect)
            + if self.horizontal {
                rect.width
            } else {
                rect.height
            }
    }

    fn delta(self, distance: f64) -> (f64, f64) {
        let signed = if self.positive { distance } else { -distance };
        if self.horizontal {
            (signed, 0.0)
        } else {
            (0.0, signed)
        }
    }
}

/// Motion over one pointer event. Only the active folder may resize; pushed
/// siblings translate rigidly. Velocities never persist into the next event.
#[derive(Clone, Copy)]
struct Motion {
    rect: Rect,
    velocity: Rect,
}

impl Motion {
    fn between(before: Rect, after: Rect) -> Self {
        Self {
            rect: before,
            velocity: Rect::new(
                after.x - before.x,
                after.y - before.y,
                after.width - before.width,
                after.height - before.height,
            ),
        }
    }

    fn advance(&mut self, time: f64) {
        self.rect.x += self.velocity.x * time;
        self.rect.y += self.velocity.y * time;
        self.rect.width += self.velocity.width * time;
        self.rect.height += self.velocity.height * time;
    }

    fn set_speed(&mut self, direction: Direction, speed: f64) {
        let (dx, dy) = direction.delta(speed);
        if direction.horizontal {
            self.velocity.x = dx;
        } else {
            self.velocity.y = dy;
        }
    }
}

#[derive(Clone, Copy)]
struct Contact {
    time: f64,
    direction: Direction,
}

struct ContactEvent {
    contact: Contact,
    a: usize,
    b: usize,
    revisions: (usize, usize),
}

impl PartialEq for ContactEvent {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for ContactEvent {}

impl PartialOrd for ContactEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ContactEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse time order so the earliest contact is popped first. Resolve
        // simultaneous contacts deterministically by the stable sibling order.
        other
            .contact
            .time
            .total_cmp(&self.contact.time)
            .then_with(|| other.a.cmp(&self.a))
            .then_with(|| other.b.cmp(&self.b))
            .then_with(|| other.revisions.cmp(&self.revisions))
    }
}

fn schedule_contacts(
    changed: usize,
    motions: &[Motion],
    revisions: &[usize],
    time: f64,
    events: &mut BinaryHeap<ContactEvent>,
) {
    for other in 0..motions.len() {
        if other == changed {
            continue;
        }
        let (a, b) = (changed.min(other), changed.max(other));
        if let Some(mut contact) = swept_contact(motions[a], motions[b], 1.0 - time) {
            contact.time += time;
            events.push(ContactEvent {
                contact,
                a,
                b,
                revisions: (revisions[a], revisions[b]),
            });
        }
    }
}

impl Layout {
    pub fn new() -> Self {
        Self::default()
    }

    /// IDs remain stable for the lifetime of the layout. Parents must be added first.
    pub fn add(&mut self, parent: Option<usize>, width: f64, height: f64) -> usize {
        assert!(parent.is_none_or(|id| id < self.nodes.len()));
        let id = self.nodes.len();
        self.nodes.push(LayoutNode {
            rect: Rect::new(0.0, 0.0, valid_size(width), valid_size(height)),
            parent,
            children: Vec::new(),
            is_folder: false,
        });
        if let Some(parent) = parent {
            self.nodes[parent].is_folder = true;
            self.nodes[parent].children.push(id);
        } else {
            self.roots.push(id);
        }
        id
    }

    pub fn add_folder(&mut self, parent: Option<usize>) -> usize {
        let id = self.add(parent, EMPTY_WIDTH, EMPTY_HEIGHT);
        self.nodes[id].is_folder = true;
        id
    }

    /// Pack measured children before their parents, as in the original viewer.
    /// Content dimensions are preserved, so this never causes text to reflow.
    pub fn arrange(&mut self) {
        for id in (0..self.nodes.len()).rev() {
            if !self.nodes[id].is_folder {
                continue;
            }
            let children = self.nodes[id].children.clone();
            self.pack(&children, FOLDER_PADDING, FOLDER_HEADER);
            if children.is_empty() {
                self.nodes[id].rect = Rect::new(0.0, 0.0, EMPTY_WIDTH, EMPTY_HEIGHT);
            } else {
                self.fit_folder(id);
            }
        }
        self.pack(&self.roots.clone(), 0.0, 0.0);
    }

    /// Arrange one folder's immediate children, retaining every child folder's
    /// internal arrangement and leaving unrelated nodes alone unless contacted.
    pub fn arrange_folder(&mut self, id: usize) {
        let Some(folder) = self.nodes.get(id) else {
            return;
        };
        if !folder.is_folder {
            return;
        }
        let rect = folder.rect;
        let children = folder.children.clone();
        self.pack(&children, rect.x + FOLDER_PADDING, rect.y + FOLDER_HEADER);
        let mut next = Some(id);
        while let Some(folder) = next {
            let before = self.nodes[folder].rect;
            self.fit_folder(folder);
            self.resolve_siblings(folder, before);
            next = self.nodes[folder].parent;
        }
    }

    fn pack(&mut self, ids: &[usize], left: f64, top: f64) {
        let area: f64 = ids
            .iter()
            .map(|&id| {
                let rect = self.nodes[id].rect;
                (rect.width + NODE_GAP) * (rect.height + NODE_GAP)
            })
            .sum();
        let target = (area.sqrt() * 1.25).max(800.0);
        let (mut x, mut y, mut row_height) = (left, top, 0.0_f64);
        for &id in ids {
            let rect = self.nodes[id].rect;
            if x > left && x + rect.width > left + target {
                x = left;
                y += row_height + NODE_GAP;
                row_height = 0.0;
            }
            self.translate(id, x - rect.x, y - rect.y);
            x += rect.width + NODE_GAP;
            row_height = row_height.max(rect.height);
        }
    }

    /// Move a card or folder, transferring only motion into the contacted edge.
    /// Swept contacts keep fast drags and collision chains from tunnelling, while
    /// tangential movement never sends a neighbour to the dragged card's corner.
    pub fn move_node(&mut self, id: usize, dx: f64, dy: f64) {
        if id >= self.nodes.len() || !dx.is_finite() || !dy.is_finite() || (dx == 0.0 && dy == 0.0)
        {
            return;
        }
        let before = self.nodes[id].rect;
        // Keep all geometry well inside the useful precision of Skia's f32 world.
        if (before.x + dx).abs() > 1.0e12 || (before.y + dy).abs() > 1.0e12 {
            return;
        }
        self.translate(id, dx, dy);
        self.resolve_siblings(id, before);
        let mut parent = self.nodes[id].parent;
        while let Some(id) = parent {
            let before = self.nodes[id].rect;
            self.fit_folder(id);
            self.resolve_siblings(id, before);
            parent = self.nodes[id].parent;
        }
    }

    /// Translate selected roots together; descendants of a selected folder move once.
    pub fn move_nodes(&mut self, ids: &[usize], dx: f64, dy: f64) {
        use std::collections::{BTreeMap, BTreeSet};
        let selected: BTreeSet<_> = ids
            .iter()
            .copied()
            .filter(|&id| id < self.nodes.len())
            .collect();
        let roots: Vec<_> = selected
            .iter()
            .copied()
            .filter(|&id| {
                let mut parent = self.nodes[id].parent;
                while let Some(id) = parent {
                    if selected.contains(&id) {
                        return false;
                    }
                    parent = self.nodes[id].parent;
                }
                true
            })
            .collect();
        if roots.len() == 1 {
            self.move_node(roots[0], dx, dy);
            return;
        }
        if roots.is_empty()
            || !dx.is_finite()
            || !dy.is_finite()
            || (dx == 0.0 && dy == 0.0)
            || roots.iter().any(|&id| {
                (self.nodes[id].rect.x + dx).abs() > 1.0e12
                    || (self.nodes[id].rect.y + dy).abs() > 1.0e12
            })
        {
            return;
        }
        let mut affected = BTreeSet::new();
        let mut before = BTreeMap::new();
        for &id in &roots {
            let mut current = Some(id);
            while let Some(node) = current {
                affected.insert(node);
                before.insert(node, self.nodes[node].rect);
                current = self.nodes[node].parent;
            }
        }
        let mut levels: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for id in affected {
            let mut depth = 0;
            let mut parent = self.nodes[id].parent;
            while let Some(node) = parent {
                depth += 1;
                parent = self.nodes[node].parent;
            }
            levels.entry(depth).or_default().push(id);
        }
        for id in roots {
            self.translate(id, dx, dy);
        }
        for (_, ids) in levels.into_iter().rev() {
            let mut groups: BTreeMap<Option<usize>, BTreeMap<usize, Rect>> = BTreeMap::new();
            for id in ids {
                if self.nodes[id].is_folder {
                    self.fit_folder(id);
                }
                groups
                    .entry(self.nodes[id].parent)
                    .or_default()
                    .insert(id, before[&id]);
            }
            for group in groups.values() {
                self.resolve_sibling_group(group);
            }
        }
    }

    fn translate(&mut self, id: usize, dx: f64, dy: f64) {
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            let node = &mut self.nodes[id];
            node.rect.x += dx;
            node.rect.y += dy;
            pending.extend(node.children.iter().copied());
        }
    }

    fn fit_folder(&mut self, id: usize) {
        let children = &self.nodes[id].children;
        if children.is_empty() {
            return;
        }
        let content = union(children.iter().map(|&child| self.nodes[child].rect));
        self.nodes[id].rect = Rect::new(
            content.x - FOLDER_PADDING,
            content.y - FOLDER_HEADER,
            (content.width + FOLDER_PADDING * 2.0).max(EMPTY_WIDTH),
            (content.height + FOLDER_HEADER + FOLDER_PADDING).max(EMPTY_HEIGHT),
        );
    }

    fn resolve_siblings(&mut self, active: usize, before: Rect) {
        self.resolve_sibling_group(&std::collections::BTreeMap::from([(active, before)]));
    }

    fn resolve_sibling_group(&mut self, active: &std::collections::BTreeMap<usize, Rect>) {
        if active
            .iter()
            .all(|(&id, &before)| self.nodes[id].rect == before)
        {
            return;
        }
        let first = *active.keys().next().unwrap();
        let siblings = match self.nodes[first].parent {
            Some(parent) => self.nodes[parent].children.clone(),
            None => self.roots.clone(),
        };
        if siblings.len() == 1 {
            return;
        }
        let mut motions: Vec<_> = siblings
            .iter()
            .map(|&id| {
                if let Some(&before) = active.get(&id) {
                    Motion::between(before, self.nodes[id].rect)
                } else {
                    Motion::between(self.nodes[id].rect, self.nodes[id].rect)
                }
            })
            .collect();
        let mut revisions = vec![0; siblings.len()];
        let mut events = BinaryHeap::new();
        let fixed: Vec<_> = siblings.iter().map(|id| active.contains_key(id)).collect();
        for (index, &is_fixed) in fixed.iter().enumerate() {
            if is_fixed {
                schedule_contacts(index, &motions, &revisions, 0.0, &mut events);
            }
        }
        let mut time = 0.0;
        while let Some(event) = events.pop() {
            let ContactEvent {
                contact,
                a,
                b,
                revisions: scheduled,
            } = event;
            if (fixed[a] && fixed[b]) || scheduled != (revisions[a], revisions[b]) {
                continue;
            }
            for motion in &mut motions {
                motion.advance((contact.time - time).max(0.0));
            }
            time = contact.time;
            let direction = contact.direction;
            let speed_a = direction.end(motions[a].velocity);
            let speed_b = direction.start(motions[b].velocity);
            // The active node is kinematic. Every other node inherits only the
            // normal component from the pushing edge, retaining its tangent.
            let changed = if fixed[a] || (!fixed[b] && speed_a.abs() >= speed_b.abs()) {
                motions[b].set_speed(direction, speed_a);
                b
            } else {
                motions[a].set_speed(direction, speed_b);
                a
            };
            revisions[changed] += 1;
            // Unchanged trajectories retain their queued contacts. Recheck only
            // the new motion, so a long touching chain costs O(n²), not O(n³).
            schedule_contacts(changed, &motions, &revisions, time, &mut events);
        }
        for (&id, mut motion) in siblings.iter().zip(motions) {
            if !active.contains_key(&id) {
                motion.advance(1.0 - time);
                let rect = self.nodes[id].rect;
                let (dx, dy) = (motion.rect.x - rect.x, motion.rect.y - rect.y);
                if dx != 0.0 || dy != 0.0 {
                    self.translate(id, dx, dy);
                }
            }
        }
    }

    pub fn bounds(&self) -> Rect {
        union(self.roots.iter().map(|&id| self.nodes[id].rect))
    }
}

fn valid_size(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(1.0, 1.0e10)
    } else {
        1.0
    }
}

fn union(rects: impl Iterator<Item = Rect>) -> Rect {
    rects
        .reduce(|a, b| {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            Rect::new(
                x,
                y,
                a.right().max(b.right()) - x,
                a.bottom().max(b.bottom()) - y,
            )
        })
        .unwrap_or(Rect::new(0.0, 0.0, 1.0, 1.0))
}

#[cfg(test)]
fn overlaps(a: Rect, b: Rect) -> bool {
    // Tolerate accumulated translation error at exact resting boundaries.
    a.x < b.right() + NODE_GAP - CONTACT_EPSILON
        && a.right() + NODE_GAP - CONTACT_EPSILON > b.x
        && a.y < b.bottom() + NODE_GAP - CONTACT_EPSILON
        && a.bottom() + NODE_GAP - CONTACT_EPSILON > b.y
}

fn swept_contact(a: Motion, b: Motion, duration: f64) -> Option<Contact> {
    let mut enter = f64::NEG_INFINITY;
    let mut exit = duration;
    let mut normal = None;
    // Intersect the four time intervals in which the expanded rectangles
    // overlap. The last entering edge is the contact normal, independently of
    // which component of the pointer movement happens to be larger.
    for horizontal in [true, false] {
        for positive in [true, false] {
            let direction = Direction {
                horizontal,
                positive,
            };
            let distance = direction.end(a.rect) + NODE_GAP - direction.start(b.rect);
            let distance = if distance.abs() < CONTACT_EPSILON {
                0.0
            } else {
                distance
            };
            let speed = direction.end(a.velocity) - direction.start(b.velocity);
            if speed > 0.0 {
                let time = -distance / speed;
                if time > enter {
                    enter = time;
                    normal = Some(direction);
                }
            } else if speed < 0.0 {
                exit = exit.min(-distance / speed);
            } else if distance <= 0.0 {
                return None;
            }
        }
    }
    (enter.max(0.0) < exit).then_some(Contact {
        time: enter.max(0.0),
        direction: normal?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_tree(layout: &Layout) {
        for (id, node) in layout.nodes.iter().enumerate() {
            assert!(node.rect.x.is_finite() && node.rect.y.is_finite());
            if let Some(parent) = node.parent {
                let frame = layout.nodes[parent].rect;
                assert!(
                    node.rect.x >= frame.x + FOLDER_PADDING - 0.0001,
                    "left containment {id}"
                );
                assert!(
                    node.rect.y >= frame.y + FOLDER_HEADER - 0.0001,
                    "top containment {id}"
                );
                assert!(
                    node.rect.right() <= frame.right() - FOLDER_PADDING + 0.0001,
                    "right containment {id}"
                );
                assert!(
                    node.rect.bottom() <= frame.bottom() - FOLDER_PADDING + 0.0001,
                    "bottom containment {id}"
                );
            }
            for (other_id, other) in layout.nodes.iter().enumerate().skip(id + 1) {
                if node.parent == other.parent {
                    assert!(
                        !overlaps(node.rect, other.rect),
                        "overlap {id}/{other_id}: {:?} {:?}",
                        node.rect,
                        other.rect
                    );
                }
            }
        }
    }

    #[test]
    fn measured_nested_layout_keeps_empty_folders_and_is_repeatable() {
        let mut layout = Layout::new();
        let root = layout.add_folder(None);
        let folder = layout.add_folder(Some(root));
        let empty = layout.add_folder(Some(root));
        layout.add(Some(folder), 720.0, 9000.0);
        layout.add(Some(folder), 720.0, 130.0);
        layout.add(Some(root), 720.0, 400.0);
        layout.arrange();
        assert_tree(&layout);
        assert_eq!(layout.nodes[empty].rect.width, EMPTY_WIDTH);
        let before: Vec<_> = layout.nodes.iter().map(|node| node.rect).collect();
        layout.arrange();
        assert_eq!(
            before,
            layout
                .nodes
                .iter()
                .map(|node| node.rect)
                .collect::<Vec<_>>()
        );
        assert_eq!(layout.bounds(), layout.nodes[root].rect);
    }

    #[test]
    fn folder_moves_descendants_exactly_once_and_preserves_other_world_positions() {
        let mut layout = Layout::new();
        let root = layout.add_folder(None);
        let folder = layout.add_folder(Some(root));
        let child = layout.add(Some(folder), 720.0, 800.0);
        let sibling = layout.add(Some(root), 720.0, 300.0);
        layout.arrange();
        let before = layout.nodes[child].rect;
        let sibling_before = layout.nodes[sibling].rect;
        layout.move_node(folder, -1000.0, -900.0);
        assert_eq!(layout.nodes[child].rect.x, before.x - 1000.0);
        assert_eq!(layout.nodes[child].rect.y, before.y - 900.0);
        assert_eq!(layout.nodes[sibling].rect, sibling_before);
        assert!(layout.bounds().x < 0.0);
        assert_tree(&layout);
    }

    #[test]
    fn collision_chain_pushes_in_both_directions_and_keeps_dragged_node_fixed() {
        for sign in [-1.0, 1.0] {
            let mut layout = Layout::new();
            let ids: Vec<_> = (0..5).map(|_| layout.add(None, 100.0, 100.0)).collect();
            for (index, &id) in ids.iter().enumerate() {
                layout.nodes[id].rect.x = index as f64 * 140.0;
            }
            let active = if sign > 0.0 { ids[0] } else { ids[4] };
            let x = layout.nodes[active].rect.x;
            layout.move_node(active, 70.0 * sign, 0.0);
            assert_eq!(layout.nodes[active].rect.x, x + 70.0 * sign);
            assert_tree(&layout);
            let far = if sign > 0.0 { ids[4] } else { ids[0] };
            assert_eq!(
                layout.nodes[far].rect.x,
                if sign > 0.0 { 630.0 } else { -70.0 }
            );
        }
    }

    #[test]
    fn sliding_against_a_side_never_snaps_a_tall_card_to_a_corner() {
        for sign in [-1.0, 1.0] {
            let mut layout = Layout::new();
            let a = layout.add(None, 100.0, 9000.0);
            let b = layout.add(None, 100.0, 2000.0);
            layout.nodes[b].rect.x = 140.0 * sign;
            layout.nodes[b].rect.y = 500.0;
            for (dx, dy) in [(2.0, 3.0), (3.0, -4.0), (1.0, 8.0)] {
                let before = layout.nodes[b].rect;
                layout.move_node(a, dx * sign, dy);
                assert_eq!(layout.nodes[b].rect.x, before.x + dx * sign);
                assert_eq!(layout.nodes[b].rect.y, before.y);
                assert_tree(&layout);
            }
            let before = layout.nodes[b].rect;
            layout.move_node(a, 0.0, 100.0);
            layout.move_node(a, -10.0 * sign, -20.0);
            assert_eq!(layout.nodes[b].rect, before);
        }
    }

    #[test]
    fn sliding_against_a_horizontal_edge_preserves_the_other_cards_x() {
        for sign in [-1.0, 1.0] {
            let mut layout = Layout::new();
            let a = layout.add(None, 2000.0, 100.0);
            let b = layout.add(None, 1000.0, 100.0);
            layout.nodes[b].rect.x = 500.0;
            layout.nodes[b].rect.y = 140.0 * sign;
            let before = layout.nodes[b].rect;
            layout.move_node(a, 8.0, 2.0 * sign);
            assert_eq!(layout.nodes[b].rect.x, before.x);
            assert_eq!(layout.nodes[b].rect.y, before.y + 2.0 * sign);
            assert_tree(&layout);
        }
    }

    #[test]
    fn diagonal_motion_pushes_each_contact_along_its_own_normal() {
        let mut layout = Layout::new();
        let a = layout.add(None, 100.0, 100.0);
        let right = layout.add(None, 100.0, 100.0);
        let below = layout.add(None, 100.0, 100.0);
        layout.nodes[right].rect.x = 140.0;
        layout.nodes[below].rect.y = 140.0;
        layout.move_node(a, 3.0, 4.0);
        assert_eq!(
            layout.nodes[right].rect,
            Rect::new(143.0, 0.0, 100.0, 100.0)
        );
        assert_eq!(
            layout.nodes[below].rect,
            Rect::new(0.0, 144.0, 100.0, 100.0)
        );
        assert_tree(&layout);
    }

    #[test]
    fn a_fast_diagonal_hit_uses_the_entry_edge_and_only_the_remaining_motion() {
        let mut layout = Layout::new();
        let a = layout.add(None, 100.0, 2000.0);
        let b = layout.add(None, 100.0, 100.0);
        layout.nodes[b].rect = Rect::new(200.0, 500.0, 100.0, 100.0);
        layout.move_node(a, 300.0, 400.0);
        assert_eq!(layout.nodes[a].rect, Rect::new(300.0, 400.0, 100.0, 2000.0));
        assert_eq!(layout.nodes[b].rect, Rect::new(440.0, 500.0, 100.0, 100.0));
        assert_tree(&layout);
    }

    #[test]
    fn a_pushed_cards_sweep_reaches_obstacles_outside_the_active_cards_path() {
        let mut layout = Layout::new();
        let a = layout.add(None, 100.0, 100.0);
        let b = layout.add(None, 100.0, 1000.0);
        let c = layout.add(None, 100.0, 100.0);
        layout.nodes[b].rect.x = 200.0;
        layout.nodes[c].rect.x = 400.0;
        layout.nodes[c].rect.y = 800.0;
        layout.move_node(a, 1000.0, 0.0);
        assert_eq!(layout.nodes[b].rect.x, 1140.0);
        assert_eq!(layout.nodes[c].rect.x, 1280.0);
        assert_eq!(layout.nodes[c].rect.y, 800.0);
        assert_tree(&layout);
    }

    #[test]
    fn folder_expansion_pushes_at_the_contacted_edge_and_preserves_its_subtree() {
        let mut layout = Layout::new();
        let a = layout.add_folder(None);
        let child_a = layout.add(Some(a), 100.0, 9000.0);
        let b = layout.add_folder(None);
        let child_b = layout.add(Some(b), 100.0, 100.0);
        layout.arrange();
        let a_rect = layout.nodes[a].rect;
        let b_rect = layout.nodes[b].rect;
        layout.translate(b, a_rect.right() + NODE_GAP - b_rect.x, a_rect.y - b_rect.y);
        let before_b = layout.nodes[b].rect;
        let before_child_b = layout.nodes[child_b].rect;
        layout.move_node(child_a, 2.0, 3.0);
        assert_eq!(layout.nodes[b].rect.x, before_b.x + 2.0);
        assert_eq!(layout.nodes[b].rect.y, before_b.y);
        assert_eq!(layout.nodes[child_b].rect.x, before_child_b.x + 2.0);
        assert_eq!(layout.nodes[child_b].rect.y, before_child_b.y);
        assert_tree(&layout);
    }

    #[test]
    fn fast_push_propagates_through_a_chain_without_skipping_distant_cards() {
        let mut layout = Layout::new();
        let a = layout.add(None, 100.0, 100.0);
        let b = layout.add(None, 100.0, 100.0);
        let c = layout.add(None, 100.0, 100.0);
        layout.nodes[b].rect.x = 200.0;
        layout.nodes[c].rect.x = 400.0;
        layout.move_node(a, 1000.0, 0.0);
        assert_eq!(layout.nodes[b].rect.x, 1140.0);
        assert_eq!(layout.nodes[c].rect.x, 1280.0);
        assert_tree(&layout);
    }

    #[test]
    fn fast_pointer_motion_pushes_cards_crossed_between_events() {
        let mut layout = Layout::new();
        let a = layout.add(None, 100.0, 100.0);
        let b = layout.add(None, 100.0, 100.0);
        layout.nodes[b].rect.x = 200.0;
        layout.move_node(a, 1000.0, 0.0);
        assert_eq!(layout.nodes[a].rect.x, 1000.0);
        assert_eq!(layout.nodes[b].rect.x, 1140.0);
        assert_tree(&layout);
    }

    #[test]
    fn expanding_nested_folder_pushes_ancestor_siblings_with_their_children() {
        let mut layout = Layout::new();
        let root = layout.add_folder(None);
        let a = layout.add_folder(Some(root));
        let b = layout.add_folder(Some(root));
        let child_a = layout.add(Some(a), 100.0, 100.0);
        let child_b = layout.add(Some(b), 100.0, 100.0);
        layout.arrange();
        let before_b = layout.nodes[child_b].rect;
        layout.move_node(child_a, 300.0, 0.0);
        assert!(layout.nodes[child_b].rect.x > before_b.x);
        assert_tree(&layout);
    }

    #[test]
    fn many_moves_preserve_sibling_spacing_and_parent_containment() {
        let mut layout = Layout::new();
        let root = layout.add_folder(None);
        for group in 0..5 {
            let folder = layout.add_folder(Some(root));
            for item in 0..7 {
                layout.add(
                    Some(folder),
                    180.0 + item as f64 * 13.0,
                    100.0 + group as f64 * 61.0,
                );
            }
        }
        layout.arrange();
        let mut random = 317_u64;
        for _ in 0..120 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let id = random as usize % (layout.nodes.len() - 1) + 1;
            let dx = ((random >> 12) % 2000) as f64 - 1000.0;
            let dy = ((random >> 32) % 1400) as f64 - 700.0;
            layout.move_node(id, dx, dy);
            assert_tree(&layout);
        }
    }

    #[test]
    fn invalid_motion_does_not_corrupt_geometry() {
        let mut layout = Layout::new();
        let id = layout.add(None, 100.0, 100.0);
        let rect = layout.nodes[id].rect;
        layout.move_node(id, f64::NAN, 0.0);
        layout.move_node(id, 0.0, f64::INFINITY);
        layout.move_node(id, 1.0e20, 0.0);
        layout.move_node(999, 100.0, 100.0);
        assert_eq!(layout.nodes[id].rect, rect);
        assert_eq!(Layout::new().bounds(), Rect::new(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn arranging_one_folder_preserves_child_folder_internal_positions() {
        let mut layout = Layout::new();
        let root = layout.add_folder(None);
        let nested = layout.add_folder(Some(root));
        let a = layout.add(Some(nested), 100.0, 100.0);
        let b = layout.add(Some(nested), 100.0, 100.0);
        layout.add(Some(root), 100.0, 200.0);
        layout.arrange();
        layout.move_node(a, -300.0, -100.0);
        let relative = (
            layout.nodes[b].rect.x - layout.nodes[a].rect.x,
            layout.nodes[b].rect.y - layout.nodes[a].rect.y,
        );
        layout.arrange_folder(root);
        assert_eq!(
            relative,
            (
                layout.nodes[b].rect.x - layout.nodes[a].rect.x,
                layout.nodes[b].rect.y - layout.nodes[a].rect.y,
            )
        );
        assert_tree(&layout);
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;

    #[test]
    fn moving_selected_siblings_preserves_offsets_and_pushes_unselected_chain() {
        let mut layout = Layout::new();
        let a = layout.add(None, 100.0, 100.0);
        let b = layout.add(None, 100.0, 100.0);
        let c = layout.add(None, 100.0, 100.0);
        let d = layout.add(None, 100.0, 100.0);
        for (i, node) in layout.nodes.iter_mut().enumerate() {
            node.rect.x = i as f64 * 140.0;
        }
        layout.move_nodes(&[a, b], 300.0, 0.0);
        assert_eq!(layout.nodes[a].rect.x, 300.0);
        assert_eq!(layout.nodes[b].rect.x, 440.0);
        assert!(layout.nodes[c].rect.x >= 580.0);
        assert!(layout.nodes[d].rect.x >= layout.nodes[c].rect.right() + NODE_GAP);
    }

    #[test]
    fn parent_and_child_selection_translates_each_descendant_once() {
        let mut layout = Layout::new();
        let parent = layout.add_folder(None);
        let child = layout.add(Some(parent), 100.0, 100.0);
        let other = layout.add(None, 100.0, 100.0);
        layout.arrange();
        let before: Vec<_> = layout.nodes.iter().map(|n| n.rect).collect();
        layout.move_nodes(&[parent, child, other], 50.0, -30.0);
        for (node, rect) in layout.nodes.iter().zip(before) {
            assert_eq!(node.rect.x, rect.x + 50.0);
            assert_eq!(node.rect.y, rect.y - 30.0);
        }
    }
}
