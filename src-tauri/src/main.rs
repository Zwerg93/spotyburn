// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub use spotyburn_lib::commands::*;

fn main() {
    spotyburn_lib::run();
}
