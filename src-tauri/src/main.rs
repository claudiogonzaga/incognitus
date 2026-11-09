// No Windows, sem janela de console no build final.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    incognitus_lib::run()
}
