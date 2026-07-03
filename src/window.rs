use std::env;
use std::path::{Path, PathBuf};

pub const WINDOW_TITLE: &str = "Aperion";
pub const INTRO_ASSETS: &[&str] = &["aperion", "formless"];

pub fn intro_asset_paths() -> Vec<PathBuf> {
    INTRO_ASSETS
        .iter()
        .map(|asset_name| {
            let relative = Path::new("assets").join("intros").join(asset_name);

            if let Ok(cwd) = env::current_dir() {
                let from_cwd = cwd.join(&relative);

                if from_cwd.exists() {
                    return from_cwd;
                }
            }

            let exe_dir = env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf));

            exe_dir
                .map(|dir| dir.join(&relative))
                .unwrap_or_else(|| PathBuf::from(&relative))
        })
        .collect()
}
