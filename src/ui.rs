use crate::{
    audio::{Command, DeviceInfo, Engine, Event, RunningInfo},
    model::{self, Backend, Preset, SharedParameters},
    storage::{self, Session},
};
use eframe::egui::{self, Color32, RichText};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering::Relaxed},
    time::{Duration, Instant},
};

const ACCENT: Color32 = Color32::from_rgb(113, 225, 195);
const MUTED: Color32 = Color32::from_rgb(148, 158, 171);
const CARD: Color32 = Color32::from_rgb(28, 34, 43);

pub struct Studio {
    session: Session,
    path: Option<PathBuf>,
    shared: Arc<SharedParameters>,
    engine: Engine,
    devices: Vec<DeviceInfo>,
    device_note: String,
    status: String,
    running: Option<RunningInfo>,
    pending: bool,
    settings: bool,
    preset_editor: bool,
    preset_name: String,
    rename_mode: bool,
    notice: Option<String>,
    dirty: bool,
    last_change: Instant,
    save_enabled: bool,
    meters: [f32; 2],
    failed: bool,
}
impl Studio {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        cc.egui_ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.spacing.item_spacing = egui::vec2(12.0, 10.0);
            style.spacing.button_padding = egui::vec2(14.0, 9.0);
            style.visuals.panel_fill = Color32::from_rgb(17, 22, 29);
            style.visuals.window_fill = Color32::from_rgb(22, 28, 36);
            style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(40, 49, 61);
            style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(53, 67, 78);
            style.visuals.selection.bg_fill = Color32::from_rgb(39, 112, 96);
            style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
            style.visuals.override_text_color = Some(Color32::from_rgb(231, 236, 241));
            style.spacing.slider_width = 330.0;
        });
        let path = storage::session_path();
        let (session, notice) = path.as_ref().map_or_else(
            || {
                (
                    Session::default(),
                    Some(
                        "Local application-data folder is unavailable. Settings cannot be saved."
                            .into(),
                    ),
                )
            },
            |p| storage::load(p),
        );
        let save_enabled = path.is_some()
            && !notice
                .as_ref()
                .is_some_and(|n| n.contains("Saving is disabled"));
        let shared = Arc::new(SharedParameters::new(session.parameters));
        let engine = Engine::new(shared.clone());
        Self {
            session,
            path,
            shared,
            engine,
            devices: Vec::new(),
            device_note: String::new(),
            status: "Connect your guitar. Make room for a little clarity.".into(),
            running: None,
            pending: false,
            settings: std::env::args().any(|arg| arg == "--setup"),
            preset_editor: false,
            preset_name: String::new(),
            rename_mode: false,
            notice,
            dirty: false,
            last_change: Instant::now(),
            save_enabled,
            meters: [0.0; 2],
            failed: false,
        }
    }
    fn changed(&mut self) {
        self.dirty = true;
        self.last_change = Instant::now();
        self.shared.publish(self.session.parameters);
    }
    fn persist(&mut self) {
        if !self.dirty || !self.save_enabled {
            return;
        }
        if let Some(path) = &self.path {
            match storage::save(path, &self.session) {
                Ok(()) => self.dirty = false,
                Err(e) => {
                    self.notice = Some(format!("Could not save settings: {e:#}"));
                    self.last_change = Instant::now();
                }
            }
        }
    }
    fn request_start(&mut self) {
        if self.engine.send(Command::Start(self.session.audio.clone())) {
            self.pending = true;
            self.status = "Opening selected audio devices…".into();
            self.failed = false;
        } else {
            self.notice = Some("Audio control is busy. Try again shortly.".into());
        }
    }
    fn poll(&mut self) {
        while let Ok(event) = self.engine.events.try_recv() {
            match event {
                Event::Devices(devices, note) => {
                    self.devices = devices;
                    self.device_note = note;
                    if self.session.audio.input_id.is_empty()
                        && self.session.audio.output_id.is_empty()
                    {
                        // Only suggest a recognized guitar interface; never auto-select a PC microphone.
                        let candidate = self.devices.iter().find(|d| {
                            let name = d.name.to_ascii_lowercase();
                            (name.contains("tonex") || name.contains("ik multimedia"))
                                && d.input_channels > 0
                        });
                        if let Some(input) = candidate {
                            self.session.audio.backend = input.backend;
                            self.session.audio.input_id = input.id.clone();
                            if input.output_channels >= 2 {
                                self.session.audio.output_id = input.id.clone();
                            } else if let Some(output) = self.devices.iter().find(|d| {
                                d.backend == input.backend
                                    && d.output_channels >= 2
                                    && (d.name.to_ascii_lowercase().contains("tonex")
                                        || d.name.to_ascii_lowercase().contains("ik multimedia"))
                            }) {
                                self.session.audio.output_id = output.id.clone();
                            }
                            self.changed();
                        } else if !self.devices.iter().any(|d| d.backend == Backend::Asio) {
                            self.session.audio.backend = Backend::Wasapi;
                        }
                    }
                }
                Event::Running(info) => {
                    self.running = Some(info);
                    self.pending = false;
                    self.failed = false;
                    self.status = "Monitoring · your guitar, with a little more space.".into();
                }
                Event::Stopped => {
                    self.running = None;
                    self.pending = false;
                    self.status = "Monitoring stopped.".into();
                }
                Event::Failed(message) => {
                    self.running = None;
                    self.pending = false;
                    self.failed = true;
                    self.status = message;
                }
            }
        }
        let d = &self.engine.diagnostics;
        for (meter, atomic) in self.meters.iter_mut().zip([&d.input_peak, &d.output_peak]) {
            *meter = (*meter * 0.82).max(f32::from_bits(atomic.swap(0, Relaxed)));
        }
    }
    fn draw_main(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(34.0, 38.0), egui::Sense::hover());
            for (i, height) in [14.0, 30.0, 38.0, 22.0].iter().enumerate() {
                let left = rect.left() + i as f32 * 8.0;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(left, rect.center().y - height / 2.0),
                        egui::vec2(4.0, *height),
                    ),
                    2.0,
                    ACCENT,
                );
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("shredinini").size(29.0).strong());
                ui.label(
                    RichText::new("A LITTLE GUITAR STUDIO")
                        .size(10.0)
                        .color(MUTED)
                        .extra_letter_spacing(2.0),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Audio & setup").clicked() {
                    self.settings = true;
                }
                let label = if self.running.is_some() {
                    "LIVE"
                } else {
                    "STANDBY"
                };
                ui.label(
                    RichText::new(label)
                        .size(11.0)
                        .color(if self.running.is_some() {
                            ACCENT
                        } else {
                            MUTED
                        }),
                );
            });
        });
        ui.add_space(22.0);
        egui::Frame::new()
            .fill(CARD)
            .corner_radius(14)
            .inner_margin(22)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    RichText::new("CURRENT PRESET")
                        .size(10.0)
                        .color(ACCENT)
                        .extra_letter_spacing(1.6),
                );
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&self.session.current_preset)
                            .size(34.0)
                            .strong(),
                    );
                    if self.preset_modified() {
                        ui.label(RichText::new("edited").size(11.0).color(MUTED));
                    }
                });
                ui.label(
                    RichText::new("Bright, smooth, melodic guitar. Make it yours.").color(MUTED),
                );
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    let mut selected = None;
                    egui::ComboBox::from_id_salt("presets")
                        .selected_text("Choose a sound")
                        .width(180.0)
                        .show_ui(ui, |ui| {
                            ui.weak("STUDIO PRESETS");
                            for p in model::factory_presets() {
                                if ui
                                    .selectable_label(
                                        self.session.current_preset == p.name,
                                        &p.name,
                                    )
                                    .clicked()
                                {
                                    selected = Some(p);
                                }
                            }
                            if !self.session.user_presets.is_empty() {
                                ui.separator();
                                ui.weak("YOUR PRESETS");
                            }
                            for p in &self.session.user_presets {
                                if ui
                                    .selectable_label(
                                        self.session.current_preset == p.name,
                                        &p.name,
                                    )
                                    .clicked()
                                {
                                    selected = Some(p.clone());
                                }
                            }
                        });
                    if let Some(p) = selected {
                        // Presets describe tone; monitoring gain, trim and bypass remain session controls.
                        let master = self.session.parameters.output_db;
                        let trim = self.session.parameters.input_trim_db;
                        let bypass = self.session.parameters.bypass;
                        self.session.current_preset = p.name;
                        self.session.parameters = p.parameters;
                        self.session.parameters.output_db = master;
                        self.session.parameters.input_trim_db = trim;
                        self.session.parameters.bypass = bypass;
                        self.changed();
                    }
                    if ui.button("Save as…").clicked() {
                        self.rename_mode = false;
                        self.preset_name.clear();
                        self.preset_editor = true;
                    }
                    let is_user = self
                        .session
                        .user_presets
                        .iter()
                        .any(|p| p.name == self.session.current_preset);
                    if ui
                        .add_enabled(
                            is_user && self.preset_modified(),
                            egui::Button::new("Update"),
                        )
                        .clicked()
                    {
                        if let Some(p) = self
                            .session
                            .user_presets
                            .iter_mut()
                            .find(|p| p.name == self.session.current_preset)
                        {
                            p.parameters = self.session.parameters;
                        }
                        self.changed();
                        self.persist();
                    }
                    if ui
                        .add_enabled(is_user, egui::Button::new("Rename"))
                        .clicked()
                    {
                        self.rename_mode = true;
                        self.preset_name = self.session.current_preset.clone();
                        self.preset_editor = true;
                    }
                    if ui
                        .add_enabled(is_user, egui::Button::new("Delete"))
                        .clicked()
                    {
                        self.session
                            .user_presets
                            .retain(|p| p.name != self.session.current_preset);
                        self.session.current_preset = "Unsaved tone".into();
                        self.changed();
                    }
                });
            });
        ui.add_space(16.0);
        let before = self.session.parameters;
        egui::Frame::new()
            .fill(CARD)
            .corner_radius(14)
            .inner_margin(22)
            .show(ui, |ui| {
                control(
                    ui,
                    "Tone",
                    "Warm and rounded to open and bright",
                    &mut self.session.parameters.tone,
                );
                control(
                    ui,
                    "Warmth / Drive",
                    "A little body. A softer edge.",
                    &mut self.session.parameters.warmth,
                );
                control(
                    ui,
                    "Reverb",
                    "A smooth stereo room around each note.",
                    &mut self.session.parameters.reverb,
                );
                control(
                    ui,
                    "Delay",
                    "A quiet echo that follows your melody.",
                    &mut self.session.parameters.delay,
                );
                ui.add_space(4.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Compression").size(16.0).strong());
                    ui.label(
                        RichText::new("Even notes, natural touch")
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.checkbox(&mut self.session.parameters.compression, "Enabled");
                    });
                });
            });
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let ready =
                !self.session.audio.input_id.is_empty() && !self.session.audio.output_id.is_empty();
            if self.running.is_some() {
                if ui
                    .add_enabled(
                        !self.pending,
                        egui::Button::new(RichText::new("Stop").size(17.0))
                            .min_size(egui::vec2(135.0, 44.0)),
                    )
                    .clicked()
                    && self.engine.send(Command::Stop)
                {
                    self.pending = true;
                }
            } else if ui
                .add_enabled(
                    !self.pending && ready,
                    egui::Button::new(
                        RichText::new(if self.failed { "Reconnect" } else { "Start" })
                            .size(17.0)
                            .color(Color32::from_rgb(14, 32, 29)),
                    )
                    .fill(ACCENT)
                    .min_size(egui::vec2(135.0, 44.0)),
                )
                .clicked()
            {
                self.request_start();
            }
            ui.checkbox(&mut self.session.parameters.bypass, "Bypass effects");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::Slider::new(&mut self.session.parameters.output_db, -60.0..=0.0)
                        .suffix(" dB")
                        .fixed_decimals(0),
                );
                ui.label("Output");
            });
        });
        if before != self.session.parameters {
            self.changed();
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            meter(ui, "IN", self.meters[0]);
            meter(ui, "OUT", self.meters[1]);
            let d = &self.engine.diagnostics;
            if d.input_clips.load(Relaxed) > 0 {
                ui.colored_label(Color32::LIGHT_RED, "Input clipped · lower hardware gain");
            } else if d.output_clips.load(Relaxed) > 0 {
                ui.colored_label(Color32::YELLOW, "Output protected · lower volume");
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(&self.status)
                .size(12.0)
                .color(if self.failed {
                    Color32::LIGHT_RED
                } else {
                    MUTED
                }),
        );
        if self.session.audio.input_id.is_empty() || self.session.audio.output_id.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("First time here?").color(ACCENT));
                if ui.link("Select your guitar input and output").clicked() {
                    self.settings = true;
                }
            });
        }
        if let Some(info) = &self.running {
            ui.label(
                RichText::new(format!(
                    "{} Hz → {} Hz  ·  {}  ·  app queue + resampler ≈ {:.1} ms",
                    info.input_rate,
                    info.output_rate,
                    self.session.audio.backend.label(),
                    f32::from_bits(self.engine.diagnostics.app_latency_ms.load(Relaxed))
                ))
                .size(11.0)
                .color(MUTED),
            );
        }
        if let Some(notice) = self.notice.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(Color32::from_rgb(241, 197, 135), notice);
                if ui.small_button("Dismiss").clicked() {
                    self.notice = None;
                }
            });
        }
    }
    fn preset_modified(&self) -> bool {
        let preset = model::factory_presets()
            .into_iter()
            .chain(self.session.user_presets.clone())
            .find(|p| p.name == self.session.current_preset);
        preset.is_none_or(|p| {
            let a = p.parameters;
            let b = self.session.parameters;
            a.tone != b.tone
                || a.warmth != b.warmth
                || a.reverb != b.reverb
                || a.delay != b.delay
                || a.compression != b.compression
        })
    }
    fn draw_settings(&mut self, ctx: &egui::Context) {
        let mut open = self.settings;
        let before = self.session.audio.clone();
        let trim = self.session.parameters.input_trim_db;
        egui::Window::new("Audio & setup").open(&mut open).default_width(620.0).default_height(720.0).max_height((ctx.viewport_rect().height()-70.0).max(200.0)).resizable(true).show(ctx,|ui|{
            egui::ScrollArea::vertical().max_height(620.0).show(ui,|ui|{
                ui.heading("Connect. Set your level. Play.");
                ui.label("1. Connect guitar → TONEX ONE input, then USB → computer.");
                ui.label("2. In IK’s TONEX Editor, bypass pedal processing (Stomp mode) and turn direct monitoring off.");
                ui.label("3. For ASIO, install IK’s TONEX ONE driver from IK Product Manager → Hardware.");
                ui.label("4. Choose TONEX output for headphones on the pedal, or WASAPI for a wired computer output.");
                ui.hyperlink_to("IK’s TONEX ONE setup guide","https://www.ikmultimedia.com/contact-support/quickfaq.php?id=1298&quick=QUICKHW");
                ui.separator();
                ui.add_enabled_ui(self.running.is_none() && !self.pending,|ui|{
                    egui::ComboBox::from_id_salt("backend").selected_text(self.session.audio.backend.label()).show_ui(ui,|ui|{
                        ui.selectable_value(&mut self.session.audio.backend,Backend::Asio,Backend::Asio.label());
                        ui.selectable_value(&mut self.session.audio.backend,Backend::Wasapi,Backend::Wasapi.label());
                    });
                    let backend=self.session.audio.backend;
                    device_picker(ui,"Guitar input",&self.devices,backend,true,&mut self.session.audio.input_id);
                    if backend==Backend::Asio {
                        self.session.audio.output_id=self.session.audio.input_id.clone();
                        ui.weak("ASIO uses the same interface for capture and playback.");
                    } else {device_picker(ui,"Stereo output",&self.devices,backend,false,&mut self.session.audio.output_id);}
                    let channels=self.devices.iter().find(|d|d.id==self.session.audio.input_id && d.backend==backend).map_or(2,|d|d.input_channels).max(1);
                    ui.horizontal(|ui|{
                        ui.label("Guitar channel");
                        let mut channel=self.session.audio.input_channel+1;
                        ui.add(egui::DragValue::new(&mut channel).range(1..=channels));
                        self.session.audio.input_channel=channel-1;
                    });
                    egui::ComboBox::from_id_salt("buffer").selected_text(buffer_label(self.session.audio.buffer_frames)).show_ui(ui,|ui|{
                        for n in [128,256,512,0] {ui.selectable_value(&mut self.session.audio.buffer_frames,n,buffer_label(n));}
                    });
                    ui.weak("Fixed sizes are requests; unsupported sizes fall back to the driver default.");
                    if ui.button("Refresh devices").clicked(){self.engine.send(Command::Refresh);}
                });
                if self.running.is_some() {ui.weak("Stop monitoring to change audio devices or buffer size.");}
                ui.add(egui::Slider::new(&mut self.session.parameters.input_trim_db,-24.0..=24.0).text("Input trim").suffix(" dB").fixed_decimals(1));
                ui.weak("Digital trim cannot repair clipping at the pedal’s input. Use the pedal’s input trim first.");
                if !self.device_note.is_empty(){ui.colored_label(Color32::from_rgb(241,197,135),&self.device_note);}
                ui.separator();ui.heading("Diagnostics");
                let d=&self.engine.diagnostics;
                ui.label(format!("Underruns: {}  ·  Dropped input frames: {}  ·  Driver xruns: {}  ·  Resyncs: {}",
                    d.underruns.load(Relaxed),d.overruns.load(Relaxed),d.xruns.load(Relaxed),d.resyncs.load(Relaxed)));
                ui.label(format!("Input queue: {} frames  ·  Clock correction: {:+.0} ppm",d.queue_frames.load(Relaxed),f32::from_bits(d.correction_ppm.load(Relaxed))));
                if let Some(info)=&self.running {
                    ui.label(format!("Input: {} Hz / {} ch / {} / {}",info.input_rate,info.input_channels,info.input_format,actual_buffer(info.input_buffer)));
                    ui.label(format!("Output: {} Hz / {} ch / {} / {}",info.output_rate,info.output_channels,info.output_format,actual_buffer(info.output_buffer)));
                    ui.label(if info.resampling {"Adaptive sinc conversion: active"}else{"Adaptive sinc conversion: off (shared ASIO clock)"});
                    let driver_ms=info.input_buffer.zip(info.output_buffer).map(|(i,o)|1000.0*(i as f32/info.input_rate as f32+o as f32/info.output_rate as f32));
                    if let Some(ms)=driver_ms {ui.label(format!("Reported driver buffer duration (input + output): {ms:.1} ms"));}
                }
                ui.weak("App latency covers queued samples, chunk staging and the resampler. Driver buffers, USB transport and converters add latency. This is not a measured round-trip value.");
                if d.realtime_denied.load(Relaxed) {ui.colored_label(Color32::YELLOW,"Windows denied real-time priority; audio continues at normal priority.");}
                ui.weak("If you hear crackles, stop monitoring and try 256 or 512 frames. Separate devices typically add more latency than the TONEX headphone output.");
                if let Some(path)=&self.path {ui.separator();ui.weak(format!("Settings: {}",path.display()));}
            });
        });
        self.settings = open;
        if self.session.audio != before || self.session.parameters.input_trim_db != trim {
            self.changed();
        }
    }
    fn draw_preset_editor(&mut self, ctx: &egui::Context) {
        let mut open = self.preset_editor;
        egui::Window::new(if self.rename_mode {
            "Rename your preset"
        } else {
            "Save your sound"
        })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label("Preset name");
            ui.text_edit_singleline(&mut self.preset_name);
            let name = self.preset_name.trim();
            let duplicate = model::factory_presets()
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(name))
                || self.session.user_presets.iter().any(|p| {
                    p.name.eq_ignore_ascii_case(name)
                        && (!self.rename_mode || p.name != self.session.current_preset)
                });
            let valid = !name.is_empty()
                && name.chars().count() <= 60
                && !name.chars().any(char::is_control)
                && !duplicate;
            if duplicate {
                ui.colored_label(Color32::YELLOW, "That name is already in use.");
            }
            ui.weak("Up to 60 characters. Use a new name to save a variation.");
            if ui
                .add_enabled(valid, egui::Button::new("Save preset"))
                .clicked()
            {
                let name = name.to_owned();
                if self.rename_mode {
                    if let Some(p) = self
                        .session
                        .user_presets
                        .iter_mut()
                        .find(|p| p.name == self.session.current_preset)
                    {
                        p.name = name.clone();
                    }
                } else {
                    self.session.user_presets.push(Preset {
                        name: name.clone(),
                        parameters: self.session.parameters,
                    });
                }
                self.session.current_preset = name;
                self.preset_editor = false;
                self.changed();
                self.persist();
            }
        });
        self.preset_editor = self.preset_editor && open;
    }
}
impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(17, 22, 29))
                    .inner_margin(28),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.draw_main(ui);
                });
            });
        let ctx = ui.ctx().clone();
        if self.settings {
            self.draw_settings(&ctx);
        }
        if self.preset_editor {
            self.draw_preset_editor(&ctx);
        }
        if self.dirty && self.last_change.elapsed() > Duration::from_millis(700) {
            self.persist();
        }
        ctx.request_repaint_after(Duration::from_millis(if self.running.is_some() {
            33
        } else {
            100
        }));
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist();
        self.engine.send(Command::Stop);
    }
}

fn control(ui: &mut egui::Ui, title: &str, subtitle: &str, value: &mut f32) {
    ui.horizontal(|ui| {
        let label = ui
            .allocate_ui_with_layout(
                egui::vec2(245.0, 50.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(245.0);
                    let label = ui.label(RichText::new(title).size(17.0).strong()).id;
                    ui.label(RichText::new(subtitle).size(11.0).color(MUTED));
                    label
                },
            )
            .inner;
        let width = (ui.available_width() - 65.0).max(80.0);
        ui.style_mut().spacing.slider_width = width;
        ui.add(egui::Slider::new(value, 0.0..=1.0).show_value(false))
            .on_hover_text(title)
            .labelled_by(label);
        ui.label(
            RichText::new(format!("{:02.0}%", *value * 100.0))
                .size(19.0)
                .color(ACCENT)
                .monospace(),
        );
    });
}
fn meter(ui: &mut egui::Ui, label: &str, peak: f32) {
    ui.label(RichText::new(label).size(10.0).color(MUTED));
    let db = 20.0 * peak.max(1e-6).log10();
    ui.add(
        egui::ProgressBar::new(((db + 60.0) / 60.0).clamp(0.0, 1.0))
            .desired_width(120.0)
            .desired_height(5.0)
            .fill(if peak >= 0.99 {
                Color32::LIGHT_RED
            } else {
                ACCENT
            }),
    );
}
fn device_picker(
    ui: &mut egui::Ui,
    label: &str,
    devices: &[DeviceInfo],
    backend: Backend,
    input: bool,
    selected: &mut String,
) {
    ui.label(label);
    let name = devices
        .iter()
        .find(|d| d.id == *selected && d.backend == backend)
        .map(|d| d.name.as_str())
        .unwrap_or(if selected.is_empty() {
            "Select a device…"
        } else {
            "Saved device unavailable"
        });
    egui::ComboBox::from_id_salt(label)
        .selected_text(name)
        .width(520.0)
        .show_ui(ui, |ui| {
            for d in devices.iter().filter(|d| {
                d.backend == backend
                    && if input {
                        d.input_channels > 0
                    } else {
                        d.output_channels >= 2
                    }
            }) {
                ui.selectable_value(selected, d.id.clone(), &d.name);
            }
        });
}
fn buffer_label(n: u32) -> String {
    if n == 0 {
        "Driver default buffer".into()
    } else {
        format!("{n} frames requested")
    }
}
fn actual_buffer(n: Option<u32>) -> String {
    n.map_or_else(
        || "buffer not reported".into(),
        |n| format!("{n} frames actual"),
    )
}
