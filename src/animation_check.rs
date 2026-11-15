use std::{
    error::Error,
    fmt::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::camera::Camera;

const WATCHDOG: Duration = Duration::from_secs(10);
const IDLE_WINDOW: Duration = Duration::from_millis(250);
const SCALE_FACTOR: f64 = 0.55;

struct Start {
    at: Instant,
    scale: f64,
    anchor: (f64, f64),
    world: (f64, f64),
}

struct Frame {
    elapsed: Duration,
    scale: f64,
    draw_ms: f64,
    anchor_error_px: f64,
    active: bool,
}

pub struct AnimationCheck {
    directory: PathBuf,
    start: Option<Start>,
    frames: Vec<Frame>,
    settled_at: Option<Instant>,
    settled_frame: Option<usize>,
    idle_redraws: usize,
}

impl AnimationCheck {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            start: None,
            frames: Vec::new(),
            settled_at: None,
            settled_frame: None,
            idle_redraws: 0,
        }
    }

    pub fn begin(&mut self, camera: Camera, anchor: (f64, f64)) {
        if self.started() {
            return;
        }
        self.start = Some(Start {
            at: Instant::now(),
            scale: camera.scale,
            anchor,
            world: camera.screen_to_world(anchor.0, anchor.1),
        });
    }

    pub fn started(&self) -> bool {
        self.start.is_some()
    }

    pub fn record(&mut self, camera: Camera, draw_ms: f64, active: bool) {
        let Some(start) = &self.start else {
            return;
        };
        let now = Instant::now();
        let anchor = camera.world_to_screen(start.world.0, start.world.1);
        let anchor_error_px = (anchor.0 - start.anchor.0).hypot(anchor.1 - start.anchor.1);
        if self.settled_at.is_some() {
            self.idle_redraws += 1;
        } else if !active {
            self.settled_at = Some(now);
            self.settled_frame = Some(self.frames.len());
        }
        self.frames.push(Frame {
            elapsed: now.duration_since(start.at),
            scale: camera.scale,
            draw_ms,
            anchor_error_px,
            active,
        });
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.settled_at
            .map(|at| at + IDLE_WINDOW)
            .or_else(|| self.start.as_ref().map(|start| start.at + WATCHDOG))
    }

    pub fn finish(&self) -> Result<String, Box<dyn Error>> {
        let mut failures = Vec::new();
        let initial_scale = self.start.as_ref().map(|start| start.scale);
        let expected_scale = initial_scale.map(|scale| scale * SCALE_FACTOR);
        let animation_frames = self
            .settled_frame
            .map(|index| &self.frames[..=index])
            .unwrap_or(&self.frames);
        let mut previous_scale = initial_scale.unwrap_or(f64::NAN);
        let mut changed_frames = 0;
        for frame in animation_frames {
            if (frame.scale - previous_scale).abs() > f64::EPSILON * previous_scale.abs().max(1.0) {
                changed_frames += 1;
            }
            previous_scale = frame.scale;
        }
        if !self.started() {
            failures.push("The animation check never started.");
        }
        if changed_frames < 8 {
            failures.push("Fewer than 8 frames changed the camera scale.");
        }
        let settled = self.settled_frame.and_then(|index| self.frames.get(index));
        let scale_matches = |scale: f64| {
            expected_scale.is_some_and(|expected| {
                scale.is_finite() && (scale - expected).abs() <= 1.0e-8 * expected.abs().max(1.0)
            })
        };
        if !settled.is_some_and(|frame| {
            !frame.active && frame.elapsed <= WATCHDOG && scale_matches(frame.scale)
        }) {
            failures.push("The camera did not converge to initial_scale * 0.55 within 10 seconds.");
        }
        if !self
            .frames
            .last()
            .is_some_and(|frame| scale_matches(frame.scale))
        {
            failures.push("The final camera scale does not match the expected target.");
        }
        let valid_samples = self.frames.iter().all(|frame| {
            frame.scale.is_finite()
                && frame.draw_ms.is_finite()
                && frame.draw_ms >= 0.0
                && frame.anchor_error_px.is_finite()
        });
        if !valid_samples {
            failures.push("At least one frame contains an invalid numeric sample.");
        }
        let max_anchor_error = self
            .frames
            .iter()
            .map(|frame| frame.anchor_error_px)
            .fold(0.0_f64, f64::max);
        if !valid_samples || max_anchor_error >= 0.01 {
            failures.push("The cursor anchor moved by at least 0.01 logical pixels.");
        }
        if self.idle_redraws > 2 {
            failures.push("More than 2 redraws occurred during the idle observation window.");
        }
        if let Some(index) = self.settled_frame
            && self.frames[index + 1..].iter().any(|frame| frame.active)
        {
            failures.push("Camera motion became active again during the idle observation window.");
        }
        let idle_elapsed = self.settled_at.map(|at| at.elapsed());
        if !idle_elapsed.is_some_and(|elapsed| elapsed >= IDLE_WINDOW) {
            failures.push("The full 250 ms idle observation window did not complete.");
        }

        let intervals = animation_frames
            .windows(2)
            .map(|pair| (pair[1].elapsed - pair[0].elapsed).as_secs_f64() * 1000.0)
            .collect::<Vec<_>>();
        let draw_times = animation_frames
            .iter()
            .map(|frame| frame.draw_ms)
            .filter(|value| value.is_finite())
            .collect::<Vec<_>>();
        let value = |number: Option<f64>| {
            number
                .map(|number| format!("{number:.9}"))
                .unwrap_or_else(|| "n/a".into())
        };
        let mut report = format!(
            "Native window smooth zoom check: {}\n\
             Input: one cursor-anchored zoom request, factor 0.55\n\
             Subsequent frames: application animation scheduling only\n\
             Successful draw/present submissions sampled: {}\n\
             Scale-changing animation frames: {} (required >= 8)\n\
             Initial scale: {}\n\
             Expected scale: {}\n\
             Final scale: {}\n\
             Animation duration to first inactive frame: {} ms\n\
             Animation submission intervals: {}\n\
             Draw + submit duration: {}\n\
             Maximum cursor-anchor error: {:.9} logical px (required < 0.01)\n\
             Idle observation elapsed: {} ms\n\
             Idle redraws: {} (allowed <= 2 for system redraws)\n\
             Timing samples are CPU wall-clock draw/submission measurements. They may include drawable acquisition and backpressure; they are not GPU timestamps, display-presented timestamps, or guaranteed display FPS.\n\
             Samples: frames.csv\n",
            if failures.is_empty() {
                "PASSED"
            } else {
                "FAILED"
            },
            self.frames.len(),
            changed_frames,
            value(initial_scale),
            value(expected_scale),
            value(self.frames.last().map(|frame| frame.scale)),
            value(settled.map(|frame| frame.elapsed.as_secs_f64() * 1000.0)),
            timing_summary(intervals),
            timing_summary(draw_times),
            max_anchor_error,
            value(idle_elapsed.map(|elapsed| elapsed.as_secs_f64() * 1000.0)),
            self.idle_redraws,
        );
        for failure in &failures {
            writeln!(&mut report, "FAIL: {failure}")?;
        }
        let mut csv = String::from(
            "frame,elapsed_ms,scale,draw_submit_ms,active,anchor_error_logical_px,phase\n",
        );
        for (index, frame) in self.frames.iter().enumerate() {
            writeln!(
                &mut csv,
                "{index},{:.6},{:.12},{:.6},{},{:.9},{}",
                frame.elapsed.as_secs_f64() * 1000.0,
                frame.scale,
                frame.draw_ms,
                frame.active,
                frame.anchor_error_px,
                if self.settled_frame.is_some_and(|settled| index > settled) {
                    "idle"
                } else {
                    "animation"
                },
            )?;
        }
        std::fs::create_dir_all(&self.directory)?;
        std::fs::write(self.directory.join("frames.csv"), csv)?;
        std::fs::write(self.directory.join("report.txt"), &report)?;
        if failures.is_empty() {
            Ok(report)
        } else {
            Err(std::io::Error::other(format!(
                "Smooth zoom check failed: {} Report: {}",
                failures.join(" "),
                self.directory.join("report.txt").display(),
            ))
            .into())
        }
    }
}

fn timing_summary(mut values: Vec<f64>) -> String {
    if values.is_empty() {
        return "n/a".into();
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    let median = if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    };
    let p95 = values[((values.len() - 1) as f64 * 0.95).round() as usize];
    format!("median {median:.3} ms, p95 {p95:.3} ms")
}
