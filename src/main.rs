mod animation_check;
mod camera;
mod documents;
mod markdown;
mod renderer;
mod scene;

use animation_check::AnimationCheck;
use camera::{Camera, CameraMotion};
use objc2::rc::autoreleasepool;
use renderer::Renderer;
use scene::{Scene, TOP_BAR};
use std::{error::Error, path::PathBuf, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, WindowId},
};

struct Options {
    directory: PathBuf,
    demo: bool,
    count: usize,
    smoke: Option<PathBuf>,
    animation_check: Option<PathBuf>,
}

impl Options {
    fn parse() -> Result<Option<Self>, Box<dyn Error>> {
        let mut options = Self {
            directory: std::env::var_os("MARKDOWN_ATLAS_DEMO_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demo-markdown")),
            demo: true,
            count: 72,
            smoke: None,
            animation_check: None,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--dir" => {
                    options.directory =
                        args.next().ok_or("--dir needs a file or directory")?.into();
                    options.demo = false;
                }
                "--demo-count" => {
                    options.count = args.next().ok_or("--demo-count needs a number")?.parse()?;
                    if !(1..=2000).contains(&options.count) {
                        return Err("--demo-count must be between 1 and 2000".into());
                    }
                }
                "--smoke-test" => {
                    options.smoke = Some(
                        args.next()
                            .ok_or("--smoke-test needs an output directory")?
                            .into(),
                    )
                }
                "--animation-test" => {
                    options.animation_check = Some(
                        args.next()
                            .ok_or("--animation-test needs an output directory")?
                            .into(),
                    )
                }
                "--help" | "-h" => {
                    println!(
                        "Markdown Atlas — native Rust + Skia/SkParagraph + Metal\n\n  cargo run --release\n  cargo run --release -- --dir /path/to/markdown\n  cargo run --release -- --demo-count 200\n  cargo run --release -- --smoke-test work/smoke\n  cargo run --release -- --animation-test work/animation\n\nControls: middle drag / Space+left drag to pan; wheel to smoothly zoom at cursor; F fit all; 1 actual size; R reset; L toggle overview detail. Click minimap to navigate. Drop a Markdown file or folder to load it.\n\nDefault: generate 72 real Markdown files in demo-markdown/ without overwriting existing files. macOS / Metal required."
                    );
                    return Ok(None);
                }
                _ => return Err(format!("Unknown argument {arg}; use --help").into()),
            }
        }
        if options.smoke.is_some() && options.animation_check.is_some() {
            return Err("Choose either --smoke-test or --animation-test".into());
        }
        Ok(Some(options))
    }
}

struct Smoke {
    directory: PathBuf,
    frame: usize,
    timings: Vec<f64>,
    intervals: Vec<f64>,
    previous: Option<Instant>,
}
struct App {
    renderer: Option<Renderer>,
    scene: Scene,
    cursor: (f64, f64),
    middle_down: bool,
    left_down: bool,
    space_down: bool,
    modifiers: ModifiersState,
    smoke: Option<Smoke>,
    error: Option<String>,
    occluded: bool,
    retry_draw: bool,
    motion: CameraMotion,
    animation_time: Option<Instant>,
    animation_check: Option<AnimationCheck>,
}

impl App {
    fn wait_for_event(&self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(
            self.animation_check
                .as_ref()
                .and_then(AnimationCheck::deadline)
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl std::fmt::Display) {
        let message = error.to_string();
        eprintln!("{message}");
        self.error = Some(message);
        event_loop.exit();
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        if self.occluded {
            self.animation_time = None;
            return;
        }
        // Layout happens once per file. Limiting it to one file per redraw keeps startup interactive.
        if !self.scene.ready() {
            self.scene.build_next();
        }
        let ready = self.scene.ready();
        let (width, height) = renderer.logical_size();
        if ready
            && let Some(check) = &mut self.animation_check
            && !check.started()
        {
            let anchor = (width * 0.6, height * 0.45);
            check.begin(self.scene.camera, anchor);
            // Exactly one input impulse. Subsequent frames must come from the animation loop.
            self.motion
                .zoom_at(&self.scene.camera, anchor.0, anchor.1, 0.55);
            self.animation_time = Some(Instant::now());
            event_loop.set_control_flow(ControlFlow::WaitUntil(check.deadline().unwrap()));
        }
        let mut snapshot = None;
        if ready && let Some(smoke) = &mut self.smoke {
            match smoke.frame {
                0 => {
                    self.scene.camera = Camera::default();
                    snapshot = Some(smoke.directory.join("01-reading.png"));
                }
                1..=59 => {
                    self.scene.camera.zoom_at(width * 0.6, height * 0.45, 0.977);
                    self.scene.camera.pan(-2.0, -3.0);
                }
                60 => {
                    self.scene
                        .camera
                        .fit(self.scene.bounds, width, height - 38.0, TOP_BAR);
                    snapshot = Some(smoke.directory.join("02-overview.png"));
                }
                61..=119 => {
                    self.scene.camera.zoom_at(width * 0.5, height * 0.5, 1.025);
                    self.scene.camera.pan(1.0, -1.0);
                }
                120 => {
                    self.scene.camera = Camera {
                        x: 40.0,
                        y: -1350.0,
                        scale: 0.95,
                    };
                    snapshot = Some(smoke.directory.join("03-code-and-tables.png"));
                }
                _ => {
                    self.scene.camera.zoom_at(width * 0.5, height * 0.45, 1.003);
                    self.scene.camera.pan(-0.5, -2.0);
                }
            }
        }
        let started = Instant::now();
        let result = autoreleasepool(|_| {
            renderer.draw(snapshot.as_deref(), |canvas, width, height| {
                // Sample time after nextDrawable's possible vsync wait, immediately before paint.
                let now = Instant::now();
                if self.motion.is_active() {
                    let dt = self
                        .animation_time
                        .replace(now)
                        .map_or(0.0, |previous| now.duration_since(previous).as_secs_f64());
                    self.motion.advance(&mut self.scene.camera, dt);
                }
                if !self.motion.is_active() {
                    self.animation_time = None;
                }
                self.scene.paint(canvas, width, height)
            })
        });
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        self.scene.frame_ms = elapsed;
        match result {
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
            Ok(false) => {
                self.animation_time = None;
                if !renderer.window.is_minimized().unwrap_or(false) {
                    self.retry_draw = true;
                    event_loop.set_control_flow(ControlFlow::WaitUntil(
                        Instant::now() + std::time::Duration::from_millis(16),
                    ));
                }
                return;
            }
            Ok(true) => {
                self.retry_draw = false;
            }
        }
        if ready && let Some(check) = &mut self.animation_check {
            check.record(self.scene.camera, elapsed, self.motion.is_active());
        }
        if ready && let Some(smoke) = &mut self.smoke {
            let now = Instant::now();
            if snapshot.is_none() {
                smoke.timings.push(elapsed);
                if let Some(previous) = smoke.previous {
                    smoke
                        .intervals
                        .push(now.duration_since(previous).as_secs_f64() * 1000.0);
                }
                smoke.previous = Some(now);
            } else {
                smoke.previous = None;
            }
            smoke.frame += 1;
            if smoke.frame >= 180 {
                smoke.timings.sort_by(f64::total_cmp);
                smoke.intervals.sort_by(f64::total_cmp);
                let percentile = |values: &[f64], p: f64| {
                    values[((values.len() - 1) as f64 * p).round() as usize]
                };
                let report = format!(
                    "Native Metal smoke test passed\nDocuments: {}\nSource lines: {}\nCached layout blocks: {}\nPresented frames: {}\nCPU draw + submit, excluding screenshots: median {:.2} ms, p95 {:.2} ms, max {:.2} ms\nEvent/presentation intervals: median {:.2} ms, p95 {:.2} ms\nTimings include drawable acquisition/backpressure; they are not GPU timestamps or guaranteed display FPS.\nSnapshots: 01-reading.png, 02-overview.png, 03-code-and-tables.png\n",
                    self.scene.total,
                    self.scene.lines,
                    self.scene.layout_blocks,
                    smoke.frame,
                    percentile(&smoke.timings, 0.5),
                    percentile(&smoke.timings, 0.95),
                    smoke.timings.last().unwrap(),
                    percentile(&smoke.intervals, 0.5),
                    percentile(&smoke.intervals, 0.95)
                );
                if let Err(error) = std::fs::write(smoke.directory.join("report.txt"), &report) {
                    self.fail(event_loop, error);
                    return;
                }
                println!("{report}");
                event_loop.exit();
                return;
            }
        }
        // A wheel event starts an animation, rather than drawing just one zoom step.
        // Metal's synchronized drawable presentation provides frame pacing.
        if !ready || self.smoke.is_some() || self.motion.is_active() {
            renderer.window.request_redraw();
        }
        self.wait_for_event(event_loop);
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if self
            .animation_check
            .as_ref()
            .and_then(AnimationCheck::deadline)
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            let check = self.animation_check.take().unwrap();
            match check.finish() {
                Ok(report) => {
                    println!("{report}");
                    event_loop.exit();
                }
                Err(error) => self.fail(event_loop, error),
            }
            return;
        }
        if self.retry_draw && matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.retry_draw = false;
            self.wait_for_event(event_loop);
            if let Some(renderer) = &self.renderer {
                renderer.window.request_redraw();
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_none() {
            match Renderer::new(event_loop) {
                Ok(renderer) => {
                    renderer.window.request_redraw();
                    self.renderer = Some(renderer);
                }
                Err(error) => self.fail(event_loop, error),
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::RedrawRequested) {
            self.redraw(event_loop);
            return;
        }
        let Some(renderer) = &self.renderer else {
            return;
        };
        let (width, height) = renderer.logical_size();
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => renderer.resize(),
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                // Resume smoothly instead of treating time spent hidden as an animation frame.
                self.animation_time = None;
            }
            WindowEvent::Focused(false) => {
                self.middle_down = false;
                self.left_down = false;
                self.space_down = false;
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                let p = position.to_logical::<f64>(renderer.window.scale_factor());
                if self.middle_down || (self.space_down && self.left_down) {
                    self.scene
                        .camera
                        .pan(p.x - self.cursor.0, p.y - self.cursor.1);
                    self.motion.cancel(self.scene.camera);
                    self.animation_time = None;
                }
                self.cursor = (p.x, p.y);
                if !self.middle_down && !(self.space_down && self.left_down) {
                    return;
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                match button {
                    MouseButton::Middle => {
                        self.middle_down = pressed;
                        if pressed {
                            self.motion.cancel(self.scene.camera);
                        }
                    }
                    MouseButton::Left => {
                        self.left_down = pressed;
                        if pressed && self.space_down {
                            self.motion.cancel(self.scene.camera);
                        }
                        if pressed && !self.space_down {
                            let before = self.scene.camera;
                            self.scene.navigate_minimap(
                                self.cursor.0,
                                self.cursor.1,
                                width,
                                height,
                            );
                            let destination = self.scene.camera;
                            self.scene.camera = before;
                            if destination != before {
                                self.motion.move_to(destination);
                            }
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64 * 0.12,
                    MouseScrollDelta::PixelDelta(p) => p.y / renderer.window.scale_factor() * 0.003,
                };
                if !amount.is_finite() || amount == 0.0 {
                    return;
                }
                self.motion.zoom_at(
                    &self.scene.camera,
                    self.cursor.0,
                    self.cursor.1,
                    amount.clamp(-0.7, 0.7).exp(),
                );
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if event.logical_key == Key::Named(NamedKey::Space) {
                    self.space_down = pressed;
                }
                if pressed && !event.repeat {
                    match &event.logical_key {
                        Key::Character(key) if key.eq_ignore_ascii_case("f") => {
                            let mut destination = self.scene.camera;
                            destination.fit(self.scene.bounds, width, height - 38.0, TOP_BAR);
                            self.motion.move_to(destination);
                        }
                        Key::Character(key) if key == "1" => {
                            let mut destination = self.scene.camera;
                            destination.zoom_at(width / 2.0, height / 2.0, 1.0 / destination.scale);
                            self.motion.move_to(destination);
                        }
                        Key::Character(key) if key.eq_ignore_ascii_case("r") => {
                            self.motion.move_to(Camera::default());
                            self.scene.message.clear();
                        }
                        Key::Character(key) if key.eq_ignore_ascii_case("l") => {
                            self.scene.lod = !self.scene.lod
                        }
                        Key::Character(key)
                            if key.eq_ignore_ascii_case("q") && self.modifiers.super_key() =>
                        {
                            event_loop.exit()
                        }
                        Key::Named(NamedKey::Escape) => {
                            self.middle_down = false;
                            self.left_down = false;
                            self.space_down = false;
                            self.motion.cancel(self.scene.camera);
                            self.scene.message.clear();
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            self.motion.pan_by(&self.scene.camera, 160.0, 0.0)
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            self.motion.pan_by(&self.scene.camera, -160.0, 0.0)
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            self.motion.pan_by(&self.scene.camera, 0.0, 160.0)
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            self.motion.pan_by(&self.scene.camera, 0.0, -160.0)
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::DroppedFile(path) => match documents::load_documents(&path) {
                Ok(documents) if !documents.is_empty() => {
                    self.scene = Scene::new(documents);
                    self.motion.cancel(self.scene.camera);
                    self.animation_time = None;
                }
                Ok(_) => self.scene.message = "没有找到 Markdown 文件。按 R 返回。".into(),
                Err(error) => {
                    eprintln!("{error}");
                    self.scene.message = format!("读取失败：{error}");
                }
            },
            _ => return,
        }
        if self.motion.is_active() {
            self.animation_time.get_or_insert_with(Instant::now);
        } else {
            self.animation_time = None;
        }
        renderer
            .window
            .set_cursor(if self.middle_down || (self.space_down && self.left_down) {
                CursorIcon::Grabbing
            } else if self.space_down {
                CursorIcon::Grab
            } else {
                CursorIcon::Default
            });
        renderer.window.request_redraw();
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let Some(options) = Options::parse()? else {
        return Ok(());
    };
    if options.demo {
        documents::ensure_demo_documents(&options.directory, options.count)?;
    }
    let documents = documents::load_documents(&options.directory)?;
    if documents.is_empty() {
        return Err(format!("No Markdown files found in {}", options.directory.display()).into());
    }
    println!(
        "Loading {} Markdown files from {}",
        documents.len(),
        options.directory.display()
    );
    let smoke = options.smoke.map(|directory| Smoke {
        directory,
        frame: 0,
        timings: Vec::new(),
        intervals: Vec::new(),
        previous: None,
    });
    let mut app = App {
        renderer: None,
        scene: Scene::new(documents),
        cursor: (720.0, 450.0),
        middle_down: false,
        left_down: false,
        space_down: false,
        modifiers: ModifiersState::default(),
        smoke,
        error: None,
        occluded: false,
        retry_draw: false,
        motion: CameraMotion::new(Camera::default()),
        animation_time: None,
        animation_check: options.animation_check.map(AnimationCheck::new),
    };
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error.into());
    }
    Ok(())
}
