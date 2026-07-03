#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unused)]

mod adf;
mod app;
mod info;
mod intro;
mod logging;
mod renderer;
mod window;

use app::App;
use logging::{LogOptions, log_as, set_verbose_logging};

use mimalloc::MiMalloc;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;
use winit::event_loop::EventLoop;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// hello aperion
fn main() -> ExitCode {
    set_verbose_logging(false);

    let cli_command = match parse_cli_args() {
        Ok(command) => command,
        Err(message) => {
            maybe_attach_console_for_cli();
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    if !matches!(cli_command, CliCommand::RunApp) {
        maybe_attach_console_for_cli();
    }

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

    match cli_command {
        CliCommand::RunApp => {}
        CliCommand::CheckAdf(path) => {
            return run_check_adf(&path);
        }
        CliCommand::MakeAdfb { input, output } => {
            return run_make_adfb(&input, &output);
        }
        CliCommand::ViewAdf(path) => {
            return run_view_adf(&path);
        }
    }

    log("starting app");

    let event_loop = EventLoop::new().unwrap();
    let mut app = App::default();
    event_loop.run_app(&mut app).unwrap();

    ExitCode::SUCCESS
}

enum CliCommand {
    RunApp,
    CheckAdf(PathBuf),
    MakeAdfb { input: PathBuf, output: PathBuf },
    ViewAdf(PathBuf),
}

fn parse_cli_args() -> Result<CliCommand, String> {
    let mut args = std::env::args_os();
    let _program = args.next();

    let Some(first_arg) = args.next() else {
        return Ok(CliCommand::RunApp);
    };

    match first_arg.to_str() {
        Some("--checkadf") => {
            let Some(path) = args.next() else {
                return Err("missing file path after --checkadf".to_string());
            };

            if args.next().is_some() {
                return Err("unexpected extra arguments after --checkadf <file>".to_string());
            }

            Ok(CliCommand::CheckAdf(PathBuf::from(path)))
        }
        Some("--makeadfb") => {
            let Some(input) = args.next() else {
                return Err("missing input path after --makeadfb".to_string());
            };
            let Some(output) = args.next() else {
                return Err("missing output path after --makeadfb <input>".to_string());
            };

            if args.next().is_some() {
                return Err(
                    "unexpected extra arguments after --makeadfb <input> <output>".to_string()
                );
            }

            Ok(CliCommand::MakeAdfb {
                input: PathBuf::from(input),
                output: PathBuf::from(output),
            })
        }
        Some("--viewadf") => {
            let Some(path) = args.next() else {
                return Err("missing file path after --viewadf".to_string());
            };

            if args.next().is_some() {
                return Err("unexpected extra arguments after --viewadf <file>".to_string());
            }

            Ok(CliCommand::ViewAdf(PathBuf::from(path)))
        }
        _ => Err(format!(
            "unknown argument {:?}. supported usage: aperion --checkadf <file> | aperion --makeadfb <input> <output> | aperion --viewadf <file>",
            first_arg
        )),
    }
}

fn run_check_adf(path: &PathBuf) -> ExitCode {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("ADF read failed for {}:", path.display());
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    let started_at = Instant::now();

    match adf::check_bytes(&bytes) {
        Ok((encoding, summary)) => {
            let parse_elapsed = started_at.elapsed();
            println!(
                "ADF parsed successfully from {}",
                path.display()
            );
            println!("encoding: {}", encoding);
            println!(
                "version: {}, material imports: {}, edits: {}",
                summary.version,
                summary.material_imports,
                summary.edits
            );
            println!("parse time: {:?}", parse_elapsed);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ADF parse failed for {}:", path.display());
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run_make_adfb(input: &PathBuf, output: &PathBuf) -> ExitCode {
    let bytes = match fs::read(input) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("ADF read failed for {}:", input.display());
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    let document = match adf::parse_bytes(&bytes) {
        Ok((_encoding, document)) => document,
        Err(error) => {
            eprintln!("ADF parse failed for {}:", input.display());
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    let encoded = match adf::encode_binary(&document) {
        Ok(encoded) => encoded,
        Err(error) => {
            eprintln!("ADFB encode failed for {}:", input.display());
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = fs::write(output, encoded) {
        eprintln!("ADFB write failed for {}:", output.display());
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }

    println!(
        "wrote ADFB to {} from {}",
        output.display(),
        input.display()
    );
    ExitCode::SUCCESS
}

fn run_view_adf(path: &PathBuf) -> ExitCode {
    return ExitCode::SUCCESS;
}

#[cfg(windows)]
fn maybe_attach_console_for_cli() {
    windows_console::attach_to_parent_console();
}

#[cfg(not(windows))]
fn maybe_attach_console_for_cli() {}

#[cfg(windows)]
mod windows_console {
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::os::windows::io::IntoRawHandle;

    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(dwProcessId: u32) -> i32;
        fn AllocConsole() -> i32;
        fn SetStdHandle(nStdHandle: u32, hHandle: *mut c_void) -> i32;
    }

    pub fn attach_to_parent_console() {
        unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                let _ = AllocConsole();
            }
        }

        redirect_standard_handle("CONOUT$", STD_OUTPUT_HANDLE);
        redirect_standard_handle("CONOUT$", STD_ERROR_HANDLE);
    }

    fn redirect_standard_handle(path: &str, std_handle: u32) {
        let Ok(file) = OpenOptions::new().write(true).open(path) else {
            return;
        };

        let handle = file.into_raw_handle();

        unsafe {
            let _ = SetStdHandle(std_handle, handle.cast());
        }
    }
}
