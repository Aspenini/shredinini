use super::*;

const BG: Color32 = Color32::from_rgb(15, 19, 25);
const SURFACE: Color32 = Color32::from_rgb(24, 30, 39);
const EDGE: Color32 = Color32::from_rgb(42, 51, 64);
const GOLD: Color32 = Color32::from_rgb(225, 184, 118);

impl Studio {
    pub(super) fn import_media(&mut self, role: media::Role, path: Option<PathBuf>) {
        if self.importer.import(role, path) {
            self.importing += 1;
        } else {
            self.notice = Some("The import queue is busy. Try again shortly.".into());
        }
    }
    pub(super) fn poll_imports(&mut self) {
        while let Ok(import) = self.importer.results.try_recv() {
            self.importing = self.importing.saturating_sub(1);
            let path = import.path;
            match import.result {
                Ok(media::Loaded::Audio(role, clip)) => {
                    let view = AssetView {
                        path: path.clone(),
                        name: file_name(&path),
                        duration: clip.duration(),
                        rate: clip.rate,
                        waveform: clip.waveform(),
                    };
                    if !self.engine.send(Command::InstallAudio(role, clip)) {
                        self.notice = Some("Audio is busy. Load the file again.".into());
                        continue;
                    }
                    if role == media::Role::Backing {
                        self.backing_view = Some(view);
                        self.session.backing_path = Some(path.clone());
                    } else {
                        let restoring = self.sample_view.is_none()
                            && self.session.sample_path.as_ref() == Some(&path);
                        self.sample_view = Some(view);
                        self.session.sample_path = Some(path.clone());
                        if !restoring {
                            self.session.media.sample_start = 0.0;
                            self.session.media.sample_end = 1.0;
                            self.choose_sound(SynthSound::UserSample);
                        }
                    }
                    if !self
                        .session
                        .library
                        .iter()
                        .any(|e| e.path == path && e.role == role)
                    {
                        self.session
                            .library
                            .push(media::LibraryEntry { path, role });
                    }
                    self.changed();
                }
                Ok(media::Loaded::Font(font)) => {
                    if !self.engine.send(Command::InstallFont(font)) {
                        self.notice = Some("Audio is busy. Load the bank again.".into());
                        continue;
                    }
                    self.bank_name = file_name(&path);
                    self.session.soundfont_path = Some(path.clone());
                    if !self.session.library.iter().any(|e| e.path == path) {
                        self.session.library.push(media::LibraryEntry {
                            path,
                            role: media::Role::SoundFont,
                        });
                    }
                    self.changed();
                }
                Err(error) => {
                    if !path.as_os_str().is_empty() {
                        self.notice =
                            Some(format!("Could not import {}: {error:#}", file_name(&path)));
                    }
                }
            }
        }
    }
    fn choose_sound(&mut self, sound: SynthSound) {
        self.page = 0;
        self.session.audio.synth_sound = sound;
        if !self.session.audio.source.has_midi() {
            if self.running.is_some() {
                self.engine.send(Command::Stop);
                self.pending = true;
            }
            self.session.audio.source = InstrumentSource::MidiKeyboard;
            self.prepare_keyboard();
        }
        self.changed();
    }
    fn play_backing(&mut self) {
        if self.backing_view.is_none() {
            self.import_media(media::Role::Backing, None);
            return;
        }
        if self.engine.media.playing.load(Relaxed) {
            self.engine.media.playing.store(false, Relaxed);
            return;
        }
        let length = self.engine.media.length.load(Relaxed);
        if self.engine.media.position.load(Relaxed) >= length {
            self.engine.media.seek(0.0);
        }
        if self.running.is_some() {
            self.engine.media.playing.store(true, Relaxed);
        } else {
            let input_ready = (!self.session.audio.source.has_audio()
                || !self.session.audio.input_id.is_empty())
                && (!self.session.audio.source.has_midi()
                    || self
                        .midi_ports
                        .iter()
                        .any(|p| p.id == self.session.audio.midi_port));
            if !input_ready {
                self.session.audio.source = InstrumentSource::Playback;
            }
            self.prepare_keyboard();
            self.play_after_start = true;
            self.changed();
            self.request_start();
        }
    }
    pub(super) fn draw_studio(&mut self, ui: &mut egui::Ui) {
        let dropped = ui
            .ctx()
            .input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("sf2"))
            {
                self.import_media(media::Role::SoundFont, Some(path));
            } else {
                self.pending_drop = Some(path);
            }
        }
        egui::Panel::bottom("transport")
            .exact_size(76.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(18, 24, 31))
                    .inner_margin(egui::Margin::symmetric(24, 15)),
            )
            .show(ui, |ui| self.transport(ui));
        egui::Panel::left("library_navigation")
            .exact_size(204.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(BG).inner_margin(20))
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(20, 25, 33))
                    .inner_margin(28),
            )
            .show(ui, |ui| {
                self.header(ui);
                ui.add_space(18.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(notice) = self.notice.clone() {
                            egui::Frame::new()
                                .fill(Color32::from_rgb(55, 44, 32))
                                .corner_radius(8)
                                .inner_margin(12)
                                .show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.colored_label(GOLD, notice);
                                        if ui.small_button("Dismiss").clicked() {
                                            self.notice = None;
                                        }
                                    });
                                });
                            ui.add_space(12.0);
                        }
                        self.hero(ui);
                        ui.add_space(16.0);
                        if self.page == 1 {
                            self.files_page(ui);
                        } else if self.page != 0 {
                            self.instrument_library(ui);
                        }
                        ui.add_space(16.0);
                        self.effects(ui);
                        ui.add_space(16.0);
                        self.backing_player(ui);
                        ui.add_space(8.0);
                    });
            });
        self.drop_dialog(ui.ctx());
    }
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(24.0, 32.0), egui::Sense::hover());
            for (i, h) in [12.0, 28.0, 22.0, 16.0].iter().enumerate() {
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(r.left() + 3.0 + i as f32 * 6.0, r.center().y),
                        egui::vec2(3.0, *h),
                    ),
                    2.0,
                    ACCENT,
                );
            }
            ui.label(RichText::new("Shredini").size(24.0).strong());
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new("INSTRUMENT STUDIO")
                .size(9.0)
                .color(MUTED)
                .extra_letter_spacing(1.8),
        );
        ui.add_space(40.0);
        eyebrow(ui, "YOUR WORKSPACE");
        ui.add_space(12.0);
        for (page, label) in [
            (0, "Studio"),
            (3, "Sound library"),
            (1, "Your audio"),
            (2, "Pinned sounds"),
        ] {
            let selected = self.page == page;
            if ui
                .add_sized(
                    [ui.available_width(), 39.0],
                    egui::Button::new(RichText::new(label).size(14.0).color(if selected {
                        ACCENT
                    } else {
                        Color32::from_rgb(181, 191, 204)
                    }))
                    .fill(if selected {
                        Color32::from_rgb(30, 52, 51)
                    } else {
                        BG
                    })
                    .stroke(egui::Stroke::NONE),
                )
                .clicked()
            {
                self.page = page;
            }
        }
        ui.add_space(24.0);
        ui.separator();
        ui.add_space(16.0);
        eyebrow(ui, "ADD TO YOUR STUDIO");
        ui.add_space(12.0);
        if ui.button("Import backing track").clicked() {
            self.import_media(media::Role::Backing, None);
        }
        if ui.button("Import keyboard sample").clicked() {
            self.import_media(media::Role::Sample, None);
        }
        if ui.button("Load SoundFont bank").clicked() {
            self.import_media(media::Role::SoundFont, None);
        }
        ui.add_space(12.0);
        ui.label(
            RichText::new("Or drop an audio file anywhere in the studio.")
                .size(11.0)
                .color(MUTED),
        );
        if self.importing > 0 {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading your audio…");
            });
        }
        ui.add_space(30.0);
        eyebrow(ui, "SOUND BANK");
        ui.add_space(8.0);
        ui.label(RichText::new(&self.bank_name).size(11.0).color(MUTED));
        ui.label(
            RichText::new(if self.session.soundfont_path.is_some() {
                "Custom General MIDI SoundFont"
            } else {
                "Sampled instruments by S. Christian Collins"
            })
            .size(10.0)
            .color(Color32::from_rgb(102, 115, 133)),
        );
        ui.add_space(24.0);
        if ui.button("Audio & MIDI setup").clicked() {
            self.settings = true;
        }
    }
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(match self.page {
                        1 => "Your audio",
                        2 => "Pinned sounds",
                        3 => "Sound library",
                        _ => "Your studio",
                    })
                    .size(25.0)
                    .strong(),
                );
                ui.label(
                    RichText::new("Plug in. Find a sound. Make something yours.")
                        .size(12.0)
                        .color(MUTED),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let live = self.running.is_some();
                let (r, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(r.center(), 3.0, if live { ACCENT } else { MUTED });
                ui.label(
                    RichText::new(if live { "LIVE" } else { "STANDBY" })
                        .size(10.0)
                        .color(if live { ACCENT } else { MUTED }),
                );
                let before = self.session.audio.source;
                ui.add_enabled_ui(!self.pending && self.running.is_none(), |ui| {
                    egui::ComboBox::from_id_salt("source_new")
                        .selected_text(self.session.audio.source.label())
                        .width(155.0)
                        .show_ui(ui, |ui| {
                            for source in [
                                InstrumentSource::Guitar,
                                InstrumentSource::MidiKeyboard,
                                InstrumentSource::GuitarAndMidi,
                                InstrumentSource::Playback,
                            ] {
                                ui.selectable_value(
                                    &mut self.session.audio.source,
                                    source,
                                    source.label(),
                                );
                            }
                        });
                });
                if before != self.session.audio.source {
                    if !self.session.audio.source.has_audio() {
                        self.prepare_keyboard();
                    }
                    self.changed();
                }
            });
        });
    }
    fn hero(&mut self, ui: &mut egui::Ui) {
        let source = self.session.audio.source;
        let sound = self.session.audio.synth_sound;
        let title = if source.has_midi() {
            sound.label()
        } else if source == InstrumentSource::Playback {
            "Listening room"
        } else {
            "Your electric guitar"
        };
        card().inner_margin(22).show(ui,|ui|{
            ui.set_min_width(ui.available_width());
            ui.columns(2,|cols|{
                eyebrow(&mut cols[0],if source.has_midi(){sound.category()}else{"LIVE AUDIO"});cols[0].add_space(8.0);
                cols[0].label(RichText::new(title).size(32.0).strong());cols[0].add_space(6.0);
                cols[0].label(RichText::new(if source.has_midi(){sound.description()}else if source==InstrumentSource::Playback{"A place for your backing tracks, loops, and ideas."}else{"A clean starting point for your guitar. Shape your tone, add some space, and play."}).size(12.0).color(MUTED));
                cols[0].add_space(14.0);
                cols[0].horizontal(|ui|{
                    if ui.add_enabled(!self.pending,egui::Button::new(RichText::new(if self.running.is_some(){"Stop audio"}else{"Start playing"}).strong().color(Color32::from_rgb(15,35,31))).fill(ACCENT)).clicked(){self.toggle_audio();}
                    if source.has_midi() && ui.add_enabled(!self.pending,egui::Button::new("Audition")).clicked(){if self.running.is_some(){self.engine.midi.test_sound();}else{self.audition_after_start=true;self.request_start();}}
                });
                cols[1].add_space(12.0);
                if source.has_midi(){draw_keyboard(&mut cols[1],&self.engine.midi);cols[1].add_space(10.0);
                    let port=self.midi_ports.iter().find(|p|p.id==self.session.audio.midi_port).map_or("Select a MIDI keyboard in setup",|p|p.name.as_str());
                    cols[1].label(RichText::new(port).size(11.0).color(ACCENT));
                    cols[1].label(RichText::new(format!("{} active notes · {} MIDI events",self.engine.midi.voices.load(Relaxed),self.engine.midi.messages.load(Relaxed))).size(10.0).color(MUTED));
                }else{let data=self.backing_view.as_ref().map(|v|v.waveform.as_slice()).unwrap_or(&[]);waveform(&mut cols[1],data,0.0,76.0,false);cols[1].label(RichText::new("Headroom for your next idea.").size(12.0).color(MUTED));}
                if source.has_audio() { cols[1].horizontal(|ui| {
                    meter(ui,"INPUT",self.meters[0]);
                    if self.engine.diagnostics.input_clips.load(Relaxed)>0 {ui.colored_label(Color32::LIGHT_RED,"Input clipped · lower hardware gain");}
                }); }
            });
        });
        if sound == SynthSound::UserSample && source.has_midi() {
            self.sampler_editor(ui);
        }
    }
    fn toggle_audio(&mut self) {
        if self.running.is_some() {
            if self.engine.send(Command::Stop) {
                self.pending = true;
            }
        } else {
            if !self.session.audio.source.has_audio() {
                self.prepare_keyboard();
            }
            self.request_start();
        }
    }
    fn instrument_library(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if self.page == 2 {
                    "Your pinned collection"
                } else {
                    "Find your sound"
                })
                .size(18.0)
                .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_sized(
                    [230.0, 28.0],
                    egui::TextEdit::singleline(&mut self.search).hint_text("Search instruments…"),
                );
            });
        });
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            for category in [
                "All sounds",
                "Keys",
                "Guitars",
                "Bass",
                "Orchestral",
                "Drums",
                "Synth",
                "Sampler",
            ] {
                if ui
                    .selectable_label(self.category == category, category)
                    .clicked()
                {
                    self.category = category.into();
                }
            }
        });
        ui.add_space(12.0);
        let query = self.search.to_lowercase();
        let sounds = SynthSound::ALL
            .into_iter()
            .filter(|s| {
                (self.category == "All sounds" || s.category() == self.category)
                    && s.label().to_lowercase().contains(&query)
                    && (self.page != 2 || self.session.favorites.contains(s))
            })
            .collect::<Vec<_>>();
        if sounds.is_empty() {
            ui.label(
                RichText::new(
                    "No sounds here yet. Try another category or pin a sound from the library.",
                )
                .color(MUTED),
            );
        }
        for row in sounds.chunks(3) {
            ui.columns(3, |cols| {
                for (column, sound) in cols.iter_mut().zip(row) {
                    let selected = *sound == self.session.audio.synth_sound
                        && self.session.audio.source.has_midi();
                    card()
                        .stroke(egui::Stroke::new(1.0, if selected { ACCENT } else { EDGE }))
                        .inner_margin(14)
                        .show(column, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(sound.category().to_uppercase())
                                        .size(9.0)
                                        .color(if selected { ACCENT } else { MUTED }),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let pinned = self.session.favorites.contains(sound);
                                        if ui
                                            .small_button(if pinned { "Pinned" } else { "Pin" })
                                            .clicked()
                                        {
                                            if pinned {
                                                self.session.favorites.retain(|s| s != sound);
                                            } else {
                                                self.session.favorites.push(*sound);
                                            }
                                            self.changed();
                                        }
                                    },
                                );
                            });
                            ui.add_space(6.0);
                            if ui
                                .add_sized(
                                    [ui.available_width(), 32.0],
                                    egui::Button::new(
                                        RichText::new(sound.label()).size(17.0).strong(),
                                    )
                                    .fill(SURFACE)
                                    .stroke(egui::Stroke::NONE),
                                )
                                .clicked()
                            {
                                self.choose_sound(*sound);
                            }
                            ui.label(
                                RichText::new(if sound.program().is_some() {
                                    "SAMPLED"
                                } else if *sound == SynthSound::UserSample {
                                    "YOUR AUDIO"
                                } else {
                                    "SYNTHESIZER"
                                })
                                .size(9.0)
                                .color(
                                    if sound.program().is_some() {
                                        GOLD
                                    } else {
                                        MUTED
                                    },
                                ),
                            );
                        });
                }
            });
            ui.add_space(10.0);
        }
    }
    fn effects(&mut self, ui: &mut egui::Ui) {
        card().inner_margin(18).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tone & space").size(17.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Save tone…").clicked() {
                        self.rename_mode = false;
                        self.preset_name.clear();
                        self.preset_editor = true;
                    }
                    self.presets(ui);
                });
            });
            ui.add_space(14.0);
            let before = self.session.parameters;
            ui.columns(4, |cols| {
                effect(
                    &mut cols[0],
                    "Tone",
                    "Warm → bright",
                    &mut self.session.parameters.tone,
                );
                effect(
                    &mut cols[1],
                    "Warmth",
                    "Soft saturation",
                    &mut self.session.parameters.warmth,
                );
                effect(
                    &mut cols[2],
                    "Reverb",
                    "Room & depth",
                    &mut self.session.parameters.reverb,
                );
                effect(
                    &mut cols[3],
                    "Delay",
                    "A little echo",
                    &mut self.session.parameters.delay,
                );
            });
            ui.add_space(12.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.session.parameters.compression,
                    "Smooth compression",
                );
                ui.checkbox(&mut self.session.parameters.bypass, "Bypass effects");
                if self.session.audio.source.has_midi()
                    && ui.small_button("All notes off").clicked()
                {
                    self.engine.midi.all_notes_off();
                }
                if self.preset_modified() {
                    ui.label(RichText::new("Tone edited").size(10.0).color(GOLD));
                }
            });
            if self.session.parameters != before {
                self.changed();
            }
        });
    }
    fn presets(&mut self, ui: &mut egui::Ui) {
        let mut selected = None;
        egui::ComboBox::from_id_salt("tone_presets")
            .selected_text(&self.session.current_preset)
            .width(160.0)
            .show_ui(ui, |ui| {
                for p in model::factory_presets()
                    .into_iter()
                    .chain(self.session.user_presets.clone())
                {
                    if ui
                        .selectable_label(self.session.current_preset == p.name, &p.name)
                        .clicked()
                    {
                        selected = Some(p);
                    }
                }
                let is_user = self
                    .session
                    .user_presets
                    .iter()
                    .any(|p| p.name == self.session.current_preset);
                if is_user {
                    ui.separator();
                    if ui.button("Update saved tone").clicked() {
                        if let Some(p) = self
                            .session
                            .user_presets
                            .iter_mut()
                            .find(|p| p.name == self.session.current_preset)
                        {
                            p.parameters = self.session.parameters;
                        }
                        self.changed();
                    }
                    if ui.button("Rename saved tone…").clicked() {
                        self.rename_mode = true;
                        self.preset_name = self.session.current_preset.clone();
                        self.preset_editor = true;
                    }
                    if ui.button("Delete saved tone").clicked() {
                        self.session
                            .user_presets
                            .retain(|p| p.name != self.session.current_preset);
                        self.session.current_preset = "Unsaved tone".into();
                        self.changed();
                    }
                }
            });
        if let Some(p) = selected {
            let old = self.session.parameters;
            self.session.parameters = p.parameters;
            self.session.parameters.output_db = old.output_db;
            self.session.parameters.input_trim_db = old.input_trim_db;
            self.session.parameters.bypass = old.bypass;
            self.session.current_preset = p.name;
            self.changed();
        }
    }
    fn backing_player(&mut self, ui: &mut egui::Ui) {
        card().inner_margin(18).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Play along").size(17.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Import track").clicked() {
                        self.import_media(media::Role::Backing, None);
                    }
                });
            });
            ui.add_space(8.0);
            if let Some(view) = &self.backing_view {
                ui.label(RichText::new(&view.name).color(ACCENT))
                    .on_hover_text(view.path.display().to_string());
                let length = self.engine.media.length.load(Relaxed).max(1);
                let position = self.engine.media.position.load(Relaxed);
                let fraction = (position as f64 / length as f64).clamp(0.0, 1.0) as f32;
                if let Some(seek) = waveform(ui, &view.waveform, fraction, 52.0, true) {
                    self.engine.media.seek(seek);
                }
                let duration = view.duration;
                ui.horizontal(|ui| {
                    if ui
                        .button(if self.engine.media.playing.load(Relaxed) {
                            "Pause track"
                        } else {
                            "Play track"
                        })
                        .clicked()
                    {
                        self.play_backing();
                    }
                    if ui.button("Rewind").clicked() {
                        self.engine.media.stop();
                    }
                    let before = self.session.media.clone();
                    ui.checkbox(&mut self.session.media.backing_loop, "Loop");
                    ui.label(
                        RichText::new(format!(
                            "{} / {}",
                            time(fraction as f64 * duration),
                            time(duration)
                        ))
                        .size(11.0)
                        .color(MUTED),
                    );
                    ui.style_mut().spacing.slider_width = 100.0;
                    ui.add(
                        egui::Slider::new(&mut self.session.media.backing_gain, 0.0..=1.0)
                            .text("Track level")
                            .show_value(false),
                    );
                    if self.session.media.backing_loop != before.backing_loop
                        || self.session.media.backing_gain != before.backing_gain
                    {
                        self.changed();
                    }
                });
            } else {
                ui.label(
                    RichText::new(
                        "Bring a song, a loop, or a recording. Play your instrument alongside it.",
                    )
                    .color(MUTED),
                );
                waveform(ui, &[], 0.0, 44.0, false);
                ui.label(
                    RichText::new("WAV · MP3 · FLAC · OGG")
                        .size(10.0)
                        .color(MUTED),
                );
            }
        });
    }
    fn sampler_editor(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        card().inner_margin(16).show(ui,|ui|{
            ui.horizontal(|ui|{ui.label(RichText::new("Keyboard sampler").size(16.0).strong());if ui.button("Choose audio…").clicked(){self.import_media(media::Role::Sample,None);}});
            if let Some(view)=&self.sample_view {
                ui.label(RichText::new(format!("{} · {} · {} Hz",view.name,time(view.duration),view.rate)).size(11.0).color(MUTED));waveform(ui,&view.waveform,0.0,48.0,false);
                let before=(self.session.media.sample_root,self.session.media.sample_loop,self.session.media.sample_start,self.session.media.sample_end);
                ui.horizontal(|ui|{ui.label("Root note");ui.add(egui::DragValue::new(&mut self.session.media.sample_root).range(0..=127));ui.label(note_name(self.session.media.sample_root));ui.checkbox(&mut self.session.media.sample_loop,"Loop while held");});
                ui.horizontal(|ui|{ui.style_mut().spacing.slider_width=140.0;ui.add(egui::Slider::new(&mut self.session.media.sample_start,0.0..=self.session.media.sample_end-0.001).text("Start").fixed_decimals(3));ui.add(egui::Slider::new(&mut self.session.media.sample_end,self.session.media.sample_start+0.001..=1.0).text("End").fixed_decimals(3));});
                if before!=(self.session.media.sample_root,self.session.media.sample_loop,self.session.media.sample_start,self.session.media.sample_end){self.changed();}
            }else{ui.label(RichText::new("Import a note, vocal, or texture. C4 plays it at its original pitch by default.").color(MUTED));}
        });
    }
    fn files_page(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("A home for your own sounds")
                .size(18.0)
                .strong(),
        );
        ui.add_space(10.0);
        ui.columns(2, |cols| {
            for (index, (title, detail, role)) in [
                (
                    "Backing tracks",
                    "Songs and loops to play along with.",
                    media::Role::Backing,
                ),
                (
                    "Keyboard samples",
                    "Turn a recording into a playable instrument.",
                    media::Role::Sample,
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let column = &mut cols[index];
                card().inner_margin(18).show(column, |ui| {
                    ui.label(RichText::new(title).size(17.0).strong());
                    ui.label(RichText::new(detail).size(12.0).color(MUTED));
                    ui.add_space(8.0);
                    if ui.button("Import audio…").clicked() {
                        self.import_media(role, None);
                    }
                });
            }
        });
        ui.add_space(18.0);
        if self.session.library.is_empty() {
            ui.label(
                RichText::new(
                    "Your imported files will appear here. Files stay in their original location.",
                )
                .color(MUTED),
            );
        }
        for entry in self.session.library.clone() {
            card().inner_margin(12).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(file_name(&entry.path)).strong());
                        ui.label(
                            RichText::new(match entry.role {
                                media::Role::Backing => "BACKING TRACK",
                                media::Role::Sample => "KEYBOARD SAMPLE",
                                media::Role::SoundFont => "INSTRUMENT BANK",
                            })
                            .size(9.0)
                            .color(MUTED),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Load").clicked() {
                            self.import_media(entry.role, Some(entry.path.clone()));
                        }
                        if entry.role != media::Role::SoundFont
                            && ui
                                .button(if entry.role == media::Role::Sample {
                                    "Use as track"
                                } else {
                                    "Use as sample"
                                })
                                .clicked()
                        {
                            self.import_media(
                                if entry.role == media::Role::Sample {
                                    media::Role::Backing
                                } else {
                                    media::Role::Sample
                                },
                                Some(entry.path.clone()),
                            );
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }
    }
    fn transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.pending,
                    egui::Button::new(
                        RichText::new(if self.running.is_some() {
                            "Stop"
                        } else {
                            "Start audio"
                        })
                        .strong()
                        .color(Color32::from_rgb(15, 35, 31)),
                    )
                    .fill(ACCENT)
                    .min_size(egui::vec2(105.0, 35.0)),
                )
                .clicked()
            {
                self.toggle_audio();
            }
            ui.vertical(|ui| {
                let output = self
                    .devices
                    .iter()
                    .find(|d| {
                        d.id == self.session.audio.output_id
                            && d.backend == self.session.audio.backend
                    })
                    .map_or("Choose an output in setup", |d| d.name.as_str());
                ui.label(RichText::new(output).size(11.0));
                ui.label(
                    RichText::new(if self.pending {
                        "Connecting…"
                    } else if self.failed {
                        &self.status
                    } else if self.running.is_some() {
                        "Audio running"
                    } else {
                        "Ready when you are"
                    })
                    .size(10.0)
                    .color(if self.failed { GOLD } else { MUTED }),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let before = self.session.parameters.output_db;
                ui.style_mut().spacing.slider_width = 110.0;
                ui.add(
                    egui::Slider::new(&mut self.session.parameters.output_db, -60.0..=0.0)
                        .suffix(" dB")
                        .fixed_decimals(0),
                );
                ui.label(RichText::new("MASTER").size(9.0).color(MUTED));
                if before != self.session.parameters.output_db {
                    self.changed();
                }
                let before = (self.session.media.metronome, self.session.media.bpm);
                ui.add(
                    egui::DragValue::new(&mut self.session.media.bpm)
                        .range(30.0..=240.0)
                        .suffix(" BPM")
                        .speed(1.0),
                );
                ui.checkbox(&mut self.session.media.metronome, "Click");
                if before != (self.session.media.metronome, self.session.media.bpm) {
                    self.changed();
                }
                meter(ui, "OUT", self.meters[1]);
                if self.engine.diagnostics.output_clips.load(Relaxed) > 0 {
                    ui.colored_label(Color32::LIGHT_RED, "PEAK").on_hover_text(
                        "Output peaks were guarded. Lower master or track level; resets on Start.",
                    );
                }
            });
        });
    }
    fn drop_dialog(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.pending_drop.clone() {
            let mut open = true;
            egui::Window::new("Make this file part of your studio")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(file_name(&path));
                    ui.label("How would you like to use it?");
                    if ui.button("Backing track — play along").clicked() {
                        self.import_media(media::Role::Backing, Some(path.clone()));
                        self.pending_drop = None;
                    }
                    if ui.button("Keyboard sample — play it on MIDI").clicked() {
                        self.import_media(media::Role::Sample, Some(path.clone()));
                        self.pending_drop = None;
                    }
                });
            if !open {
                self.pending_drop = None;
            }
        }
    }
}

fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(12)
        .stroke(egui::Stroke::new(1.0, EDGE))
}
fn eyebrow(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(9.0)
            .color(MUTED)
            .extra_letter_spacing(1.3),
    );
}
fn effect(ui: &mut egui::Ui, label: &str, description: &str, value: &mut f32) {
    ui.label(RichText::new(label).size(13.0).strong());
    ui.label(
        RichText::new(format!("{:.0}%", *value * 100.0))
            .size(27.0)
            .color(ACCENT),
    );
    ui.style_mut().spacing.slider_width = ui.available_width() - 8.0;
    ui.add(
        egui::Slider::new(value, 0.0..=1.0)
            .show_value(false)
            .text(""),
    )
    .on_hover_text(label);
    ui.label(RichText::new(description).size(10.0).color(MUTED));
}
fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
fn time(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}
fn waveform(
    ui: &mut egui::Ui,
    bins: &[f32],
    progress: f32,
    height: f32,
    seekable: bool,
) -> Option<f32> {
    let (r, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        if seekable {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::hover()
        },
    );
    ui.painter()
        .rect_filled(r, 6.0, Color32::from_rgb(18, 24, 32));
    let count = if bins.is_empty() { 64 } else { bins.len() };
    let width = r.width() / count as f32;
    for i in 0..count {
        let level = if bins.is_empty() {
            0.025
        } else {
            bins[i].clamp(0.025, 1.0)
        };
        let x = r.left() + i as f32 * width;
        let amplitude = level * (height - 12.0) * 0.5;
        ui.painter().line_segment(
            [
                egui::pos2(x, r.center().y - amplitude),
                egui::pos2(x, r.center().y + amplitude),
            ],
            egui::Stroke::new(
                width.clamp(1.0, 3.0),
                if i as f32 / (count as f32) < progress {
                    ACCENT
                } else {
                    Color32::from_rgb(76, 98, 116)
                },
            ),
        );
    }
    if progress > 0.0 {
        let x = r.left() + r.width() * progress;
        ui.painter().line_segment(
            [
                egui::pos2(x, r.top() + 3.0),
                egui::pos2(x, r.bottom() - 3.0),
            ],
            egui::Stroke::new(1.0, ACCENT),
        );
    }
    if seekable && (response.clicked() || response.dragged()) {
        response
            .interact_pointer_pos()
            .map(|p| ((p.x - r.left()) / r.width()).clamp(0.0, 1.0))
    } else {
        None
    }
}
