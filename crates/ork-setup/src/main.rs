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
mod paint;
mod release;
mod ui;

/// Which way to draw, when the environment insists.
///
/// An escape hatch, not a setting. There is no reason for anybody to need it,
/// and if somebody does, being able to say "run it with `ORK_SETUP_RENDERER=gl`"
/// is the difference between a bug report and a person who cannot install the
/// tool at all.
fn forced_renderer() -> Option<eframe::Renderer> {
    match std::env::var("ORK_SETUP_RENDERER")
        .ok()?
        .to_lowercase()
        .as_str()
    {
        "gl" | "glow" | "opengl" => Some(eframe::Renderer::Glow),
        "wgpu" | "vulkan" | "dx12" => Some(eframe::Renderer::Wgpu),
        _ => None,
    }
}

fn options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([840.0, 660.0])
        .with_min_inner_size([680.0, 520.0])
        .with_title("Outlaw Repair Kit — Setup");
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions {
        viewport,
        renderer,
        ..Default::default()
    }
}

/// The project's icon, so the installer is recognisable in a taskbar.
///
/// Best-effort: an installer that refused to start because it could not
/// decode its own picture would be a poor trade.
fn icon() -> Option<std::sync::Arc<eframe::egui::IconData>> {
    let bytes = include_bytes!("../assets/outlaw.png");
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(std::sync::Arc::new(eframe::egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }))
}

fn start(renderer: eframe::Renderer) -> eframe::Result<()> {
    eframe::run_native(
        "outlaw-setup",
        options(renderer),
        Box::new(|context| Ok(Box::new(ui::Setup::new(context)))),
    )
}

/// Draw with wgpu, and fall back to OpenGL if there is nothing for it to use.
///
/// This is the one program that cannot have prerequisites, so it carries two
/// ways of drawing and tries them in order.
///
/// **Why wgpu is first, having previously been rejected outright.** The
/// OpenGL path renders a blank white window on a perfectly ordinary machine:
/// a Windows desktop with an RTX 3090, current drivers, and the overlay
/// software that comes with a graphics card. OpenGL initialises without
/// complaint -- the context is created, the shaders compile, egui lays out
/// every frame at the right size -- and not one of those frames reaches the
/// screen. Overlay injectors hook the OpenGL buffer swap, and there are a
/// great many of them on consumer machines: the card's own overlay, Discord,
/// OBS, Overwolf, Steam. The same machine draws the window correctly through
/// wgpu, which reaches Direct3D or Vulkan instead.
///
/// An installer that opens a blank white window has failed completely, and it
/// has failed at the first thing anybody sees. That outweighs the reasons wgpu
/// was passed over -- a larger download and a slower build -- by a distance.
///
/// OpenGL stays, second, because wgpu needs Direct3D 12, Vulkan, or GLES, and
/// a machine old enough to have none of them is exactly the kind of machine
/// somebody would be installing a repair tool on.
fn main() -> eframe::Result<()> {
    if let Some(renderer) = forced_renderer() {
        return start(renderer);
    }

    match start(eframe::Renderer::Wgpu) {
        Ok(()) => Ok(()),
        Err(error) => {
            // Not a silent retry: somebody watching a terminal should see why
            // the second attempt is happening, and a crash report should carry
            // both halves.
            eprintln!("could not draw with wgpu ({error}); falling back to OpenGL");
            start(eframe::Renderer::Glow)
        }
    }
}
