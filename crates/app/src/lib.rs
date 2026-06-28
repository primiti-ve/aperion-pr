pub mod app;
pub mod window;
pub mod renderer;
pub mod intro;

pub use app::*;
pub use window::*;
pub use renderer::*;
pub use intro::*;

use winit::event_loop::EventLoop;

pub fn init() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::default();
    
    event_loop.run_app(&mut app).unwrap();
}
