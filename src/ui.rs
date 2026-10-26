use crate::{
    audio::{Command, DeviceInfo, Engine, Event, RunningInfo},
    media, midi,
    model::{self, Backend, InstrumentSource, Preset, SharedParameters, SynthSound},
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
mod studio_view;

struct AssetView {
    path: PathBuf,
    name: String,
    duration: f64,
    rate: u32,
    waveform: Vec<f32>,
}

pub struct Studio {
    importer: media::Importer,
    importing: usize,
    backing_view: Option<AssetView>,
    sample_view: Option<AssetView>,
    bank_name: String,
    search: String,
    category: String,
    page: u8,
    pending_drop: Option<PathBuf>,
    play_after_start: bool,
    session: Session,
    path: Option<PathBuf>,
    shared: Arc<SharedParameters>,
    engine: Engine,
    devices: Vec<DeviceInfo>,
    midi_ports: Vec<midi::Port>,
    midi_note: String,
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
    audition_after_start: bool,
}
impl Studio {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("studio".into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "studio".into());
            cc.egui_ctx.set_fonts(fonts);
        }
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
        let (mut session, notice) = path.as_ref().map_or_else(
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
        if std::env::args().any(|arg| arg == "--keyboard") {
            session.audio.source = InstrumentSource::MidiKeyboard;
        }
        let save_enabled = path.is_some()
            && !notice
                .as_ref()
                .is_some_and(|n| n.contains("Saving is disabled"));
        let shared = Arc::new(SharedParameters::new(session.parameters));
        let engine = Engine::new(shared.clone());
        engine.media.publish(&session.media);
        let importer = media::Importer::new();
        let mut importing = 0;
        for (role, path) in [
            (media::Role::Backing, &session.backing_path),
            (media::Role::Sample, &session.sample_path),
            (media::Role::SoundFont, &session.soundfont_path),
        ] {
            if let Some(path) = path
                && importer.import(role, Some(path.clone()))
            {
                importing += 1;
            }
        }
        Self {
            importer,
            importing,
            backing_view: None,
            sample_view: None,
            bank_name: "GeneralUser GS · factory bank".into(),
            search: String::new(),
            category: "All sounds".into(),
            page: 0,
            pending_drop: None,
            play_after_start: false,
            session,
            path,
            shared,
            engine,
            devices: Vec::new(),
            midi_ports: Vec::new(),
            midi_note: String::new(),
            device_note: String::new(),
            status: "Choose your instrument, then press Start.".into(),
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
            audition_after_start: false,
        }
    }
    fn changed(&mut self) {
        self.dirty = true;
        self.last_change = Instant::now();
        self.shared.publish(self.session.parameters);
        self.engine.midi.set_sound(self.session.audio.synth_sound);
        self.engine.media.publish(&self.session.media);
    }
    fn prepare_keyboard(&mut self) {
        if self.session.audio.midi_port.is_empty()
            && let Some(port) = self
                .midi_ports
                .iter()
                .find(|p| p.name.to_lowercase().contains("mpk"))
                .or(self.midi_ports.first())
        {
            self.session.audio.midi_port = port.id.clone();
        }
        if self.session.audio.output_id.is_empty()
            && let Some(output) = self
                .devices
                .iter()
                .find(|d| {
                    d.backend == Backend::Wasapi && d.default_output && d.output_channels >= 2
                })
                .or_else(|| {
                    self.devices
                        .iter()
                        .find(|d| d.backend == Backend::Wasapi && d.output_channels >= 2)
                })
        {
            self.session.audio.backend = Backend::Wasapi;
            self.session.audio.output_id = output.id.clone();
        }
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
        let mut config = self.session.audio.clone();
        if self.audition_after_start
            && !config.source.has_audio()
            && !self.midi_ports.iter().any(|p| p.id == config.midi_port)
        {
            config.midi_port.clear();
            self.notice = Some(
                "Preview mode. Connect your MIDI keyboard and restart audio to play it live."
                    .into(),
            );
        }
        if self.engine.send(Command::Start(config)) {
            self.pending = true;
            self.status = "Opening selected audio devices…".into();
            self.failed = false;
        } else {
            self.notice = Some("Audio control is busy. Try again shortly.".into());
        }
    }
    fn poll(&mut self) {
        self.poll_imports();
        while let Ok(event) = self.engine.events.try_recv() {
            match event {
                Event::Notice(message) => self.notice = Some(message),
                Event::Devices(devices, note) => {
                    self.devices = devices;
                    self.device_note = note;
                    if self.session.audio.source == InstrumentSource::Guitar
                        && self.session.audio.input_id.is_empty()
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
                Event::MidiDevices(ports, note) => {
                    self.midi_ports = ports;
                    self.midi_note = note;
                    if self.session.audio.source == InstrumentSource::Guitar
                        && self.session.audio.input_id.is_empty()
                        && self
                            .midi_ports
                            .iter()
                            .any(|p| p.name.to_lowercase().contains("mpk"))
                    {
                        self.session.audio.source = InstrumentSource::MidiKeyboard;
                    }
                    if self.session.audio.source.has_midi() {
                        self.prepare_keyboard();
                        self.changed();
                    }
                }
                Event::Running(info) => {
                    if self.play_after_start {
                        self.engine.media.playing.store(true, Relaxed);
                        self.play_after_start = false;
                    }
                    if self.audition_after_start {
                        self.engine.midi.test_sound();
                        self.audition_after_start = false;
                    }
                    self.running = Some(info);
                    self.pending = false;
                    self.failed = false;
                    self.status = if self.session.audio.source.has_midi() {
                        "Ready to play · MIDI notes become sound in Shredini."
                    } else {
                        "Monitoring · your guitar, with a little more space."
                    }
                    .into();
                }
                Event::Stopped => {
                    self.running = None;
                    self.pending = false;
                    self.status = "Monitoring stopped.".into();
                }
                Event::Failed(message) => {
                    self.play_after_start = false;
                    self.audition_after_start = false;
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
                if self.session.audio.source.has_audio() {
                ui.label("1. Connect your guitar using a USB guitar cable with built-in audio conversion, or any instrument audio interface. TONEX is optional.");
                ui.label("2. For an input-only USB cable, choose WASAPI, select the cable as Guitar input, and choose your speakers or headphones as Stereo output.");
                ui.label("3. Start with guitar channel 1. Mono inputs and different input/output sample rates are supported automatically.");
                ui.label("4. For an interface with its own headphone output and ASIO driver, ASIO can provide lower-latency monitoring through that same interface.");
                ui.weak("A passive jack-to-USB adapter cannot digitize a guitar. The cable must appear as a Windows audio input. Device names vary by model.");
                ui.collapsing("Using TONEX ONE?",|ui|{
                    ui.label("Bypass pedal processing in Stomp mode and disable Direct Monitoring in TONEX Editor. Install IK's ASIO driver from IK Product Manager → Hardware if using ASIO.");
                    ui.hyperlink_to("IK’s TONEX ONE setup guide","https://www.ikmultimedia.com/contact-support/quickfaq.php?id=1298&quick=QUICKHW");
                });
                }
                if self.session.audio.source.has_midi() {
                    ui.label("Connect your MPK mini or another USB MIDI keyboard. Shredini generates the instrument sound; no guitar input is needed in MIDI-only mode.");
                    ui.label("Select a MIDI input and a stereo output, press Start, then play. Keys and note-mode pads play notes; sustain, pitch bend and modulation are supported.");
                }
                ui.separator();
                ui.add_enabled_ui(self.running.is_none() && !self.pending,|ui|{
                    if self.session.audio.source.has_midi() {
                        ui.label("MIDI keyboard input");
                        let selected=self.midi_ports.iter().find(|p|p.id==self.session.audio.midi_port).map_or("Select a MIDI keyboard…",|p|p.name.as_str());
                        egui::ComboBox::from_id_salt("midi_port").selected_text(selected).width(520.0).show_ui(ui,|ui|{
                            for port in &self.midi_ports {ui.selectable_value(&mut self.session.audio.midi_port,port.id.clone(),&port.name);}
                        });
                    }
                    egui::ComboBox::from_id_salt("backend").selected_text(self.session.audio.backend.label()).show_ui(ui,|ui|{
                        ui.selectable_value(&mut self.session.audio.backend,Backend::Asio,Backend::Asio.label());
                        ui.selectable_value(&mut self.session.audio.backend,Backend::Wasapi,Backend::Wasapi.label());
                    });
                    let backend=self.session.audio.backend;
                    if self.session.audio.source.has_audio() {device_picker(ui,"Guitar input",&self.devices,backend,true,&mut self.session.audio.input_id);}
                    if backend==Backend::Asio && self.session.audio.source.has_audio() {
                        self.session.audio.output_id=self.session.audio.input_id.clone();
                        ui.weak("ASIO uses the same interface for capture and playback.");
                    } else {device_picker(ui,"Stereo output",&self.devices,backend,false,&mut self.session.audio.output_id);}
                    if self.session.audio.source.has_audio() {
                    let channels=self.devices.iter().find(|d|d.id==self.session.audio.input_id && d.backend==backend).map_or(2,|d|d.input_channels).max(1);
                    ui.horizontal(|ui|{
                        ui.label("Guitar channel");
                        let mut channel=self.session.audio.input_channel+1;
                        ui.add(egui::DragValue::new(&mut channel).range(1..=channels));
                        self.session.audio.input_channel=channel-1;
                    });
                    }
                    egui::ComboBox::from_id_salt("buffer").selected_text(buffer_label(self.session.audio.buffer_frames)).show_ui(ui,|ui|{
                        for n in [128,256,512,0] {ui.selectable_value(&mut self.session.audio.buffer_frames,n,buffer_label(n));}
                    });
                    ui.weak("Fixed sizes are requests; unsupported sizes fall back to the driver default.");
                    if ui.button("Refresh devices").clicked(){self.engine.send(Command::Refresh);}
                });
                if self.running.is_some() {ui.weak("Stop monitoring to change audio devices or buffer size.");}
                ui.add(egui::Slider::new(&mut self.session.parameters.input_trim_db,-24.0..=24.0).text("Input trim").suffix(" dB").fixed_decimals(1));
                if self.session.audio.source.has_audio() {
                ui.weak("Digital trim cannot undo clipping at the audio input. Lower hardware input gain where available if the input clips.");
                if !self.device_note.is_empty(){ui.colored_label(Color32::from_rgb(241,197,135),&self.device_note);}
                }
                if self.session.audio.source.has_midi() && !self.midi_note.is_empty() {ui.colored_label(Color32::YELLOW,&self.midi_note);}
                ui.separator();ui.heading("Diagnostics");
                let d=&self.engine.diagnostics;
                ui.label(format!("Underruns: {}  ·  Dropped input frames: {}  ·  Driver xruns: {}  ·  Resyncs: {}",
                    d.underruns.load(Relaxed),d.overruns.load(Relaxed),d.xruns.load(Relaxed),d.resyncs.load(Relaxed)));
                ui.label(format!("Input queue: {} frames  ·  Clock correction: {:+.0} ppm",d.queue_frames.load(Relaxed),f32::from_bits(d.correction_ppm.load(Relaxed))));
                ui.label(format!("Input clipping: {} · Output peaks guarded: {}", d.input_clips.load(Relaxed), d.output_clips.load(Relaxed)));
                if self.session.audio.source.has_midi() {
                    let m=&self.engine.midi;
                    ui.label(format!("MIDI messages: {} · Dropped MIDI events: {} · Active voices: {}",m.messages.load(Relaxed),m.dropped.load(Relaxed),m.voices.load(Relaxed)));
                    ui.weak("The note display lights when MIDI notes reach the synthesizer. All notes off clears held/sustained notes if a controller misses a release.");
                }
                if let Some(info)=&self.running {
                    if info.source.has_audio() {ui.label(format!("Input: {} Hz / {} ch / {} / {}",info.input_rate,info.input_channels,info.input_format,actual_buffer(info.input_buffer)));}
                    ui.label(format!("Output: {} Hz / {} ch / {} / {}",info.output_rate,info.output_channels,info.output_format,actual_buffer(info.output_buffer)));
                    ui.label(if info.resampling {"Adaptive sinc conversion: active"}else if info.source.has_audio() {"Adaptive sinc conversion: off (shared ASIO clock)"}else if info.source.has_midi(){"Keyboard: generated directly at the output sample rate"}else{"Audio files: interpolated at the output sample rate"});
                    let driver_ms=info.input_buffer.zip(info.output_buffer).map(|(i,o)|1000.0*(i as f32/info.input_rate as f32+o as f32/info.output_rate as f32));
                    if let Some(ms)=driver_ms {ui.label(format!("Reported driver buffer duration (input + output): {ms:.1} ms"));}
                }
                ui.weak("App latency covers queued samples, chunk staging and the resampler. Driver buffers, USB transport and converters add latency. This is not a measured round-trip value.");
                if d.realtime_denied.load(Relaxed) {ui.colored_label(Color32::YELLOW,"Windows denied real-time priority; audio continues at normal priority.");}
                ui.weak("If you hear crackles, stop monitoring and try 256 or 512 frames. A USB cable plus separate output may add more latency than an interface with its own headphone output.");
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
        self.draw_studio(ui);
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

fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[note as usize % 12], note as i32 / 12 - 1)
}
fn draw_keyboard(ui: &mut egui::Ui, state: &midi::State) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(420.0), 78.0),
        egui::Sense::hover(),
    );
    let notes = [state.notes[0].load(Relaxed), state.notes[1].load(Relaxed)];
    let last = state.last_note.load(Relaxed).saturating_sub(1);
    let base = if last == 0 {
        48
    } else {
        (last / 12 * 12).clamp(12, 96)
    } as u8;
    let w = rect.width() / 14.0;
    let mut white = 0;
    for black_pass in [false, true] {
        for note in base..base + 24 {
            let black = [1, 3, 6, 8, 10].contains(&(note % 12));
            let held = notes[note as usize / 64] & (1_u64 << (note % 64)) != 0;
            if black == black_pass {
                let (x, width, height) = if black {
                    ((white as f32 - 0.32) * w, w * 0.64, rect.height() * 0.63)
                } else {
                    (white as f32 * w, w - 1.0, rect.height())
                };
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left() + x, rect.top()),
                        egui::vec2(width, height),
                    ),
                    2.0,
                    if held {
                        ACCENT
                    } else if black {
                        Color32::from_rgb(17, 22, 29)
                    } else {
                        Color32::from_rgb(105, 118, 132)
                    },
                );
            }
            if !black {
                white += 1;
            }
        }
        white = 0;
    }
}

fn meter(ui: &mut egui::Ui, label: &str, peak: f32) {
    ui.label(RichText::new(label).size(10.0).color(MUTED));
    let db = 20.0 * peak.max(1e-6).log10();
    ui.add(
        egui::ProgressBar::new(((db + 60.0) / 60.0).clamp(0.0, 1.0))
            .desired_width(65.0)
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
