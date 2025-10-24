use crate::{
    error::backtraced_err,
    terminal_emulator::{
        ControlAction, LoadRecordingError, LoadSnapshotError, PtyIo, Recording, RecordingHandle,
        ReplayControl, ReplayIo, TerminalEmulator,
    },
};
use eframe::egui::{self, CentralPanel};
use terminal::TerminalWidget;
use thiserror::Error;

use std::path::{Path, PathBuf};

mod terminal;

fn set_egui_options(ctx: &egui::Context) {
    ctx.options_mut(|options| {
        options.zoom_with_keyboard = true;
    });

    let mut style = (*ctx.style()).clone();
    style.visuals.window_rounding = 8.0.into();
    style.visuals.window_shadow.blur = 16.0;
    ctx.set_style(style);
}

struct LoadReplayResponse {
    terminal_emulator: TerminalEmulator<ReplayIo>,
    replay_control: ReplayControl,
}

#[derive(Debug, Error)]
enum LoadReplayError {
    #[error("failed to load recording")]
    Recording(LoadRecordingError),
    #[error("failed to construct terminal emulator")]
    CreateTerminalEmulator(LoadSnapshotError),
}

fn load_replay(path: &Path) -> Result<LoadReplayResponse, LoadReplayError> {
    let recording = Recording::load(path).map_err(LoadReplayError::Recording)?;
    let mut replay_control = ReplayControl::new(recording);
    let io_handle = replay_control.io_handle();
    let snapshot = replay_control.initial_state();
    let terminal_emulator = TerminalEmulator::from_snapshot(snapshot, io_handle)
        .map_err(LoadReplayError::CreateTerminalEmulator)?;
    Ok(LoadReplayResponse {
        terminal_emulator,
        replay_control,
    })
}

struct ReplayTermieGui {
    terminal_emulator: TerminalEmulator<ReplayIo>,
    terminal_widget: TerminalWidget,
    replay_path: PathBuf,
    replay_control: ReplayControl,
    slider_pos: usize,
}

impl ReplayTermieGui {
    fn new(
        cc: &eframe::CreationContext<'_>,
        replay_path: PathBuf,
        terminal_emulator: TerminalEmulator<ReplayIo>,
        replay_control: ReplayControl,
    ) -> Self {
        set_egui_options(&cc.egui_ctx);

        ReplayTermieGui {
            terminal_emulator,
            terminal_widget: TerminalWidget::new(&cc.egui_ctx),
            replay_path,
            replay_control,
            slider_pos: 0,
        }
    }

    fn step_replay(&mut self) {
        let action = self.replay_control.next();
        match action {
            ControlAction::Resize { width, height } => {
                if let Err(e) = self.terminal_emulator.set_win_size(width, height) {
                    error!("failed to set window size: {}", backtraced_err(&*e));
                }
            }
            ControlAction::None => (),
        }
    }
}

impl eframe::App for ReplayTermieGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let current_pos = self.replay_control.current_pos();
        if current_pos > self.slider_pos {
            match load_replay(&self.replay_path) {
                Ok(response) => {
                    self.terminal_emulator = response.terminal_emulator;
                    self.replay_control = response.replay_control;
                }
                Err(e) => {
                    error!("failed to reload replay: {}", backtraced_err(&e));
                }
            }
        }

        let current_pos = self.replay_control.current_pos();
        if current_pos < self.slider_pos {
            for _ in 0..self.slider_pos - current_pos {
                self.step_replay();
            }
        }

        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame {
                    fill: ctx.style().visuals.faint_bg_color,
                    ..Default::default()
                }
                .inner_margin(8.0),
            )
            .show(ctx, |ui| {
                if ui.button("next").clicked() {
                    self.step_replay();
                    self.slider_pos += 1;
                }
            });

        egui::TopBottomPanel::bottom("seek")
            .frame(
                egui::Frame {
                    fill: ctx.style().visuals.panel_fill,
                    ..Default::default()
                }
                .inner_margin(8.0),
            )
            .show(ctx, |ui| {
                ui.style_mut().spacing.slider_width = ui.available_width();
                let slider =
                    egui::Slider::new(&mut self.slider_pos, 0..=self.replay_control.len() - 1)
                        .show_value(false)
                        .clamping(egui::SliderClamping::Always);
                ui.add(slider);
            });

        let panel_response = CentralPanel::default().show(ctx, |ui| {
            self.terminal_widget.show(ui, &mut self.terminal_emulator);
        });

        panel_response.response.context_menu(|ui| {
            self.terminal_widget.show_options(ui);
        });
    }
}

struct TermieGui {
    terminal_emulator: TerminalEmulator<PtyIo>,
    terminal_widget: TerminalWidget,
    recording_handle: Option<RecordingHandle>,
    show_debug_panel: bool,
}

impl TermieGui {
    fn new(cc: &eframe::CreationContext<'_>, terminal_emulator: TerminalEmulator<PtyIo>) -> Self {
        set_egui_options(&cc.egui_ctx);

        TermieGui {
            terminal_emulator,
            terminal_widget: TerminalWidget::new(&cc.egui_ctx),
            recording_handle: None,
            show_debug_panel: true,
        }
    }
}

impl eframe::App for TermieGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.show_debug_panel {
            egui::SidePanel::right("debug_panel")
                .default_width(200.0)
                .min_width(200.0)
                .max_width(600.0)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);

                            if let Some(last_input) = self.terminal_widget.last_keystroke() {
                                ui.add_space(10.0);
                                ui.monospace(
                                    egui::RichText::new("┌─────────────┐")
                                        .size(16.0)
                                        .color(egui::Color32::from_rgb(100, 150, 200)),
                                );
                                ui.monospace(
                                    egui::RichText::new("│  Keyboard   │")
                                        .size(16.0)
                                        .color(egui::Color32::from_rgb(100, 150, 200)),
                                );
                                ui.monospace(
                                    egui::RichText::new("└─────────────┘")
                                        .size(16.0)
                                        .color(egui::Color32::from_rgb(100, 150, 200)),
                                );

                                ui.add_space(8.0);
                                ui.monospace(
                                    egui::RichText::new("      ↓")
                                        .size(20.0)
                                        .color(egui::Color32::from_rgb(150, 200, 150)),
                                );
                                ui.add_space(8.0);

                                ui.monospace(
                                    egui::RichText::new(format!("   [{}]", last_input))
                                        .size(18.0)
                                        .color(egui::Color32::from_rgb(255, 200, 100))
                                        .strong(),
                                );

                                ui.add_space(8.0);
                                ui.monospace(
                                    egui::RichText::new("      ↓")
                                        .size(20.0)
                                        .color(egui::Color32::from_rgb(150, 200, 150)),
                                );
                                ui.add_space(8.0);

                                ui.monospace(
                                    egui::RichText::new("┌─────────────┐")
                                        .size(16.0)
                                        .color(egui::Color32::from_rgb(200, 100, 150)),
                                );
                                ui.monospace(
                                    egui::RichText::new("│     PTY     │")
                                        .size(16.0)
                                        .color(egui::Color32::from_rgb(200, 100, 150)),
                                );
                                ui.monospace(
                                    egui::RichText::new("└─────────────┘")
                                        .size(16.0)
                                        .color(egui::Color32::from_rgb(200, 100, 150)),
                                );
                            } else {
                                ui.add_space(40.0);
                            }
                        });
                    });
                });
        }

        let panel_response = CentralPanel::default().show(ctx, |ui| {
            let (width_chars, height_chars) = self.terminal_widget.calculate_available_size(ui);

            if let Err(e) = self
                .terminal_emulator
                .set_win_size(width_chars, height_chars)
            {
                error!("failed to set window size {}", backtraced_err(&*e));
            }

            self.terminal_widget.show(ui, &mut self.terminal_emulator);
        });

        panel_response.response.context_menu(|ui| {
            self.terminal_widget.show_options(ui);

            ui.separator();
            ui.checkbox(&mut self.show_debug_panel, "Show Debug Panel");

            if self.recording_handle.is_some() {
                if ui.button("Stop recording").clicked() {
                    self.recording_handle = None;
                }
            } else if ui.button("Start recording").clicked() {
                match self.terminal_emulator.start_recording() {
                    Ok(v) => {
                        self.recording_handle = Some(v);
                    }
                    Err(e) => {
                        error!("failed to start recording: {}", backtraced_err(&e));
                    }
                }
            }
        });
    }
}

pub fn run_replay(replay_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 650.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    let LoadReplayResponse {
        terminal_emulator,
        replay_control,
    } = load_replay(&replay_path)?;

    eframe::run_native(
        "Termie",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(ReplayTermieGui::new(
                cc,
                replay_path,
                terminal_emulator,
                replay_control,
            )))
        }),
    )?;

    Ok(())
}

pub fn run(terminal_emulator: TerminalEmulator<PtyIo>) -> Result<(), Box<dyn std::error::Error>> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([950.0, 670.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Termie",
        native_options,
        Box::new(move |cc| Ok(Box::new(TermieGui::new(cc, terminal_emulator)))),
    )?;
    Ok(())
}
