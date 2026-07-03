use std::sync::Arc;

use crate::logging::{LogOptions, log_as};
use crate::renderer::Renderer;
use crate::window;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId};

#[derive(Default)]
pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    window_shown: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let log = log_as(Some("APP"), LogOptions::default());
        let log_verbose = log_as(Some("APP"), LogOptions { verbose_only: true });

        let attribs = WindowAttributes::default()
            .with_title(window::WINDOW_TITLE)
            .with_visible(false)
            .with_fullscreen(Some(Fullscreen::Borderless(None)));

        let window = Arc::new(event_loop.create_window(attribs).unwrap());
        let mut renderer = pollster::block_on(Renderer::new(window.clone()));

        renderer.resize(window.inner_size());
        self.window_shown = false;

        match renderer.render() {
            Ok(()) => {
                window.set_visible(true);
                self.window_shown = true;
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                renderer.resize(window.inner_size());
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                log("startup render ran out of memory");
                event_loop.exit();
            }
            Err(wgpu::SurfaceError::Timeout) => {
                log("startup render timed out");
            }
        }

        window.request_redraw();

        log_verbose("window & renderer created");
        self.renderer = Some(renderer);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                let log = log_as(Some("APP"), LogOptions::default());
                log("close requested");
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(new_size);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    match renderer.render() {
                        Ok(()) => {
                            if !self.window_shown {
                                window.set_visible(true);
                                self.window_shown = true;
                            }
                        }
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            renderer.resize(window.inner_size());
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            event_loop.exit();
                        }
                        Err(wgpu::SurfaceError::Timeout) => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}
