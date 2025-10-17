#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod gui;
mod native;

use std::env;

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

fn main() {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = env::args().collect();
    if args.len() <= 1 {
        gui::start();
    } else {
        cli::start();
    }
}
