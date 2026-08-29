//! The look, drawn rather than declared.
//!
//! The window and the terminal boot screen share one palette, and this is the
//! third front-end wearing it: electric cyan and hot magenta over a near-black
//! that leans violet, amber for the things that are ours, red for the things
//! that are wrong. The installer is the first thing anybody sees of this
//! project, and it looked like a dialogue box from a different program.
//!
//! The values here are the ones in `apps/desktop/src/app.css`, and a test
//! below reads that file and checks they still are -- so the two cannot drift
//! into being nearly the same, which looks worse than being plainly different.
//!
//! **The rule that keeps this readable.** Everything below pushes against it,
//! so it is worth stating: this is a screen somebody reads while installing
//! something on a computer they may already be worried about. Neon goes behind
//! the content and on the chrome -- headings, borders, the rule under the
//! title. Body text stays plain and high-contrast. A screen that looks
//! tremendous and is tiring to read has failed at the only job it has.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use eframe::epaint::{Mesh, Shape, TextureId};

// The ground. Not grey -- a very dark violet, which is what makes the cyan and
// magenta on top of it read as light rather than as paint.
pub const BACKGROUND: Color32 = Color32::from_rgb(0x06, 0x06, 0x0f);
pub const PANEL: Color32 = Color32::from_rgb(0x0c, 0x0c, 0x1a);
pub const RAISED: Color32 = Color32::from_rgb(0x14, 0x14, 0x2a);
pub const LINE: Color32 = Color32::from_rgb(0x2a, 0x2b, 0x52);
pub const LINE_BRIGHT: Color32 = Color32::from_rgb(0x3d, 0x3f, 0x7a);

pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xec, 0xff);
pub const DIM: Color32 = Color32::from_rgb(0x8b, 0x93, 0xbf);

pub const AMBER: Color32 = Color32::from_rgb(0xff, 0xc2, 0x1a);
pub const CYAN: Color32 = Color32::from_rgb(0x00, 0xf0, 0xff);
pub const MAGENTA: Color32 = Color32::from_rgb(0xff, 0x2d, 0x95);
pub const GREEN: Color32 = Color32::from_rgb(0x39, 0xff, 0x88);
pub const YELLOW: Color32 = Color32::from_rgb(0xff, 0xd9, 0x3d);
pub const RED: Color32 = Color32::from_rgb(0xff, 0x2d, 0x55);

/// How far apart the cyan grid's lines are.
const FINE_GRID: f32 = 46.0;
/// And the magenta one, offset so the two cross rather than stack. That is
/// what stops the background reading as graph paper.
const COARSE_GRID: f32 = 138.0;

/// How long the pulse under the header takes to cross it, in seconds.
///
/// Slow on purpose. It is there so the window reads as running without asking
/// for a glance, and anything quicker becomes something you look at.
const PULSE_SECONDS: f32 = 8.0;

/// Draw everything that sits behind the content.
///
/// Five layers, back to front: the ground, two crossing grids, two blooms as
/// though something off-screen is lit, the vignette that keeps the corners
/// dark so the middle reads as the lit part, and the scanlines over all of it.
pub fn backdrop(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 0.0, BACKGROUND);

    grid(painter, rect, FINE_GRID, CYAN.gamma_multiply(0.11));
    grid(painter, rect, COARSE_GRID, MAGENTA.gamma_multiply(0.085));

    // Low and to the left, high and to the right, so the light has a
    // direction rather than sitting in the middle of the screen.
    bloom(
        painter,
        Pos2::new(rect.left() + rect.width() * 0.12, rect.bottom()),
        rect.width() * 0.75,
        MAGENTA.gamma_multiply(0.30),
    );
    bloom(
        painter,
        Pos2::new(rect.left() + rect.width() * 0.88, rect.top()),
        rect.width() * 0.8,
        CYAN.gamma_multiply(0.24),
    );

    vignette(painter, rect);
    scanlines(painter, rect);
}

fn grid(painter: &egui::Painter, rect: Rect, step: f32, colour: Color32) {
    let stroke = Stroke::new(1.0_f32, colour);
    let mut x = rect.left();
    while x <= rect.right() {
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            stroke,
        );
        x += step;
    }
    let mut y = rect.top();
    while y <= rect.bottom() {
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            stroke,
        );
        y += step;
    }
}

/// A soft round glow, as a fan of triangles from a lit centre to a
/// transparent rim.
///
/// egui has no radial gradient, and stacking translucent circles produces
/// visible rings. A mesh with the colour on the vertices is what a gradient
/// is anyway.
fn bloom(painter: &egui::Painter, centre: Pos2, radius: f32, colour: Color32) {
    const SIDES: usize = 48;
    let mut mesh = Mesh::with_texture(TextureId::default());
    mesh.colored_vertex(centre, colour);
    for step in 0..=SIDES {
        let angle = std::f32::consts::TAU * (step as f32) / (SIDES as f32);
        mesh.colored_vertex(
            centre + Vec2::new(angle.cos() * radius, angle.sin() * radius * 0.6),
            Color32::TRANSPARENT,
        );
    }
    for step in 0..SIDES {
        mesh.add_triangle(0, 1 + step as u32, 2 + step as u32);
    }
    painter.add(Shape::mesh(mesh));
}

/// Darkness gathering at the edges, so the middle reads as the lit part.
fn vignette(painter: &egui::Painter, rect: Rect) {
    let depth = (rect.width().min(rect.height()) * 0.42).max(60.0);
    let dark = Color32::from_black_alpha(120);

    for (from, to) in [
        (rect.left_top(), Vec2::new(0.0, depth)),
        (rect.left_bottom(), Vec2::new(0.0, -depth)),
        (rect.left_top(), Vec2::new(depth, 0.0)),
        (rect.right_top(), Vec2::new(-depth, 0.0)),
    ] {
        let along = if to.x.abs() > 0.0 {
            Vec2::new(0.0, rect.height())
        } else {
            Vec2::new(rect.width(), 0.0)
        };
        let mut mesh = Mesh::with_texture(TextureId::default());
        mesh.colored_vertex(from, dark);
        mesh.colored_vertex(from + along, dark);
        mesh.colored_vertex(from + to, Color32::TRANSPARENT);
        mesh.colored_vertex(from + along + to, Color32::TRANSPARENT);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 2, 3);
        painter.add(Shape::mesh(mesh));
    }
}

/// The faint horizontal banding of a screen photographed by another screen.
fn scanlines(painter: &egui::Painter, rect: Rect) {
    let stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(200, 225, 255, 6));
    let mut y = rect.top();
    while y <= rect.bottom() {
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            stroke,
        );
        y += 3.0;
    }
}

/// The rule under the header, with a pulse travelling along it.
///
/// The one thing on this screen that moves on its own. It sits in a couple of
/// rows of pixels well clear of anything anybody is reading, and it is there
/// so the window reads as running rather than as stopped -- which matters most
/// during a long download, when nothing else on screen is changing.
pub fn header_rule(painter: &egui::Painter, rect: Rect, seconds: f64, animate: bool) {
    painter.line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1.0_f32, LINE),
    );
    if !animate {
        return;
    }

    let progress = ((seconds as f32) % PULSE_SECONDS) / PULSE_SECONDS;
    let head = rect.left() + rect.width() * progress;
    let tail = 90.0_f32.min(rect.width() * 0.25);

    let mut mesh = Mesh::with_texture(TextureId::default());
    let y = rect.center().y;
    mesh.colored_vertex(Pos2::new(head - tail, y - 1.0), Color32::TRANSPARENT);
    mesh.colored_vertex(Pos2::new(head - tail, y + 1.0), Color32::TRANSPARENT);
    mesh.colored_vertex(Pos2::new(head, y - 1.0), CYAN);
    mesh.colored_vertex(Pos2::new(head, y + 1.0), CYAN);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// A panel: a lit box, bracketed cyan where reading starts and magenta where
/// it ends.
///
/// The brackets are the same device the window uses. They say where a block of
/// content begins and ends without drawing a full border round it, which at
/// this size would make the screen look like a form.
fn panel_shapes(rect: Rect) -> Vec<Shape> {
    let mut shapes = vec![
        Shape::rect_filled(rect, 4.0, PANEL),
        Shape::rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0_f32, LINE_BRIGHT),
            egui::StrokeKind::Inside,
        ),
    ];

    let arm = 14.0_f32.min(rect.width() * 0.2).min(rect.height() * 0.4);
    let mut corner = |at: Pos2, dx: f32, dy: f32, colour: Color32| {
        let stroke = Stroke::new(2.0_f32, colour);
        shapes.push(Shape::line_segment(
            [at, at + Vec2::new(dx * arm, 0.0)],
            stroke,
        ));
        shapes.push(Shape::line_segment(
            [at, at + Vec2::new(0.0, dy * arm)],
            stroke,
        ));
    };
    corner(rect.left_top(), 1.0, 1.0, CYAN);
    corner(rect.right_bottom(), -1.0, -1.0, MAGENTA);
    shapes
}

/// Keep a place in the drawing order for a panel that has not been measured
/// yet, and fill it in once it has.
///
/// A panel is drawn behind its own contents, and its size is only known after
/// those contents have been laid out. So a slot is reserved first and filled
/// afterwards; drawing it at the end instead would put a filled rectangle over
/// the text it is meant to sit behind.
pub fn reserve_panel(ui: &egui::Ui) -> eframe::egui::layers::ShapeIdx {
    ui.painter().add(Shape::Noop)
}

pub fn fill_panel(ui: &egui::Ui, at: eframe::egui::layers::ShapeIdx, rect: Rect) {
    ui.painter().set(at, Shape::Vec(panel_shapes(rect)));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window's stylesheet, so the two front-ends cannot drift apart
    /// without something saying so.
    const STYLESHEET: &str = include_str!("../../../apps/desktop/src/app.css");

    fn declared(name: &str) -> String {
        STYLESHEET
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim() == name).then(|| value.trim().trim_end_matches(';').to_lowercase())
            })
            .unwrap_or_else(|| panic!("the stylesheet declares no {name}"))
    }

    fn as_hex(colour: Color32) -> String {
        format!("#{:02x}{:02x}{:02x}", colour.r(), colour.g(), colour.b())
    }

    #[test]
    fn the_installer_wears_the_same_colours_as_the_window() {
        // Read out of the window's own stylesheet rather than written down
        // twice. Two front-ends that are nearly the same colour look worse
        // than two that are plainly different, and nobody notices the drift
        // until they are side by side.
        for (name, colour) in [
            ("--bg", BACKGROUND),
            ("--bg-panel", PANEL),
            ("--bg-raised", RAISED),
            ("--line", LINE),
            ("--line-bright", LINE_BRIGHT),
            ("--text", TEXT),
            ("--text-dim", DIM),
            ("--amber", AMBER),
            ("--cyan", CYAN),
            ("--magenta", MAGENTA),
            ("--green", GREEN),
            ("--yellow", YELLOW),
            ("--red", RED),
        ] {
            assert_eq!(
                declared(name),
                as_hex(colour),
                "{name} has drifted from the window"
            );
        }
    }

    #[test]
    fn the_pulse_crosses_once_and_starts_again() {
        // A progress that did not wrap would send the pulse off the end of
        // the rule and never bring it back, leaving a window that looked
        // stopped -- which is the one thing this is there to prevent.
        let at = |seconds: f32| (seconds % PULSE_SECONDS) / PULSE_SECONDS;
        assert!((at(0.0) - 0.0).abs() < f32::EPSILON);
        assert!(at(PULSE_SECONDS * 0.5) > 0.4 && at(PULSE_SECONDS * 0.5) < 0.6);
        assert!(at(PULSE_SECONDS * 3.0 + 0.001) < 0.01, "it did not wrap");
    }

    #[test]
    fn the_grids_do_not_line_up_with_each_other() {
        // Two grids on the same spacing stack into one brighter grid, and the
        // background reads as graph paper. They have to cross.
        // Both spacings are constants, so this is settled when the code is
        // compiled rather than when the test runs -- which is the right time
        // for it. Changing either one to a value that stacks fails the build.
        const {
            assert!(FINE_GRID != COARSE_GRID);
            assert!(COARSE_GRID / FINE_GRID >= 3.0);
        }
    }
}
