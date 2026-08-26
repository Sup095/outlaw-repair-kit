//! The Outlaw Repair Kit's installer.
//!
//! A small program you download once and run. It works out what has been
//! released, downloads the version you pick, refuses to install anything whose
//! checksum does not match the one published beside it, puts the tool on your
//! PATH without asking for administrator rights, and offers -- offers -- to
//! set up a model sized for whatever graphics card you have.
//!
//! It exists so that installing this is not a terminal exercise. The shell
//! installers in `install/` do the same work and are still there for anyone
//! who prefers them; this one does it in a window, and answers the question
//! "what is it about to do to my computer" on screen, before it does any of it.
//!
//! It is deliberately small and deliberately dumb. It carries no copy of the
//! tool inside it -- it fetches what you choose -- so it stays a quick download
//! however large the thing it installs becomes.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod install;
mod job;
mod model;
mod release;
mod ui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([760.0, 620.0])
            .with_min_inner_size([680.0, 520.0])
            .with_title("Outlaw Repair Kit — Setup"),
        ..Default::default()
    };

    eframe::run_native(
        "outlaw-setup",
        options,
        Box::new(|context| Ok(Box::new(ui::Setup::new(context)))),
    )
}
