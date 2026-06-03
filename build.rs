use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const ASSET_SOURCE_DIR: &str = "assets";

fn main() {
    println!("cargo:rerun-if-changed={ASSET_SOURCE_DIR}");

    if let Err(err) = copy_assets() {
        println!("cargo:warning=Could not stage assets: {err}");
    }
}

fn copy_assets() -> io::Result<()> {
    let source_dir = Path::new(ASSET_SOURCE_DIR);
    if !source_dir.exists() {
        return Ok(());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR should be set"));
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("expected OUT_DIR to be nested under target/<profile>/build");
    let asset_dir = profile_dir.join("assets");

    copy_dir_recursive(source_dir, &asset_dir)?;

    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let entry_type = entry.file_type()?;
        let dest_path = destination.join(entry.file_name());

        if entry_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else if entry_type.is_file() {
            fs::copy(entry.path(), dest_path)?;
        }
    }

    Ok(())
}
