//! Files are decoded on a worker. The callback only reads prepared PCM and
//! retires replaced resources to the control thread for destruction.
use crate::{midi, model::SynthSound};
use anyhow::{Context, Result, bail};
use ringbuf::{HeapCons, HeapProd, HeapRb, traits::*};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Cursor, ErrorKind},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
        mpsc,
    },
    thread,
};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

const MAX_FRAMES: usize = 48_000 * 60 * 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Backing,
    Sample,
    SoundFont,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub path: PathBuf,
    pub role: Role,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub backing_gain: f32,
    pub backing_loop: bool,
    pub sample_root: u8,
    pub sample_loop: bool,
    pub sample_start: f32,
    pub sample_end: f32,
    pub metronome: bool,
    pub bpm: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            backing_gain: 0.65,
            backing_loop: false,
            sample_root: 60,
            sample_loop: false,
            sample_start: 0.0,
            sample_end: 1.0,
            metronome: false,
            bpm: 100.0,
        }
    }
}
impl Settings {
    pub fn sanitize(&mut self) {
        fn finite(v: f32, lo: f32, hi: f32, default: f32) -> f32 {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                default
            }
        }
        self.backing_gain = finite(self.backing_gain, 0.0, 1.0, 0.65);
        self.sample_root = self.sample_root.min(127);
        self.sample_start = finite(self.sample_start, 0.0, 0.99, 0.0);
        self.sample_end = finite(self.sample_end, self.sample_start + 0.001, 1.0, 1.0);
        self.bpm = finite(self.bpm, 30.0, 240.0, 100.0);
    }
}
pub struct State {
    pub playing: AtomicBool,
    pub position: AtomicU64,
    pub length: AtomicU64,
    pub seek_revision: AtomicU64,
    pub seek_fraction: AtomicU32,
    pub gain: AtomicU32,
    pub looping: AtomicBool,
    pub root: AtomicU32,
    pub sample_loop: AtomicBool,
    pub sample_start: AtomicU32,
    pub sample_end: AtomicU32,
    pub click: AtomicBool,
    pub bpm: AtomicU32,
}
impl Default for State {
    fn default() -> Self {
        let s = Self {
            playing: AtomicBool::new(false),
            position: AtomicU64::new(0),
            length: AtomicU64::new(0),
            seek_revision: AtomicU64::new(0),
            seek_fraction: AtomicU32::new(0),
            gain: AtomicU32::new(0),
            looping: AtomicBool::new(false),
            root: AtomicU32::new(60),
            sample_loop: AtomicBool::new(false),
            sample_start: AtomicU32::new(0),
            sample_end: AtomicU32::new(1.0_f32.to_bits()),
            click: AtomicBool::new(false),
            bpm: AtomicU32::new(100.0_f32.to_bits()),
        };
        s.publish(&Settings::default());
        s
    }
}
impl State {
    pub fn publish(&self, settings: &Settings) {
        let mut p = settings.clone();
        p.sanitize();
        self.gain.store(p.backing_gain.to_bits(), Relaxed);
        self.looping.store(p.backing_loop, Relaxed);
        self.root.store(p.sample_root as u32, Relaxed);
        self.sample_loop.store(p.sample_loop, Relaxed);
        self.sample_start.store(p.sample_start.to_bits(), Relaxed);
        self.sample_end.store(p.sample_end.to_bits(), Relaxed);
        self.click.store(p.metronome, Relaxed);
        self.bpm.store(p.bpm.to_bits(), Relaxed);
    }
    pub fn seek(&self, fraction: f32) {
        self.position.store(
            (fraction.clamp(0.0, 1.0) as f64 * self.length.load(Relaxed) as f64) as u64,
            Relaxed,
        );
        self.seek_fraction
            .store(fraction.clamp(0.0, 1.0).to_bits(), Relaxed);
        self.seek_revision.fetch_add(1, Relaxed);
    }
    pub fn stop(&self) {
        self.playing.store(false, Relaxed);
        self.seek(0.0);
    }
}

pub struct Clip {
    pub frames: Vec<[f32; 2]>,
    pub rate: u32,
    pub peak: f32,
}
impl Clip {
    pub fn duration(&self) -> f64 {
        self.frames.len() as f64 / self.rate as f64
    }
    pub fn at(&self, position: f64) -> [f32; 2] {
        if self.frames.is_empty() {
            return [0.0; 2];
        }
        let i = (position.max(0.0) as usize).min(self.frames.len() - 1);
        let j = (i + 1).min(self.frames.len() - 1);
        let t = (position - position.floor()) as f32;
        std::array::from_fn(|c| self.frames[i][c] + t * (self.frames[j][c] - self.frames[i][c]))
    }
    pub fn waveform(&self) -> Vec<f32> {
        let mut bins = vec![0.0_f32; 384];
        let size = self.frames.len().max(1);
        for (i, frame) in self.frames.iter().enumerate() {
            let b = (i * 384 / size).min(383);
            bins[b] = bins[b].max(frame[0].abs().max(frame[1].abs()));
        }
        bins
    }
}
pub fn decode(path: &Path) -> Result<Arc<Clip>> {
    let file = File::open(path).context("Open audio file")?;
    if file.metadata()?.len() > 512 * 1024 * 1024 {
        bail!("Audio file is too large (512 MB maximum).");
    }
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut reader = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )?
        .format;
    let track = reader.default_track().context("No supported audio track")?;
    let id = track.id;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut rate = 0;
    let mut frames = Vec::new();
    let mut peak = 0.0_f32;
    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != id {
            continue;
        }
        let audio = decoder.decode(&packet).context("Decode audio packet")?;
        let spec = *audio.spec();
        let channels = spec.channels.count();
        if channels == 0 {
            bail!("Audio track has no channels.");
        }
        if rate != 0 && rate != spec.rate {
            bail!("Audio changes sample rate midway through the file.");
        }
        rate = spec.rate;
        let mut samples = SampleBuffer::<f32>::new(audio.capacity() as u64, spec);
        samples.copy_interleaved_ref(audio);
        for frame in samples.samples().chunks_exact(channels) {
            let pair = [frame[0], frame[if channels > 1 { 1 } else { 0 }]].map(|v| {
                if v.is_finite() {
                    v.clamp(-8.0, 8.0)
                } else {
                    0.0
                }
            });
            peak = peak.max(pair[0].abs()).max(pair[1].abs());
            frames.push(pair);
        }
        if frames.len() > MAX_FRAMES {
            bail!("Decoded audio is too long for this version (about 20 minutes at 48 kHz).");
        }
    }
    if frames.is_empty() || rate == 0 {
        bail!("The file contains no decoded audio.");
    }
    Ok(Arc::new(Clip { frames, rate, peak }))
}
pub fn default_font() -> Result<Arc<SoundFont>> {
    let bytes = include_bytes!("../assets/GeneralUser-GS.sf2");
    Ok(Arc::new(
        SoundFont::new(&mut Cursor::new(bytes.as_slice()))
            .map_err(|e| anyhow::anyhow!("Instrument bank: {e}"))?,
    ))
}
pub fn load_font(path: &Path) -> Result<Arc<SoundFont>> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() > 256 * 1024 * 1024 {
        bail!("SoundFont exceeds the 256 MB import limit.");
    }
    Ok(Arc::new(
        SoundFont::new(&mut file).map_err(|e| anyhow::anyhow!("Invalid SoundFont: {e}"))?,
    ))
}
pub struct FontPlayer {
    pub synth: Synthesizer,
    left: [f32; 64],
    right: [f32; 64],
    index: usize,
}
impl FontPlayer {
    pub fn new(font: &Arc<SoundFont>, rate: u32) -> Result<Box<Self>> {
        let mut settings = SynthesizerSettings::new(rate as i32);
        settings.enable_reverb_and_chorus = false;
        settings.maximum_polyphony = 64;
        let mut synth = Synthesizer::new(font, &settings).map_err(|e| anyhow::anyhow!("{e}"))?;
        synth.set_master_volume(0.5);
        Ok(Box::new(Self {
            synth,
            left: [0.0; 64],
            right: [0.0; 64],
            index: 64,
        }))
    }
    pub fn select(&mut self, sound: SynthSound) {
        self.synth.note_off_all(true);
        for c in 0..16 {
            self.synth
                .process_midi_message(c, 0xc0, sound.program().unwrap_or(0), 0);
        }
        self.index = 64;
    }
    pub fn tick(&mut self) -> f32 {
        if self.index >= 64 {
            self.synth.render(&mut self.left, &mut self.right);
            self.index = 0;
        }
        let x = (self.left[self.index] + self.right[self.index]) * 0.5;
        self.index += 1;
        x
    }
}

#[derive(Clone, Default)]
pub struct Resources {
    pub sample: Option<Arc<Clip>>,
    pub backing: Option<Arc<Clip>>,
    pub font: Option<Arc<SoundFont>>,
}
pub enum Update {
    Sample(Option<Arc<Clip>>),
    Backing(Option<Arc<Clip>>),
    Font(Option<Box<FontPlayer>>),
}
pub struct ControlPorts {
    pub updates: HeapProd<Update>,
    pub retired: HeapCons<Update>,
}
pub struct Runtime {
    pub state: Arc<State>,
    pub backing: Option<Arc<Clip>>,
    pub updates: HeapCons<Update>,
    pub retired: HeapProd<Update>,
    pub position: f64,
    last_seek: u64,
    gain: f32,
    master: f32,
    beat: f64,
    click_remaining: u32,
    click_phase: f32,
}
impl Runtime {
    pub fn new(state: Arc<State>, backing: Option<Arc<Clip>>) -> (Self, ControlPorts) {
        let (updates, consumer) = HeapRb::<Update>::new(8).split();
        let (retired, retire_consumer) = HeapRb::<Update>::new(8).split();
        let position = state.position.load(Relaxed) as f64;
        let last_seek = state.seek_revision.load(Relaxed);
        (
            Self {
                state,
                backing,
                updates: consumer,
                retired,
                position,
                last_seek,
                gain: 0.0,
                master: 0.25,
                beat: 0.0,
                click_remaining: 0,
                click_phase: 0.0,
            },
            ControlPorts {
                updates,
                retired: retire_consumer,
            },
        )
    }
    pub fn begin_block(&mut self, instrument: &mut Option<midi::Instrument>) {
        while self.retired.vacant_len() > 0 {
            let Some(update) = self.updates.try_pop() else {
                break;
            };
            let old = match update {
                Update::Backing(clip) => {
                    self.position = 0.0;
                    Update::Backing(std::mem::replace(&mut self.backing, clip))
                }
                Update::Sample(clip) => {
                    if let Some(i) = instrument {
                        i.reset_notes();
                        Update::Sample(std::mem::replace(&mut i.sample, clip))
                    } else {
                        Update::Sample(clip)
                    }
                }
                Update::Font(font) => {
                    if let Some(i) = instrument {
                        i.reset_notes();
                        let old = std::mem::replace(&mut i.font, font);
                        i.refresh_font();
                        Update::Font(old)
                    } else {
                        Update::Font(font)
                    }
                }
            };
            // Space is checked above; only this callback produces retired items.
            let _ = self.retired.try_push(old);
        }
        let revision = self.state.seek_revision.load(Relaxed);
        if revision != self.last_seek {
            self.last_seek = revision;
            if let Some(clip) = &self.backing {
                self.position = f32::from_bits(self.state.seek_fraction.load(Relaxed)) as f64
                    * clip.frames.len() as f64;
            }
        }
    }
    pub fn tick(&mut self, rate: u32, master_db: f32) -> [f32; 2] {
        let playing = self.state.playing.load(Relaxed);
        let target = if playing {
            f32::from_bits(self.state.gain.load(Relaxed))
        } else {
            0.0
        };
        self.gain += (target - self.gain) * (1.0 / (0.005 * rate as f32));
        self.master +=
            (crate::dsp::db_gain(master_db) - self.master) * (1.0 / (0.020 * rate as f32));
        let mut pair = [0.0; 2];
        if let Some(clip) = &self.backing
            && (playing || self.gain > 0.00001)
        {
            if self.position >= clip.frames.len() as f64 {
                if self.state.looping.load(Relaxed) {
                    self.position = 0.0;
                } else {
                    self.state.playing.store(false, Relaxed);
                    self.gain = 0.0;
                }
            }
            if self.position < clip.frames.len() as f64 {
                pair = clip.at(self.position).map(|v| v * self.gain * self.master);
                if playing {
                    self.position += clip.rate as f64 / rate as f64;
                }
            }
        }
        if self.state.click.load(Relaxed) {
            self.beat += f32::from_bits(self.state.bpm.load(Relaxed)) as f64 / (60.0 * rate as f64);
            if self.beat >= 1.0 {
                self.beat -= 1.0;
                self.click_remaining = rate / 60;
                self.click_phase = 0.0;
            }
            if self.click_remaining > 0 {
                self.click_phase += 1200.0 / rate as f32;
                let click = (self.click_phase * std::f32::consts::TAU).sin()
                    * self.click_remaining as f32
                    / (rate as f32 / 60.0)
                    * 0.12
                    * self.master;
                for v in &mut pair {
                    *v += click;
                }
                self.click_remaining -= 1;
            }
        } else {
            self.beat = 1.0;
            self.click_remaining = 0;
        }
        pair
    }
    pub fn publish(&self) {
        self.state
            .position
            .store(self.position.max(0.0) as u64, Relaxed);
    }
}

pub enum Loaded {
    Audio(Role, Arc<Clip>),
    Font(Arc<SoundFont>),
}
pub struct ImportResult {
    pub path: PathBuf,
    pub result: Result<Loaded>,
}
pub struct Importer {
    tx: mpsc::SyncSender<(Role, Option<PathBuf>)>,
    pub results: mpsc::Receiver<ImportResult>,
}
impl Importer {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel::<(Role, Option<PathBuf>)>(8);
        let (done, results) = mpsc::channel();
        thread::Builder::new()
            .name("media-import".into())
            .spawn(move || {
                while let Ok((role, path)) = rx.recv() {
                    let path = path.or_else(|| {
                        let dialog = rfd::FileDialog::new();
                        if role == Role::SoundFont {
                            dialog.add_filter("SoundFont", &["sf2"]).pick_file()
                        } else {
                            dialog
                                .add_filter("Audio", &["wav", "mp3", "flac", "ogg"])
                                .pick_file()
                        }
                    });
                    let Some(path) = path else {
                        let _ = done.send(ImportResult {
                            path: PathBuf::new(),
                            result: Err(anyhow::anyhow!("Import cancelled")),
                        });
                        continue;
                    };
                    let result = std::panic::catch_unwind(|| {
                        if role == Role::SoundFont {
                            load_font(&path).map(Loaded::Font)
                        } else {
                            decode(&path).map(|clip| Loaded::Audio(role, clip))
                        }
                    })
                    .unwrap_or_else(|_| {
                        Err(anyhow::anyhow!(
                            "The file reader could not process this file."
                        ))
                    });
                    if done.send(ImportResult { path, result }).is_err() {
                        break;
                    }
                }
            })
            .expect("create media worker");
        Self { tx, results }
    }
    pub fn import(&self, role: Role, path: Option<PathBuf>) -> bool {
        self.tx.try_send((role, path)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decode_compressed_formats() {
        for extension in ["mp3", "flac", "ogg"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/test-audio")
                .join(format!("tone.{extension}"));
            let clip = decode(&path).unwrap_or_else(|e| panic!("{extension}: {e:#}"));
            assert_eq!(clip.rate, 44100);
            assert!((0.09..0.2).contains(&clip.duration()));
            assert!(clip.peak > 0.01 && clip.peak < 0.2);
            assert!(clip.frames.iter().flatten().all(|v| v.is_finite()));
        }
    }
    #[test]
    fn decode_pcm_and_reject_malformed_media() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("media-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("test.wav");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&44_u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&44100_u32.to_le_bytes());
        bytes.extend_from_slice(&88200_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&8_u32.to_le_bytes());
        for sample in [0_i16, 16384, -16384, 0] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&path, bytes).unwrap();
        let clip = decode(&path).unwrap();
        assert_eq!(clip.rate, 44100);
        assert_eq!(clip.frames.len(), 4);
        assert_eq!(clip.frames[1], [0.5, 0.5]);
        assert_eq!(clip.at(1.5), [0.0, 0.0]);
        std::fs::write(&path, b"not an audio file").unwrap();
        assert!(decode(&path).is_err());
        assert!(load_font(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }
    #[test]
    fn live_asset_swaps_retire_large_buffers_off_callback() {
        let state = Arc::new(State::default());
        let old = Arc::new(Clip {
            frames: vec![[0.2; 2]; 1024],
            rate: 44100,
            peak: 0.2,
        });
        let weak = Arc::downgrade(&old);
        let (mut runtime, mut control) = Runtime::new(state, Some(old));
        control
            .updates
            .try_push(Update::Backing(None))
            .ok()
            .unwrap();
        let count = crate::allocation_probe::count(|| runtime.begin_block(&mut None));
        assert_eq!(count, 0);
        assert!(weak.upgrade().is_some());
        drop(control.retired.try_pop());
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn backing_seek_loop_and_pause_do_not_allocate() {
        let state = Arc::new(State::default());
        state.playing.store(true, Relaxed);
        state.looping.store(true, Relaxed);
        let clip = Arc::new(Clip {
            frames: vec![[0.2, -0.2]; 480],
            rate: 48000,
            peak: 0.2,
        });
        let (mut player, _) = Runtime::new(state.clone(), Some(clip));
        let allocations = crate::allocation_probe::count(|| {
            for _ in 0..48000 {
                for v in player.tick(44100, -12.0) {
                    assert!(v.is_finite());
                }
            }
            state.seek(0.5);
            player.begin_block(&mut None);
        });
        assert_eq!(allocations, 0);
        assert!(player.position >= 240.0);
        state.playing.store(false, Relaxed);
        for _ in 0..48000 {
            player.tick(44100, -12.0);
        }
        assert!(player.tick(44100, -12.0)[0].abs() < 1e-5);
    }
    #[test]
    fn bundled_sampled_instruments_render_without_allocations() {
        let font = default_font().unwrap();
        let mut player = FontPlayer::new(&font, 48000).unwrap();
        let mut energy = 0.0;
        let allocations = crate::allocation_probe::count(|| {
            for sound in SynthSound::ALL {
                if sound.program().is_none() {
                    continue;
                }
                player.select(sound);
                player
                    .synth
                    .note_on(if sound == SynthSound::Drums { 9 } else { 0 }, 60, 100);
                for _ in 0..4800 {
                    energy += player.tick().abs();
                }
            }
        });
        assert_eq!(allocations, 0);
        assert!(energy > 1.0);
    }
}
