use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub type LogFn = Box<dyn Fn(&str) + Send + Sync + 'static>;

static VERBOSE_LOGGING_ENABLED: AtomicBool = AtomicBool::new(false);
static LOG_FILE: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
static LOG_START_TIME: OnceLock<Instant> = OnceLock::new();

#[derive(Clone, Copy, Default)]
pub struct LogOptions {
    pub verbose_only: bool,
}

pub fn set_verbose_logging(enabled: bool) {
    VERBOSE_LOGGING_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn verbose_logging_enabled() -> bool {
    VERBOSE_LOGGING_ENABLED.load(Ordering::Relaxed)
}

/// builds a logger tied to a specific name and visibility
pub fn log_as(name: Option<&str>, opts: LogOptions) -> LogFn {
    let name = name.map(str::to_string);

    Box::new(move |message: &str| {
        if opts.verbose_only && !verbose_logging_enabled() {
            return;
        }

        let line = match &name {
            Some(name) => format!("({}) [{}] {}", log_timestamp(), name, message),
            None => message.to_string(),
        };

        #[cfg(debug_assertions)]
        println!("{line}");

        #[cfg(not(debug_assertions))]
        write_log_file(&line);
    })
}

fn log_timestamp() -> String {
    let start = LOG_START_TIME.get_or_init(Instant::now);
    let elapsed = start.elapsed();

    let seconds = elapsed.as_secs();
    let millis = elapsed.subsec_millis();

    format!("{seconds:03}.{millis:03}s")
}

#[cfg(not(debug_assertions))]
fn write_log_file(line: &str) {
    let file_mutex = LOG_FILE.get_or_init(|| Mutex::new(open_log_file()));

    let Ok(mut file_guard) = file_mutex.lock() else {
        return;
    };

    let Some(file) = file_guard.as_mut() else {
        return;
    };

    let _ = writeln!(file, "{line}");
    let _ = file.flush();
}

#[cfg(not(debug_assertions))]
fn open_log_file() -> Option<std::fs::File> {
    let log_path = log_file_path().unwrap_or_else(|| PathBuf::from("aperion.log"));

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .ok()
}

#[cfg(not(debug_assertions))]
fn log_file_path() -> Option<PathBuf> {
    let exe_path = std::env::current_exe().ok()?;
    let exe_dir = exe_path.parent()?;

    Some(exe_dir.join("aperion.log"))
}
