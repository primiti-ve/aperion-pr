#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unused)]

mod app;
mod info;
mod intro;
mod logging;
mod renderer;
mod window;

use app::App;
use logging::{LogOptions, log_as, set_verbose_logging};

use mimalloc::MiMalloc;
use winit::event_loop::EventLoop;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// hello aperion
fn main() {
    set_verbose_logging(false);

    let log = log_as(Some("MAIN"), LogOptions::default());
    let log_noname = log_as(None, LogOptions::default());

    log_noname(info::ENGINE_LOGO);
    log_noname(&format!(
        "{} v{}",
        info::ENGINE_NAME,
        env!("CARGO_PKG_VERSION")
    ));
    log_noname(info::ENGINE_START_TEXT1);
    log_noname(info::ENGINE_START_TEXT2);

    log("starting app");

    let event_loop = EventLoop::new().unwrap();
    let mut app = App::default();
    event_loop.run_app(&mut app).unwrap();
}
