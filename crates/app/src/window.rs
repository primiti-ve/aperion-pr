use std::env;
use std::path::{Path, PathBuf};

pub const WINDOW_TITLE: &str = "Aperion";
pub const APERION: Option<&str> = Some("aperion");

pub fn intro_asset_name() -> Option<&'static str> {
    APERION
}

/// resolve intro asset paths
pub fn intro_asset_path() -> Option<PathBuf> {
    let asset_name = APERION?;

    let relative = Path::new("assets").join("intros").join(asset_name);

    if let Ok(cwd) = env::current_dir() {
        let from_cwd = cwd.join(&relative);

        if from_cwd.exists() {
            return Some(from_cwd);
        }
    }

    let exe_dir = env::current_exe().ok()?.parent()?.to_path_buf();
    Some(exe_dir.join(relative))
}
