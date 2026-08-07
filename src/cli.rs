use crate::{native, remote};
use clap::{ArgAction, Args, Error, Parser, error::ErrorKind};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt::Display;
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

#[derive(Parser, Debug)]
#[command(name = "Invisiwind")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Hide certain windows when sharing your screen")]
#[command(disable_help_flag = true)]
struct Cli {
    #[arg(long, action = ArgAction::HelpLong, help = "Show command-line usage")]
    help: Option<bool>,

    #[command(flatten)]
    hide_args: HideArgs,

    #[arg(
        long,
        value_name = "HWND",
        help = "Only act on these window handles, instead of every window of the target"
    )]
    hwnd: Vec<u32>,

    #[arg(
        long,
        value_name = "BOOL",
        help = "Also hide the window from the taskbar and Alt+Tab, or restore it there"
    )]
    hide_from_taskbar: Option<bool>,

    #[arg(required = true)]
    targets: Vec<String>,
}

fn print_error(message: impl Display) {
    let _ = Error::raw(ErrorKind::InvalidValue, message).print();
}

#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
struct HideArgs {
    #[arg(short, long, help = "Hide a window")]
    hide: bool,

    #[arg(short, long, help = "Stop hiding a window")]
    unhide: bool,
}

/// Runs the command line interface, returning `true` if the whole request succeeded and `false`
/// if any part failed. Delegated invocations read this through the child's exit code, so it must
/// stay accurate.
pub fn start() -> bool {
    // attempt to attach to parent console window
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };

    let cli = Cli::parse();
    let mut failed = false;

    // iterate through targets
    let pids: HashSet<u32> = cli
        .targets
        .into_iter()
        // convert to list of pids
        .flat_map(|target| {
            if let Ok(pid) = target.parse::<u32>() {
                // open eagerly so an unusable pid is reported as such rather than as a
                // window lookup failure further down
                return match remote::Process::open(pid) {
                    Ok(_) => vec![pid],
                    Err(err) => {
                        print_error(err.to_string());
                        failed = true;
                        vec![]
                    }
                };
            }

            match remote::find_pids_by_name(&target) {
                Ok(pids) if pids.is_empty() => {
                    print_error(format!("Could not find any processes with name {}", target));
                    failed = true;
                    vec![]
                }
                Ok(pids) => pids,
                Err(err) => {
                    print_error(err.to_string());
                    failed = true;
                    vec![]
                }
            }
        })
        .collect();

    // populate available windows
    let mut windows: HashMap<u32, Vec<u32>> = HashMap::new();
    native::get_top_level_windows()
        .into_iter()
        .for_each(|window_info| {
            windows
                .entry(window_info.pid)
                .or_insert_with(Vec::new)
                .push(window_info.hwnd)
        });

    pids.into_iter().for_each(|pid| {
        let Some(mut hwnds) = windows.remove(&pid) else {
            print_error(format!(
                "Cannot find any top level windows for pid {:?}",
                pid
            ));
            failed = true;
            return;
        };

        // --hwnd narrows the operation to specific windows, which is how a delegated
        // single-window request from the GUI keeps from affecting the whole process.
        if !cli.hwnd.is_empty() {
            hwnds.retain(|hwnd| cli.hwnd.contains(hwnd));

            if hwnds.is_empty() {
                print_error(format!(
                    "None of the given window handles belong to pid {:?}",
                    pid
                ));
                failed = true;
                return;
            }
        }

        let result = native::Injector::set_window_props(
            pid,
            &hwnds,
            cli.hide_args.hide,
            cli.hide_from_taskbar,
        );
        if let Err(err) = result {
            print_error(format!("Error: {:?}", err));
            failed = true;
        }
    });

    !failed
}
