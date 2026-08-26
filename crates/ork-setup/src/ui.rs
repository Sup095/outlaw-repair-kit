//! The window.
//!
//! Four pages, in order, with a Back button until the point where going back
//! would mean undoing something. Nothing on any page is a surprise: the last
//! page before anything happens lists exactly what is about to be done, and
//! the page after it shows each of those things as it happens.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;

use crate::install::{self, Receipt, Step};
use crate::job::{self, Choices, ModelChoice, Progress};
use crate::model::{self, Hardware, Runner};
use crate::release::{self, Release};

/// The project's colours, so the installer looks like the thing it installs.
mod paint {
    use eframe::egui::Color32;

    pub const AMBER: Color32 = Color32::from_rgb(255, 176, 0);
    pub const CYAN: Color32 = Color32::from_rgb(34, 224, 226);
    pub const RED: Color32 = Color32::from_rgb(255, 89, 94);
    pub const YELLOW: Color32 = Color32::from_rgb(255, 209, 102);
    pub const DIM: Color32 = Color32::from_rgb(150, 158, 172);
    pub const BACKGROUND: Color32 = Color32::from_rgb(12, 15, 21);
    pub const PANEL: Color32 = Color32::from_rgb(18, 22, 30);
    pub const LINE: Color32 = Color32::from_rgb(35, 42, 54);
}

#[derive(PartialEq, Eq)]
enum Page {
    Welcome,
    Choose,
    Working,
    Done,
}

/// What the list of releases is doing.
enum Releases {
    Loading,
    Ready(Vec<Release>),
    Failed(String),
}

pub struct Setup {
    page: Page,

    releases: Releases,
    releases_inbox: Option<Receiver<Result<Vec<Release>, String>>>,
    chosen: usize,
    show_prereleases: bool,

    directory: String,
    desktop: bool,
    add_to_path: bool,
    shortcut: bool,

    hardware: Hardware,
    runner: Runner,
    want_model: bool,
    may_install_runner: bool,

    log: Vec<(egui::Color32, String)>,
    stage: String,
    download: Option<(String, u64, u64)>,
    progress_inbox: Option<Receiver<Progress>>,
    outcome: Option<Result<Receipt, String>>,
    /// What a previous run of this installer left behind, if anything.
    already: Option<Receipt>,
}

impl Setup {
    pub fn new(context: &eframe::CreationContext<'_>) -> Self {
        style(&context.egui_ctx);

        // Asked for immediately, on its own thread, so the first page is up
        // before the network has answered. An installer that shows nothing
        // until GitHub replies looks broken on a slow connection.
        let (sender, receiver) = channel();
        let ping = context.egui_ctx.clone();
        std::thread::spawn(move || {
            let answer = release::list().map_err(|error| format!("{error:#}"));
            let _ = sender.send(answer);
            ping.request_repaint();
        });

        let hardware = model::look();
        let directory = install::default_directory().unwrap_or_default();
        // Read before anything is decided, so the first page can say what is
        // already there. Somebody who runs this twice should be told which
        // version they have rather than left to work it out.
        let already = Receipt::read(&directory);
        Self {
            already,
            page: Page::Welcome,
            releases: Releases::Loading,
            releases_inbox: Some(receiver),
            chosen: 0,
            show_prereleases: false,
            directory: directory.display().to_string(),
            desktop: true,
            add_to_path: true,
            shortcut: true,
            runner: model::runner(),
            want_model: false,
            may_install_runner: false,
            hardware,
            log: Vec::new(),
            stage: String::new(),
            download: None,
            progress_inbox: None,
            outcome: None,
        }
    }

    fn visible_releases(&self) -> Vec<&Release> {
        match &self.releases {
            Releases::Ready(all) => all
                .iter()
                .filter(|release| self.show_prereleases || !release.prerelease)
                .collect(),
            _ => Vec::new(),
        }
    }

    fn selected(&self) -> Option<&Release> {
        let visible = self.visible_releases();
        visible.get(self.chosen).copied()
    }

    fn start(&mut self, context: &egui::Context) {
        let Some(release) = self.selected().cloned() else {
            return;
        };

        let choices = Choices {
            release,
            directory: PathBuf::from(&self.directory),
            desktop: self.desktop,
            add_to_path: self.add_to_path,
            shortcut: self.shortcut,
            model: if self.want_model {
                ModelChoice::Pull {
                    tag: self.hardware.pick.tag.to_string(),
                    may_install_runner: self.may_install_runner,
                }
            } else {
                ModelChoice::None
            },
        };

        let (sender, receiver) = channel();
        self.progress_inbox = Some(receiver);
        self.log.clear();
        self.outcome = None;
        self.page = Page::Working;

        let ping = context.clone();
        std::thread::spawn(move || {
            let relay: Sender<Progress> = sender;
            job::run(choices, relay);
            ping.request_repaint();
        });
    }

    fn drain(&mut self, context: &egui::Context) {
        if let Some(inbox) = &self.releases_inbox
            && let Ok(answer) = inbox.try_recv()
        {
            self.releases = match answer {
                Ok(list) => Releases::Ready(list),
                Err(reason) => Releases::Failed(reason),
            };
            self.releases_inbox = None;
        }

        let Some(inbox) = &self.progress_inbox else {
            return;
        };
        let mut finished = false;
        while let Ok(message) = inbox.try_recv() {
            match message {
                Progress::Stage(text) => {
                    self.stage = text.clone();
                    self.download = None;
                    self.log.push((paint::AMBER, text));
                }
                Progress::Note(text) => self.log.push((paint::DIM, text)),
                Progress::Warning(text) => self.log.push((paint::YELLOW, text)),
                Progress::Downloading { name, done, total } => {
                    self.download = Some((name, done, total));
                }
                Progress::Finished(receipt) => {
                    self.outcome = Some(Ok(*receipt));
                    finished = true;
                }
                Progress::Failed(reason) => {
                    self.log.push((paint::RED, reason.clone()));
                    self.outcome = Some(Err(reason));
                    finished = true;
                }
            }
            context.request_repaint();
        }
        if finished {
            self.progress_inbox = None;
            self.download = None;
            self.page = Page::Done;
        }
        // Kept ticking while work is in flight, because progress arrives from
        // another thread and egui only redraws when something asks it to.
        if self.progress_inbox.is_some() {
            context.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}

fn style(context: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = paint::BACKGROUND;
    visuals.window_fill = paint::BACKGROUND;
    visuals.extreme_bg_color = paint::PANEL;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, paint::LINE);
    visuals.widgets.inactive.bg_fill = paint::PANEL;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, paint::AMBER);
    visuals.selection.bg_fill = paint::AMBER.gamma_multiply(0.35);
    context.set_visuals(visuals);

    let mut style = (*context.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    context.set_style(style);
}

/// What to tell somebody to do next, given what was actually done.
///
/// Written from the receipt rather than fixed, because the closing line used
/// to offer a shortcut whether or not one had been made, and tell people to
/// run `outlaw` from a terminal whether or not it was on their PATH. Advice
/// that does not work is worse than no advice: somebody follows it, it fails,
/// and now they doubt the install rather than the sentence.
fn next_step(receipt: &Receipt) -> String {
    let on_path = receipt
        .steps
        .iter()
        .any(|step| matches!(step, Step::AddedToPath { .. }));
    let shortcut = receipt
        .steps
        .iter()
        .any(|step| matches!(step, Step::Shortcut { .. }));

    let check = if on_path {
        "Open a new terminal and run `outlaw boot` to check it over.".to_string()
    } else {
        format!(
            "Run `outlaw boot` from {} to check it over -- it was not added to your PATH.",
            receipt.directory
        )
    };

    if shortcut {
        format!("{check} The window is on your Start menu.")
    } else {
        check
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text)
            .size(22.0)
            .color(paint::AMBER)
            .strong(),
    );
    ui.add_space(2.0);
}

fn dim(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(paint::DIM).size(13.0));
}

impl eframe::App for Setup {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain(context);

        egui::TopBottomPanel::top("brand").show(context, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("OUTLAW")
                        .color(paint::AMBER)
                        .strong()
                        .size(18.0),
                );
                ui.label(
                    egui::RichText::new("REPAIR KIT")
                        .color(paint::DIM)
                        .size(13.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("setup v{}", env!("CARGO_PKG_VERSION")))
                            .color(paint::DIM)
                            .size(12.0),
                    );
                });
            });
            ui.add_space(8.0);
        });

        egui::TopBottomPanel::bottom("footer").show(context, |ui| {
            ui.add_space(6.0);
            dim(
                ui,
                "Made by Outlaw Systems, in collaboration with AI. Nothing is installed \
                 that does not match its published checksum.",
            );
            ui.add_space(6.0);
        });

        // Above the footer and below everything else, so that on the page
        // where it matters the plan and the button that carries it out are
        // both on screen at any window size. This used to sit at the end of a
        // scrolling column, which put the one thing somebody must read before
        // pressing Install below the fold, under the Install button.
        if self.page == Page::Choose {
            egui::TopBottomPanel::bottom("plan").show(context, |ui| {
                self.plan_and_buttons(ui, context);
            });
        }

        egui::CentralPanel::default().show(context, |ui| match self.page {
            Page::Welcome => self.welcome(ui),
            Page::Choose => self.choose(ui),
            Page::Working => self.working(ui),
            Page::Done => self.done(ui),
        });
    }
}

impl Setup {
    fn welcome(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Set up the Outlaw Repair Kit");
        dim(
            ui,
            "This puts the tool on your computer. It downloads from the project's own \
             releases, checks every file against the checksum published with it, and \
             refuses to install anything that does not match.",
        );
        ui.add_space(6.0);
        dim(
            ui,
            "Nothing here needs administrator rights. Everything goes in your own \
             account, and a record of what was done is written beside it.",
        );

        if let Some(already) = &self.already {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} is already installed here, from {}. Installing again replaces it.",
                    already.version, already.installed_at
                ))
                .color(paint::CYAN)
                .size(13.0),
            );
        }

        ui.add_space(14.0);
        ui.label(egui::RichText::new("Version").color(paint::CYAN).size(13.0));

        match &self.releases {
            Releases::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    dim(ui, "asking GitHub what has been released…");
                });
            }
            Releases::Failed(reason) => {
                ui.label(
                    egui::RichText::new(format!("Could not reach the releases: {reason}"))
                        .color(paint::RED)
                        .size(13.0),
                );
                ui.add_space(4.0);
                dim(
                    ui,
                    "Check the connection and start this again. Nothing has been changed.",
                );
            }
            Releases::Ready(_) => {
                let visible: Vec<(String, bool)> = self
                    .visible_releases()
                    .iter()
                    .map(|release| (release.tag.clone(), release.prerelease))
                    .collect();

                if visible.is_empty() {
                    dim(ui, "no releases published yet");
                } else {
                    if self.chosen >= visible.len() {
                        self.chosen = 0;
                    }
                    let label = {
                        let (tag, pre) = &visible[self.chosen];
                        let newest = self.chosen == 0 && !pre;
                        format!("{tag}{}", if newest { "  (newest)" } else { "" })
                    };
                    egui::ComboBox::from_id_salt("version")
                        .width(280.0)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (index, (tag, pre)) in visible.iter().enumerate() {
                                let text = if *pre {
                                    format!("{tag}  (pre-release)")
                                } else {
                                    tag.clone()
                                };
                                ui.selectable_value(&mut self.chosen, index, text);
                            }
                        });
                    ui.checkbox(&mut self.show_prereleases, "Show pre-releases");
                }
            }
        }

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let ready = self.selected().is_some();
            if ui
                .add_enabled(ready, egui::Button::new("Continue"))
                .clicked()
            {
                self.page = Page::Choose;
            }
            if !ready {
                dim(ui, "waiting for the list of releases");
            }
        });
    }

    fn choose(&mut self, ui: &mut egui::Ui) {
        heading(ui, "What to install");

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.checkbox(
                    &mut self.desktop,
                    "The window as well as the command-line tool",
                );
                dim(
                    ui,
                    "The window is downloaded and checked here, then left for you to run — \
                     it is its own installer and asks its own questions.",
                );

                ui.add_space(8.0);
                ui.checkbox(&mut self.add_to_path, "Let me run `outlaw` from anywhere");
                ui.checkbox(&mut self.shortcut, "Add a shortcut");

                ui.add_space(10.0);
                ui.label(egui::RichText::new("Where").color(paint::CYAN).size(13.0));
                ui.add(egui::TextEdit::singleline(&mut self.directory).desired_width(460.0));

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("A model on this machine")
                        .color(paint::CYAN)
                        .size(13.0),
                );
                dim(
                    ui,
                    "Optional, and genuinely optional: every check runs and every known \
                     problem is explained with no model at all. A model only helps with \
                     problems that are not in the runbook library.",
                );

                ui.add_space(6.0);
                dim(ui, &self.hardware.summary());

                match &self.runner {
                    Runner::Present { name } => {
                        ui.add_space(6.0);
                        dim(
                            ui,
                            &format!(
                                "{name} is already installed, so nothing needs setting up — \
                                 the tool finds whatever is running on its own."
                            ),
                        );
                    }
                    Runner::Ollama | Runner::None => {
                        ui.add_space(6.0);
                        let pick = &self.hardware.pick;
                        ui.checkbox(
                            &mut self.want_model,
                            format!("Download {} — about {} GB", pick.tag, pick.about_gb),
                        );
                        dim(ui, pick.why);

                        if self.want_model && self.runner == Runner::None {
                            ui.add_space(6.0);
                            match model::install_command() {
                                Some((program, args)) => {
                                    dim(
                                        ui,
                                        "Nothing here can run a model yet. Ollama is what runs \
                                         it. It would be installed with:",
                                    );
                                    ui.label(
                                        egui::RichText::new(model::as_typed(program, &args))
                                            .monospace()
                                            .color(paint::YELLOW)
                                            .size(12.5),
                                    );
                                    ui.checkbox(&mut self.may_install_runner, "Run that command");
                                }
                                None => {
                                    dim(
                                        ui,
                                        "Nothing here can run a model yet, and there is no way \
                                         to install one automatically on this machine. Install \
                                         Ollama or LM Studio yourself — the tool finds whatever \
                                         is running, and needs no configuring.",
                                    );
                                    if ui.link("ollama.com/download").clicked() {
                                        let _ = ork_core::platform::open_url(
                                            "https://ollama.com/download",
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                ui.add_space(8.0);
            });
    }

    /// The plan, and the button that carries it out, side by side and always
    /// visible.
    fn plan_and_buttons(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("What will happen")
                .color(paint::CYAN)
                .size(13.0),
        );
        for line in self.plan_lines() {
            dim(ui, &format!("• {line}"));
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.page = Page::Welcome;
            }
            let ready = !self.directory.trim().is_empty();
            if ui
                .add_enabled(ready, egui::Button::new("Install"))
                .clicked()
            {
                self.start(context);
            }
        });
        ui.add_space(8.0);
    }

    /// Everything that is about to happen, in the order it will happen.
    ///
    /// Written out before anything starts, because "what is this about to do
    /// to my computer" is the question every installer should answer without
    /// being asked.
    fn plan_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let version = self
            .selected()
            .map(|release| release.tag.clone())
            .unwrap_or_else(|| "the chosen release".to_string());

        lines.push(format!(
            "Download the checksums published with {version}, and check everything against them"
        ));
        lines.push(format!(
            "Put {} in {}",
            install::program_name(),
            self.directory
        ));
        if self.desktop {
            lines.push("Download the window's installer and leave it in that folder".into());
        }
        if self.add_to_path {
            lines.push("Add that folder to this account's PATH".into());
        }
        if self.shortcut {
            lines.push("Add a shortcut".into());
        }
        if self.want_model {
            if self.may_install_runner && self.runner == Runner::None {
                lines.push("Install Ollama, using the command shown above".into());
            }
            lines.push(format!(
                "Ask Ollama for {} — about {} GB, and no time limit on it",
                self.hardware.pick.tag, self.hardware.pick.about_gb
            ));
        }
        lines.push("Write down what was done, beside the installed files".into());
        lines
    }

    fn working(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            if self.stage.is_empty() {
                "Working"
            } else {
                &self.stage
            },
        );

        if let Some((name, done, total)) = &self.download {
            let fraction = if *total > 0 {
                *done as f32 / *total as f32
            } else {
                0.0
            };
            ui.add(egui::ProgressBar::new(fraction).text(format!(
                "{name} — {:.1} of {:.1} MB",
                *done as f64 / 1_048_576.0,
                *total as f64 / 1_048_576.0
            )));
        } else {
            ui.horizontal(|ui| {
                ui.spinner();
                dim(ui, "working…");
            });
        }

        ui.add_space(10.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (colour, line) in &self.log {
                    ui.label(egui::RichText::new(line).color(*colour).size(12.5));
                }
            });
    }

    fn done(&mut self, ui: &mut egui::Ui) {
        match self.outcome.clone() {
            Some(Ok(receipt)) => {
                heading(ui, "Installed");
                dim(
                    ui,
                    &format!("{} is in {}", receipt.version, receipt.directory),
                );

                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("What was done")
                        .color(paint::CYAN)
                        .size(13.0),
                );
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(ui.available_height() - 100.0)
                    .show(ui, |ui| {
                        for step in &receipt.steps {
                            dim(ui, &format!("• {}", describe(step)));
                        }
                        ui.add_space(8.0);
                        for (colour, line) in &self.log {
                            if *colour == paint::YELLOW {
                                ui.label(
                                    egui::RichText::new(format!("! {line}"))
                                        .color(paint::YELLOW)
                                        .size(12.5),
                                );
                            }
                        }
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Open the folder").clicked() {
                        open_folder(&receipt.directory);
                    }
                    if ui.button("Close").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.add_space(6.0);
                dim(ui, &next_step(&receipt));
            }
            Some(Err(reason)) => {
                heading(ui, "It stopped");
                ui.label(egui::RichText::new(reason).color(paint::RED).size(13.0));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Back").clicked() {
                        self.page = Page::Choose;
                    }
                    if ui.button("Close").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            }
            None => {
                heading(ui, "Finished");
            }
        }
    }
}

fn describe(step: &Step) -> String {
    match step {
        Step::Wrote { path, .. } => format!("wrote {path}"),
        Step::AddedToPath { directory } => {
            format!("added {directory} to this account's PATH")
        }
        Step::Shortcut { path } => format!("created {path}"),
        Step::Delegated { what, command } => {
            format!("installed {what} by running `{command}`")
        }
    }
}

fn open_folder(directory: &str) {
    #[cfg(windows)]
    let _ = ork_core::platform::run_capture("explorer", &[directory]);
    #[cfg(target_os = "linux")]
    let _ = ork_core::platform::run_capture("xdg-open", &[directory]);
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = directory;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_of_step_can_be_described() {
        // The last page is the only record most people will read. A step that
        // renders as nothing is a step that silently did not happen, as far as
        // anybody looking at that page can tell.
        let steps = [
            Step::Wrote {
                path: "C:/x/outlaw.exe".to_string(),
                sha256: "abc".to_string(),
            },
            Step::AddedToPath {
                directory: "C:/x".to_string(),
            },
            Step::Shortcut {
                path: "C:/y/Outlaw.lnk".to_string(),
            },
            Step::Delegated {
                what: "Ollama".to_string(),
                command: "winget install --id Ollama.Ollama -e".to_string(),
            },
        ];
        for step in &steps {
            let text = describe(step);
            assert!(!text.trim().is_empty(), "{step:?} describes as nothing");
        }
    }

    #[test]
    fn the_digest_never_appears_on_the_finished_page() {
        // It is in the record on disk, where it is useful. On screen it is
        // sixty-four characters of noise across a line somebody is trying to
        // read.
        let described = describe(&Step::Wrote {
            path: "C:/x/outlaw.exe".to_string(),
            sha256: "0123456789abcdef".to_string(),
        });
        assert!(!described.contains("0123456789abcdef"), "{described}");
    }

    fn receipt_with(steps: Vec<Step>) -> Receipt {
        Receipt {
            version: "v0.7.0".to_string(),
            directory: r"C:\Somewhere\Else".to_string(),
            steps,
            ..Default::default()
        }
    }

    #[test]
    fn the_last_word_does_not_offer_a_shortcut_that_was_not_made() {
        // It used to, whatever the choices had been. Advice that does not work
        // is worse than no advice: somebody follows it, nothing happens, and
        // now they doubt the installation rather than the sentence.
        let advice = next_step(&receipt_with(vec![Step::Wrote {
            path: r"C:\Somewhere\Else\outlaw.exe".to_string(),
            sha256: "abc".to_string(),
        }]));
        assert!(!advice.to_lowercase().contains("shortcut"), "{advice}");
        assert!(!advice.to_lowercase().contains("start menu"), "{advice}");
    }

    #[test]
    fn somebody_not_on_the_path_is_told_where_to_run_it_from() {
        // "Open a new terminal and run `outlaw boot`" is a broken instruction
        // for somebody who declined to have it added to their PATH, and it
        // fails in the most confusing way -- command not found, immediately
        // after being told the install worked.
        let advice = next_step(&receipt_with(vec![Step::Wrote {
            path: r"C:\Somewhere\Else\outlaw.exe".to_string(),
            sha256: "abc".to_string(),
        }]));
        assert!(advice.contains(r"C:\Somewhere\Else"), "{advice}");
        assert!(advice.contains("PATH"), "{advice}");
    }

    #[test]
    fn somebody_on_the_path_is_simply_told_to_open_a_terminal() {
        let advice = next_step(&receipt_with(vec![Step::AddedToPath {
            directory: r"C:\Somewhere\Else".to_string(),
        }]));
        assert!(advice.contains("new terminal"), "{advice}");
        assert!(!advice.contains("not added"), "{advice}");
    }

    #[test]
    fn a_shortcut_that_was_made_is_mentioned() {
        let advice = next_step(&receipt_with(vec![
            Step::AddedToPath {
                directory: r"C:\Somewhere\Else".to_string(),
            },
            Step::Shortcut {
                path: r"C:\Start\Menu\Outlaw.lnk".to_string(),
            },
        ]));
        assert!(advice.contains("Start menu"), "{advice}");
    }
}
