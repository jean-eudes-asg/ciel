// Ciel runs without a console window: Ciel is the whole UI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ciel_lib::run()
}
