pub const MIN_SCALE: f64 = 0.015;
pub const MAX_SCALE: f64 = 4.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.is_valid()
            && other.is_valid()
            && self.x <= other.right()
            && self.right() >= other.x
            && self.y <= other.bottom()
            && self.bottom() >= other.y
    }

    pub fn expanded(&self, margin: f64) -> Self {
        if !margin.is_finite() || margin < 0.0 {
            return *self;
        }
        Self::new(
            self.x - margin,
            self.y - margin,
            self.width + margin * 2.0,
            self.height + margin * 2.0,
        )
    }

    fn is_valid(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width >= 0.0
            && self.height >= 0.0
            && self.right().is_finite()
            && self.bottom().is_finite()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            x: 64.0,
            y: 120.0,
            scale: 0.65,
        }
    }
}

impl Camera {
    pub fn world_to_screen(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.scale + self.x, y * self.scale + self.y)
    }

    pub fn screen_to_world(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.x) / self.scale, (y - self.y) / self.scale)
    }

    pub fn pan(&mut self, dx: f64, dy: f64) {
        let (x, y) = (self.x + dx, self.y + dy);
        if x.is_finite() && y.is_finite() {
            self.x = x;
            self.y = y;
        }
    }

    pub fn zoom_at(&mut self, cursor_x: f64, cursor_y: f64, factor: f64) {
        if !cursor_x.is_finite()
            || !cursor_y.is_finite()
            || !factor.is_finite()
            || factor <= 0.0
            || !self.scale.is_finite()
            || self.scale <= 0.0
        {
            return;
        }
        let (world_x, world_y) = self.screen_to_world(cursor_x, cursor_y);
        let scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let (x, y) = (cursor_x - world_x * scale, cursor_y - world_y * scale);
        if x.is_finite() && y.is_finite() {
            *self = Self { x, y, scale };
        }
    }

    pub fn visible_rect(&self, width: f64, height: f64) -> Rect {
        if !width.is_finite()
            || !height.is_finite()
            || width < 0.0
            || height < 0.0
            || !self.scale.is_finite()
            || self.scale <= 0.0
        {
            return Rect::default();
        }
        let (x, y) = self.screen_to_world(0.0, 0.0);
        Rect::new(x, y, width / self.scale, height / self.scale)
    }

    pub fn fit(&mut self, bounds: Rect, width: f64, height: f64, top_overlay: f64) {
        if !bounds.is_valid()
            || !width.is_finite()
            || !height.is_finite()
            || !top_overlay.is_finite()
        {
            return;
        }
        let padding = 32.0;
        let top = top_overlay.max(0.0) + padding;
        let available_width = width - padding * 2.0;
        let available_height = height - top - padding;
        if available_width <= 0.0 || available_height <= 0.0 {
            return;
        }
        let scale = (available_width / bounds.width.max(1.0))
            .min(available_height / bounds.height.max(1.0))
            .clamp(MIN_SCALE, MAX_SCALE);
        let x = padding + (available_width - bounds.width * scale) / 2.0 - bounds.x * scale;
        let y = top + (available_height - bounds.height * scale) / 2.0 - bounds.y * scale;
        if x.is_finite() && y.is_finite() {
            *self = Self { x, y, scale };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1.0e-8, "{actual} != {expected}");
    }

    #[test]
    fn screen_world_round_trip_after_pan_and_zoom() {
        let mut camera = Camera::default();
        camera.pan(-142.0, 37.0);
        camera.zoom_at(730.0, 415.0, 1.8);
        let screen = camera.world_to_screen(-5100.0, 9260.0);
        let world = camera.screen_to_world(screen.0, screen.1);
        near(world.0, -5100.0);
        near(world.1, 9260.0);
    }

    #[test]
    fn zoom_keeps_cursor_world_position_anchored_even_at_limits() {
        let mut camera = Camera::default();
        let before = camera.screen_to_world(836.0, 392.0);
        for factor in [1.14, 0.7, 1.0e20, 1.0e-20] {
            camera.zoom_at(836.0, 392.0, factor);
            let after = camera.screen_to_world(836.0, 392.0);
            near(after.0, before.0);
            near(after.1, before.1);
            assert!((MIN_SCALE..=MAX_SCALE).contains(&camera.scale));
        }
    }

    #[test]
    fn equal_and_opposite_panning_returns_to_start() {
        let mut camera = Camera::default();
        let before = camera;
        camera.pan(-5000.0, 3200.0);
        camera.pan(5000.0, -3200.0);
        assert_eq!(camera, before);
    }

    #[test]
    fn visible_world_rectangle_matches_screen_corners() {
        let camera = Camera {
            x: -120.0,
            y: 80.0,
            scale: 0.5,
        };
        let rect = camera.visible_rect(1000.0, 700.0);
        assert_eq!(rect, Rect::new(240.0, -160.0, 2000.0, 1400.0));
        assert!(rect.intersects(&Rect::new(300.0, -150.0, 400.0, 200.0)));
        assert!(!rect.intersects(&Rect::new(3000.0, 2000.0, 400.0, 200.0)));
    }

    #[test]
    fn touching_edges_are_included_to_prevent_culling_flicker() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(rect.intersects(&Rect::new(100.0, 50.0, 30.0, 20.0)));
        assert!(!rect.intersects(&Rect::new(100.01, 50.0, 30.0, 20.0)));
        assert!(!rect.intersects(&Rect::new(f64::NAN, 0.0, 30.0, 20.0)));
    }

    #[test]
    fn fit_centers_content_in_area_below_overlay() {
        let mut camera = Camera::default();
        let bounds = Rect::new(-1000.0, 1000.0, 4000.0, 2500.0);
        camera.fit(bounds, 1200.0, 800.0, 80.0);
        let top_left = camera.world_to_screen(bounds.x, bounds.y);
        let bottom_right = camera.world_to_screen(bounds.right(), bounds.bottom());
        assert!(top_left.0 >= 32.0 && top_left.1 >= 112.0);
        assert!(bottom_right.0 <= 1168.0 && bottom_right.1 <= 768.0);
        near((top_left.0 + bottom_right.0) / 2.0, 600.0);
        near((top_left.1 + bottom_right.1) / 2.0, 440.0);
    }

    #[test]
    fn invalid_input_does_not_poison_camera() {
        let mut camera = Camera::default();
        let before = camera;
        camera.pan(f64::INFINITY, 1.0);
        camera.pan(1.0, f64::NAN);
        camera.zoom_at(0.0, 0.0, 0.0);
        camera.zoom_at(0.0, 0.0, -1.0);
        camera.zoom_at(0.0, 0.0, f64::INFINITY);
        camera.zoom_at(f64::NAN, 0.0, 2.0);
        camera.fit(Rect::new(0.0, 0.0, 100.0, 100.0), 0.0, 0.0, 90.0);
        assert_eq!(camera, before);
    }
}
