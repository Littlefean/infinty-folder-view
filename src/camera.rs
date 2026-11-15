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

// Project Graph's 0.11 per-tick interpolation at 120 Hz, converted to exponential
// decay per second: -ln(1 - 0.11) * 120. This keeps the response independent of FPS.
const SMOOTHING_RATE: f64 = 13.984;
const POSITION_EPSILON: f64 = 0.05;
const RELATIVE_SCALE_EPSILON: f64 = 0.00001;

#[derive(Clone, Copy, Debug)]
pub struct CameraMotion {
    target: Camera,
    active: bool,
}

impl CameraMotion {
    pub fn new(camera: Camera) -> Self {
        Self {
            target: if Self::valid_camera(&camera) {
                camera
            } else {
                Camera::default()
            },
            active: false,
        }
    }

    #[cfg(test)]
    pub fn target(&self) -> Camera {
        self.target
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn zoom_at(&mut self, current: &Camera, x: f64, y: f64, factor: f64) {
        if !Self::valid_camera(current)
            || !x.is_finite()
            || !y.is_finite()
            || !factor.is_finite()
            || factor <= 0.0
        {
            return;
        }
        let (world_x, world_y) = current.screen_to_world(x, y);
        let scale = (self.target.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let target = Camera {
            x: x - world_x * scale,
            y: y - world_y * scale,
            scale,
        };
        if Self::valid_camera(&target) {
            self.target = target;
            self.active = current != &target;
        }
    }

    pub fn move_to(&mut self, mut target: Camera) {
        if Self::valid_camera(&target) {
            target.scale = target.scale.clamp(MIN_SCALE, MAX_SCALE);
            self.target = target;
            self.active = true;
        }
    }

    pub fn pan_by(&mut self, current: &Camera, dx: f64, dy: f64) {
        if !Self::valid_camera(current) || !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let base = if self.active { self.target } else { *current };
        let target = Camera {
            x: base.x + dx,
            y: base.y + dy,
            scale: base.scale,
        };
        if Self::valid_camera(&target) {
            self.target = target;
            self.active = current != &target;
        }
    }

    pub fn cancel(&mut self, current: Camera) {
        if Self::valid_camera(&current) {
            self.target = current;
            self.active = false;
        }
    }

    pub fn advance(&mut self, current: &mut Camera, dt_seconds: f64) -> bool {
        if !self.active
            || !Self::valid_camera(current)
            || !dt_seconds.is_finite()
            || dt_seconds <= 0.0
        {
            return self.active;
        }
        let alpha = -(-SMOOTHING_RATE * dt_seconds).exp_m1();
        // Using the same alpha for scale and offsets preserves the cursor's world
        // point throughout an anchored zoom, not only at its destination.
        let blend = |value: f64, target: f64| value * (1.0 - alpha) + target * alpha;
        let next = Camera {
            x: blend(current.x, self.target.x),
            y: blend(current.y, self.target.y),
            scale: blend(current.scale, self.target.scale),
        };
        if !Self::valid_camera(&next) {
            return self.active;
        }
        *current = next;
        let position_error = (current.x - self.target.x).hypot(current.y - self.target.y);
        let scale_error =
            (current.scale - self.target.scale).abs() / current.scale.max(self.target.scale);
        if position_error < POSITION_EPSILON && scale_error < RELATIVE_SCALE_EPSILON {
            *current = self.target;
            self.active = false;
        }
        self.active
    }

    fn valid_camera(camera: &Camera) -> bool {
        camera.x.is_finite()
            && camera.y.is_finite()
            && camera.scale.is_finite()
            && camera.scale > 0.0
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

    #[test]
    fn smoothing_matches_at_sixty_and_one_twenty_hz() {
        let mut at_sixty = Camera::default();
        let mut at_one_twenty = at_sixty;
        let mut sixty_motion = CameraMotion::new(at_sixty);
        let mut one_twenty_motion = CameraMotion::new(at_one_twenty);
        sixty_motion.zoom_at(&at_sixty, 730.0, 415.0, 2.0);
        one_twenty_motion.zoom_at(&at_one_twenty, 730.0, 415.0, 2.0);
        for _ in 0..6 {
            assert!(sixty_motion.advance(&mut at_sixty, 1.0 / 60.0));
        }
        for _ in 0..12 {
            assert!(one_twenty_motion.advance(&mut at_one_twenty, 1.0 / 120.0));
        }
        near(at_sixty.x, at_one_twenty.x);
        near(at_sixty.y, at_one_twenty.y);
        near(at_sixty.scale, at_one_twenty.scale);
    }

    #[test]
    fn animated_zoom_keeps_cursor_anchor_on_every_frame() {
        let mut camera = Camera::default();
        let mut motion = CameraMotion::new(camera);
        let anchor = camera.screen_to_world(836.0, 392.0);
        for factor in [2.4, 0.001, 1.0e20] {
            motion.zoom_at(&camera, 836.0, 392.0, factor);
            for _ in 0..200 {
                let active = motion.advance(&mut camera, 1.0 / 120.0);
                let screen = camera.world_to_screen(anchor.0, anchor.1);
                near(screen.0, 836.0);
                near(screen.1, 392.0);
                if !active {
                    break;
                }
            }
            assert!(!motion.is_active());
            assert_eq!(camera, motion.target());
        }
    }

    #[test]
    fn wheel_events_accumulate_and_reverse_target_without_jumping_current() {
        let mut camera = Camera::default();
        let initial = camera;
        let mut motion = CameraMotion::new(camera);
        motion.zoom_at(&camera, 400.0, 300.0, 1.2);
        motion.zoom_at(&camera, 400.0, 300.0, 1.2);
        near(motion.target().scale, initial.scale * 1.2 * 1.2);
        assert_eq!(camera, initial);
        motion.advance(&mut camera, 1.0 / 120.0);
        let before_reverse = camera;
        motion.zoom_at(&camera, 400.0, 300.0, 1.0 / 1.2);
        motion.zoom_at(&camera, 400.0, 300.0, 1.0 / 1.2);
        near(motion.target().scale, initial.scale);
        near(motion.target().x, initial.x);
        near(motion.target().y, initial.y);
        assert_eq!(camera, before_reverse);
    }

    #[test]
    fn changing_zoom_cursor_reanchors_without_a_position_jump() {
        let mut camera = Camera::default();
        let mut motion = CameraMotion::new(camera);
        motion.zoom_at(&camera, 300.0, 220.0, 1.4);
        motion.advance(&mut camera, 1.0 / 60.0);
        let before_reanchor = camera;
        let anchor = camera.screen_to_world(1050.0, 620.0);
        motion.zoom_at(&camera, 1050.0, 620.0, 1.3);
        assert_eq!(camera, before_reanchor);
        near(motion.target().scale, Camera::default().scale * 1.4 * 1.3);
        for _ in 0..20 {
            motion.advance(&mut camera, 1.0 / 120.0);
            let screen = camera.world_to_screen(anchor.0, anchor.1);
            near(screen.0, 1050.0);
            near(screen.1, 620.0);
        }
    }

    #[test]
    fn pending_pan_accumulates_and_cancel_stops_exactly() {
        let mut camera = Camera::default();
        let mut motion = CameraMotion::new(camera);
        motion.pan_by(&camera, 160.0, -40.0);
        motion.advance(&mut camera, 1.0 / 120.0);
        motion.pan_by(&camera, 160.0, -40.0);
        near(motion.target().x, Camera::default().x + 320.0);
        near(motion.target().y, Camera::default().y - 80.0);
        motion.cancel(camera);
        let stopped = camera;
        assert!(!motion.is_active());
        assert!(!motion.advance(&mut camera, 1.0));
        assert_eq!(camera, stopped);
        assert_eq!(motion.target(), stopped);
    }

    #[test]
    fn move_to_converges_and_snaps_to_exact_target() {
        let mut camera = Camera::default();
        let mut motion = CameraMotion::new(camera);
        let target = Camera {
            x: -9120.0,
            y: 1800.0,
            scale: 0.025,
        };
        motion.move_to(target);
        for _ in 0..240 {
            if !motion.advance(&mut camera, 1.0 / 120.0) {
                break;
            }
        }
        assert_eq!(camera, target);
        assert!(!motion.is_active());
        motion.move_to(Camera {
            scale: 100.0,
            ..target
        });
        assert_eq!(motion.target().scale, MAX_SCALE);
    }

    #[test]
    fn motion_rejects_invalid_events_and_elapsed_time() {
        let mut camera = Camera::default();
        let mut motion = CameraMotion::new(camera);
        motion.zoom_at(&camera, 400.0, 300.0, 1.4);
        let target = motion.target();
        motion.zoom_at(&camera, 400.0, 300.0, f64::NAN);
        motion.zoom_at(&camera, 400.0, 300.0, -1.0);
        motion.zoom_at(&camera, f64::INFINITY, 300.0, 1.2);
        motion.pan_by(&camera, f64::INFINITY, 0.0);
        motion.move_to(Camera {
            scale: 0.0,
            ..camera
        });
        motion.cancel(Camera {
            x: f64::NAN,
            ..camera
        });
        assert_eq!(motion.target(), target);
        let before = camera;
        for dt in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(motion.advance(&mut camera, dt));
            assert_eq!(camera, before);
        }
        assert_eq!(motion.target(), target);
        assert!(motion.is_active());
        assert_eq!(
            CameraMotion::new(Camera {
                scale: f64::NAN,
                ..camera
            })
            .target(),
            Camera::default()
        );
    }
}
