use std::{error::Error, path::Path};

use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::NSView;
use objc2_core_foundation::CGSize;
use objc2_metal::{MTLCommandBuffer, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use skia_safe::{
    Canvas, ColorType, EncodedImageFormat,
    gpu::{self, DirectContext, SurfaceOrigin, backend_render_targets, mtl},
};
use winit::{dpi::LogicalSize, event_loop::ActiveEventLoop, window::Window};

pub struct Renderer {
    // Skia must release its GPU resources while the queue, layer and window still exist.
    pub context: DirectContext,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    layer: Retained<CAMetalLayer>,
    pub window: Window,
}

impl Renderer {
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, Box<dyn Error>> {
        let window = event_loop.create_window(
            Window::default_attributes()
                .with_title("Markdown Atlas · Rust / Skia / Metal")
                .with_inner_size(LogicalSize::new(1440.0, 940.0))
                .with_min_inner_size(LogicalSize::new(760.0, 520.0)),
        )?;
        let device = MTLCreateSystemDefaultDevice().ok_or("No Metal GPU is available")?;
        let queue = device
            .newCommandQueue()
            .ok_or("Unable to create a Metal queue")?;
        let layer = CAMetalLayer::new();
        layer.setDevice(Some(&device));
        layer.setPixelFormat(objc2_metal::MTLPixelFormat::BGRA8Unorm);
        layer.setPresentsWithTransaction(false);
        layer.setFramebufferOnly(false);
        layer.setDisplaySyncEnabled(true);
        layer.setContentsScale(window.scale_factor());
        let size = window.inner_size();
        layer.setDrawableSize(CGSize::new(size.width as f64, size.height as f64));
        let handle = window.window_handle()?.as_raw();
        let RawWindowHandle::AppKit(handle) = handle else {
            return Err("This Metal demo requires macOS".into());
        };
        // Winit owns the NSView and keeps it alive for the entire lifetime of Renderer.
        let view = unsafe { (handle.ns_view.as_ptr() as *mut NSView).as_ref() }
            .ok_or("Missing native view")?;
        view.setWantsLayer(true);
        view.setLayer(Some(&layer.clone().into_super()));
        let backend = unsafe {
            mtl::BackendContext::new(
                Retained::as_ptr(&device) as mtl::Handle,
                Retained::as_ptr(&queue) as mtl::Handle,
            )
        };
        let mut context = gpu::direct_contexts::make_metal(&backend, None)
            .ok_or("Skia could not create its Metal context")?;
        context.set_resource_cache_limit(256 * 1024 * 1024);
        Ok(Self {
            context,
            queue,
            layer,
            window,
        })
    }

    pub fn resize(&self) {
        let size = self.window.inner_size();
        self.layer.setContentsScale(self.window.scale_factor());
        self.layer
            .setDrawableSize(CGSize::new(size.width as f64, size.height as f64));
    }

    pub fn logical_size(&self) -> (f64, f64) {
        let size = self
            .window
            .inner_size()
            .to_logical::<f64>(self.window.scale_factor());
        (size.width, size.height)
    }

    pub fn draw(
        &mut self,
        snapshot: Option<&Path>,
        paint: impl FnOnce(&Canvas, f64, f64),
    ) -> Result<bool, Box<dyn Error>> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(false);
        }
        let Some(drawable) = self.layer.nextDrawable() else {
            return Ok(false);
        };
        let texture = drawable.texture();
        let info = unsafe { mtl::TextureInfo::new(Retained::as_ptr(&texture) as mtl::Handle) };
        let target =
            backend_render_targets::make_mtl((size.width as i32, size.height as i32), &info);
        let mut surface = gpu::surfaces::wrap_backend_render_target(
            &mut self.context,
            &target,
            SurfaceOrigin::TopLeft,
            ColorType::BGRA8888,
            None,
            None,
        )
        .ok_or("Skia could not wrap the Metal drawable")?;
        let dpr = self.window.scale_factor();
        surface.canvas().scale((dpr as f32, dpr as f32));
        paint(
            surface.canvas(),
            size.width as f64 / dpr,
            size.height as f64 / dpr,
        );
        self.context
            .flush_and_submit()
            .map_err(|error| format!("GPU flush failed: {error}"))?;
        if let Some(path) = snapshot {
            let image = surface.image_snapshot();
            let data = image
                .encode(&mut self.context, EncodedImageFormat::PNG, None)
                .ok_or("Could not encode the GPU frame as PNG")?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, data.as_bytes())?;
        }
        drop(surface);
        let command = self
            .queue
            .commandBuffer()
            .ok_or("No Metal command buffer")?;
        let presentable: Retained<ProtocolObject<dyn objc2_metal::MTLDrawable>> =
            (&drawable).into();
        command.presentDrawable(&presentable);
        command.commit();
        Ok(true)
    }
}
