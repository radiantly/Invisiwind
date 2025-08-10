use crate::injector;
use clap::{ArgAction, Args, Error, Parser, error::ErrorKind};
use dll_syringe::process::{OwnedProcess, Process};
use std::collections::HashMap;
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

pub fn start() {
    // attempt to attach to parent console window
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };

    let cli = Cli::parse();

    // iterate through targets
    let processes: HashMap<_, _> = cli
        .targets
        .into_iter()
        // convert to list of (pid, process)
        .flat_map(|target| {
            if let Ok(pid) = target.parse::<u32>() {
                match OwnedProcess::from_pid(pid) {
                    Ok(process) => vec![(pid, process)],
                    Err(err) => {
                        print_error(err.to_string());
                        vec![]
                    }
                }
            } else {
                let processes = OwnedProcess::find_all_by_name(&target);

                if processes.len() == 0 {
                    print_error(format!("Could not find any processes with name {}", target));
                }

                processes
                    .into_iter()
                    .filter_map(|process| match process.pid() {
                        Ok(pid) => Some((pid.get(), process)),
                        Err(err) => {
                            print_error(err.to_string());
                            None
                        }
                    })
                    .collect()
            }
        })
        .collect();

    // populate available windows
    let mut windows: HashMap<u32, Vec<u32>> = HashMap::new();
    injector::get_top_level_windows()
        .into_iter()
        .for_each(|window_info| {
            windows
                .entry(window_info.pid)
                .or_insert_with(Vec::new)
                .push(window_info.hwnd)
        });

    processes.into_iter().for_each(|(pid, process)| {
        if let Some(hwnds) = windows.remove(&pid) {
            injector::set_window_props(process, &hwnds, cli.hide_args.hide, false);
        } else {
            print_error(format!(
                "Cannot find any top level windows for pid {:?}",
                pid
            ));
        }
    });
}
