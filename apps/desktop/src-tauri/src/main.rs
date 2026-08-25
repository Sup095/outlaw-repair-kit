// Without this, a release build on Windows opens a console window behind the
// application. The debug build keeps it, because that is where logs go.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ork_desktop_lib::run();
}
