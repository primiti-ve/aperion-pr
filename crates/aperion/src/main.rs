#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unused)]

use aperion_app::App;
use aperion_logger::{LogOptions, log_as, set_verbose_logging};

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// hello aperion
fn main() {
    set_verbose_logging(false);

    let log = log_as(Some("MAIN"), LogOptions::default());
    let log_noname = log_as(None, LogOptions::default());

    log_noname(aperion_shared::ENGINE_LOGO);
    log_noname(&format!(
        "{} v{}",
        aperion_shared::ENGINE_NAME,
        env!("CARGO_PKG_VERSION")
    ));
    log_noname(aperion_shared::ENGINE_START_TEXT1);
    log_noname(aperion_shared::ENGINE_START_TEXT2);

    log("starting app");

    aperion_app::init();
}
