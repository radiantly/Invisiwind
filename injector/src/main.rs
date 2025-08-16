#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod gui;
mod native;

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() <= 1 {
        gui::start();
    } else {
        cli::start();
    }
}
