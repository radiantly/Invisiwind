#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod delegate;
mod gui;
mod native;
mod remote;

use std::env;
use std::process::ExitCode;

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

fn main() -> ExitCode {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = env::args().collect();
    if args.len() <= 1 {
        gui::start();
        ExitCode::SUCCESS
    } else {
        match cli::start() {
            true => ExitCode::SUCCESS,
            false => ExitCode::FAILURE,
        }
    }
}
