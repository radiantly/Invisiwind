//! Handing cross-architecture targets to an architecture-appropriate build of Invisiwind

use crate::remote;
use std::env;
use std::error;
use std::path::PathBuf;
use std::process::Command;
use tracing::debug;
use windows::Win32::System::SystemInformation::{
    IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64, IMAGE_FILE_MACHINE_I386,
};

type Result<T> = std::result::Result<T, Box<dyn error::Error>>;

/// Set on the child so that a mis-detected architecture fails fast rather than spawning an
/// unbounded chain of processes.
pub const NO_DELEGATE_VAR: &str = "INVISIWIND_NO_DELEGATE";

/// The sibling binary that handles each architecture. The build for [`remote::HOST_MACHINE`]
/// is the running executable itself and is never looked up here.
fn binary_name(machine: IMAGE_FILE_MACHINE) -> Option<&'static str> {
    match machine {
        IMAGE_FILE_MACHINE_I386 => Some("Invisiwind32.exe"),
        IMAGE_FILE_MACHINE_AMD64 => Some("Invisiwind64.exe"),
        IMAGE_FILE_MACHINE_ARM64 => Some("Invisiwind64a.exe"),
        _ => None,
    }
}

fn binary_path(machine: IMAGE_FILE_MACHINE) -> Result<PathBuf> {
    let name = binary_name(machine).ok_or_else(|| {
        format!(
            "cannot operate on {} processes",
            remote::machine_name(machine)
        )
    })?;

    let mut path = env::current_exe()?;
    path.pop();
    path.push(name);

    if !path.exists() {
        return Err(format!(
            "{} targets need {} next to {}",
            remote::machine_name(machine),
            name,
            env::current_exe()?.display()
        )
        .into());
    }

    Ok(path)
}

/// Re-runs the operation in the build matching `machine`.
pub fn run(
    machine: IMAGE_FILE_MACHINE,
    pid: u32,
    hwnds: &[u32],
    hide: bool,
    hide_from_taskbar: Option<bool>,
) -> Result<()> {
    if env::var_os(NO_DELEGATE_VAR).is_some() {
        return Err(format!(
            "refusing to delegate again: this is the {} build but the target is {}",
            remote::machine_name(remote::HOST_MACHINE),
            remote::machine_name(machine)
        )
        .into());
    }

    let path = binary_path(machine)?;

    let mut command = Command::new(&path);
    command.env(NO_DELEGATE_VAR, "1");
    command.arg(if hide { "--hide" } else { "--unhide" });

    for hwnd in hwnds {
        command.arg("--hwnd").arg(hwnd.to_string());
    }

    if let Some(hide_from_taskbar) = hide_from_taskbar {
        command
            .arg("--hide-from-taskbar")
            .arg(hide_from_taskbar.to_string());
    }

    command.arg(pid.to_string());

    debug!("delegating to {:?}", command);
    let status = command.status()?;

    if !status.success() {
        return Err(format!(
            "{} exited with {}",
            path.file_name().unwrap_or(path.as_os_str()).display(),
            status
        )
        .into());
    }

    Ok(())
}
