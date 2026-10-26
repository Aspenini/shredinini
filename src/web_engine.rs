//! Worklet-facing adapter. Construction and asset installation happen before
//! connecting the node. `render` uses fixed buffers and the shared Rust DSP.
use crate::{
    dsp::{Processor, peak_protect},
    media, midi,
    model::{SynthSound, ToneParameters},
};
use ringbuf::{HeapProd, HeapRb, traits::*};
use std::{
    io::Cursor,
    sync::{Arc, atomic::Ordering::Relaxed},
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

const MAX_BLOCK: usize = 2048;
const MAX_WEB_FRAMES: usize = 48_000 * 60 * 10;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct WebEngine {
    rate: u32,
    parameters: ToneParameters,
    settings: media::Settings,
    processor: Processor,
    instrument: Option<midi::Instrument>,
    messages: HeapProd<midi::Message>,
    state: Arc<midi::State>,
    player: media::Runtime,
    input: Box<[f32; MAX_BLOCK]>,
    output: Box<[f32; MAX_BLOCK * 2]>,
    audio: bool,
    keyboard: bool,
    bypass: f32,
    fade: f32,
    input_peak: f32,
    output_peak: f32,
    protected_peaks: u32,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl WebEngine {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(rate: u32, audio: bool, keyboard: bool) -> Result<Self, String> {
        if !(8_000..=192_000).contains(&rate) {
            return Err("Unsupported audio sample rate".into());
        }
        let parameters = ToneParameters::default();
        let mut processor = Processor::new(rate, parameters);
        if !audio {
            processor.use_full_instrument_range(rate);
        }
        let state = Arc::new(midi::State::default());
        state.set_sound(SynthSound::ElectricKeys);
        let (messages, consumer) = HeapRb::new(midi::QUEUE_CAPACITY).split();
        let media_state = Arc::new(media::State::default());
        let mut instrument = midi::Instrument::new(consumer, state.clone(), rate);
        instrument.media = media_state.clone();
        let (player, _) = media::Runtime::new(media_state, None);
        Ok(Self {
            rate,
            parameters,
            settings: Default::default(),
            processor,
            instrument: Some(instrument),
            messages,
            state,
            player,
            input: Box::new([0.0; MAX_BLOCK]),
            output: Box::new([0.0; MAX_BLOCK * 2]),
            audio,
            keyboard,
            bypass: 0.0,
            fade: 0.0,
            input_peak: 0.0,
            output_peak: 0.0,
            protected_peaks: 0,
        })
    }
    /// Install only while disconnected/suspended; parsing and allocation are not
    /// part of the render callback. The browser prepares assets before Start.
    pub fn load_font(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > 256 * 1024 * 1024 {
            return Err("SoundFont exceeds 256 MB".into());
        }
        let font = Arc::new(
            rustysynth::SoundFont::new(&mut Cursor::new(bytes)).map_err(|e| e.to_string())?,
        );
        let player = media::FontPlayer::new(&font, self.rate).map_err(|e| e.to_string())?;
        let instrument = self.instrument.as_mut().unwrap();
        instrument.reset_notes();
        instrument.font = Some(player);
        instrument.refresh_font();
        Ok(())
    }
    pub fn load_sample(&mut self, pcm: &[f32], rate: u32) -> Result<(), String> {
        let clip = clip(pcm, rate)?;
        let instrument = self.instrument.as_mut().unwrap();
        instrument.reset_notes();
        instrument.sample = Some(clip);
        Ok(())
    }
    pub fn load_backing(&mut self, pcm: &[f32], rate: u32) -> Result<(), String> {
        let clip = clip(pcm, rate)?;
        self.player.state.stop();
        self.player
            .state
            .length
            .store(clip.frames.len() as u64, Relaxed);
        self.player.backing = Some(clip);
        Ok(())
    }
    pub fn select_sound(&self, index: u32) {
        self.state.set_sound(SynthSound::from_index(index));
    }
    #[allow(clippy::too_many_arguments)]
    pub fn tone(
        &mut self,
        tone: f32,
        warmth: f32,
        reverb: f32,
        delay: f32,
        compression: bool,
        trim: f32,
        master: f32,
        bypass: bool,
    ) {
        self.parameters = ToneParameters {
            tone,
            warmth,
            reverb,
            delay,
            compression,
            input_trim_db: trim,
            output_db: master,
            bypass,
        };
        self.parameters.sanitize();
    }
    pub fn sample_controls(&mut self, root: u8, start: f32, end: f32, looping: bool) {
        self.settings.sample_root = root;
        self.settings.sample_start = start;
        self.settings.sample_end = end;
        self.settings.sample_loop = looping;
        self.settings.sanitize();
        self.player.state.publish(&self.settings);
    }
    pub fn track(&mut self, playing: bool, gain: f32, looping: bool) {
        self.settings.backing_gain = gain;
        self.settings.backing_loop = looping;
        self.settings.sanitize();
        self.player.state.publish(&self.settings);
        self.player.state.playing.store(playing, Relaxed);
    }
    pub fn seek(&self, fraction: f32) {
        if fraction.is_finite() {
            self.player.state.seek(fraction);
        }
    }
    pub fn click(&mut self, enabled: bool, bpm: f32) {
        self.settings.metronome = enabled;
        self.settings.bpm = bpm;
        self.settings.sanitize();
        self.player.state.publish(&self.settings);
    }
    pub fn midi(&mut self, status: u8, a: u8, b: u8) {
        if let Some(message) = midi::decode(&[status, a, b]) {
            self.state.messages.fetch_add(1, Relaxed);
            if self.messages.try_push(message).is_err() {
                self.state.all_notes_off();
            }
        }
    }
    pub fn audition(&self) {
        self.state.test_sound();
    }
    pub fn panic(&self) {
        self.state.all_notes_off();
    }
    pub fn input_ptr(&mut self) -> *mut f32 {
        self.input.as_mut_ptr()
    }
    pub fn output_ptr(&self) -> *const f32 {
        self.output.as_ptr()
    }
    pub fn input_peak(&self) -> f32 {
        self.input_peak
    }
    pub fn output_peak(&self) -> f32 {
        self.output_peak
    }
    pub fn protected_peaks(&self) -> u32 {
        self.protected_peaks
    }
    pub fn voices(&self) -> u32 {
        self.state.voices.load(Relaxed)
    }
    pub fn midi_events(&self) -> u32 {
        self.state.messages.load(Relaxed) as u32
    }
    pub fn position(&self) -> f64 {
        self.player.position / self.player.backing.as_ref().map_or(1, |c| c.rate) as f64
    }
    pub fn playing(&self) -> bool {
        self.player.state.playing.load(Relaxed)
    }
    pub fn render(&mut self, frames: usize) -> bool {
        if frames > MAX_BLOCK {
            return false;
        }
        self.player.begin_block(&mut self.instrument);
        let instrument = self.instrument.as_mut().unwrap();
        instrument.begin_block();
        self.processor.set_parameters(self.parameters);
        self.input_peak = 0.0;
        self.output_peak = 0.0;
        for n in 0..frames {
            let guitar = if self.audio { self.input[n] } else { 0.0 };
            let input = guitar
                + if self.keyboard {
                    instrument.tick()
                } else {
                    0.0
                };
            self.input_peak = self.input_peak.max(input.abs());
            self.bypass += ((if self.parameters.bypass { 1.0 } else { 0.0 }) - self.bypass)
                / (0.020 * self.rate as f32);
            let pair = self.processor.tick(input, self.bypass);
            let backing = self.player.tick(self.rate, self.parameters.output_db);
            self.fade = (self.fade + 1.0 / (0.005 * self.rate as f32)).min(1.0);
            for c in 0..2 {
                let x = peak_protect((pair[c] + backing[c]) * self.fade);
                self.output[n * 2 + c] = x;
                self.output_peak = self.output_peak.max(x.abs());
                if x.abs() > 0.98 {
                    self.protected_peaks = self.protected_peaks.saturating_add(1);
                }
            }
        }
        instrument.publish_activity();
        self.player.publish();
        true
    }
}

fn clip(pcm: &[f32], rate: u32) -> Result<Arc<media::Clip>, String> {
    if !(8000..=192000).contains(&rate)
        || pcm.is_empty()
        || !pcm.len().is_multiple_of(2)
        || pcm.len() / 2 > MAX_WEB_FRAMES
    {
        return Err(
            "Expected stereo PCM, 8–192 kHz, at most 28.8 million frames (10 minutes at 48 kHz)."
                .into(),
        );
    }
    let frames: Vec<_> = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            [p[0], p[1]].map(|v| {
                if v.is_finite() {
                    v.clamp(-8.0, 8.0)
                } else {
                    0.0
                }
            })
        })
        .collect();
    let peak = frames.iter().flatten().fold(0.0_f32, |a, b| a.max(b.abs()));
    Ok(Arc::new(media::Clip { frames, rate, peak }))
}

/// Decoder worker entry point. Returned PCM is copied/transferred once during
/// import; the worklet never decodes compressed files while rendering.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct DecodedAudio {
    clip: Arc<media::Clip>,
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl DecodedAudio {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(bytes: Vec<u8>, extension: &str) -> Result<Self, String> {
        let clip = media::decode_bytes(bytes, extension).map_err(|e| format!("{e:#}"))?;
        if !(8000..=192000).contains(&clip.rate) {
            return Err("Browser files must use sample rates from 8 to 192 kHz.".into());
        }
        if clip.frames.len() > MAX_WEB_FRAMES {
            return Err(
                "Browser audio is limited to 28.8 million frames (10 minutes at 48 kHz).".into(),
            );
        }
        Ok(Self { clip })
    }
    pub fn rate(&self) -> u32 {
        self.clip.rate
    }
    pub fn pcm(&self) -> Vec<f32> {
        self.clip.frames.iter().flatten().copied().collect()
    }
    pub fn waveform(&self) -> Vec<f32> {
        self.clip.waveform()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worklet_adapter_renders_shared_instruments_and_files_without_allocations() {
        let mut engine = WebEngine::new(48000, false, true).unwrap();
        engine
            .load_font(include_bytes!("../assets/GeneralUser-GS.sf2"))
            .unwrap();
        engine.load_backing(&vec![0.1; 8820], 44100).unwrap();
        engine.load_sample(&vec![0.2; 4410], 44100).unwrap();
        engine.track(true, 0.6, true);
        let count = crate::allocation_probe::count(|| {
            for sound in [3, 4, 5, 6, 7, 8, 9, 10, 11] {
                engine.select_sound(sound);
                engine.midi(0x90, 60, 100);
                for size in [1, 64, 128, 511, 2048] {
                    assert!(engine.render(size));
                    assert!(
                        engine.output[..size * 2]
                            .iter()
                            .all(|v| v.is_finite() && v.abs() <= 1.0)
                    );
                }
                engine.midi(0x80, 60, 0);
                engine.sample_controls(60, 0.1, 0.9, true);
                engine.tone(0.9, 0.8, 0.7, 0.6, true, 0.0, -12.0, false);
            }
            engine.panic();
            engine.render(128);
        });
        assert_eq!(count, 0);
        assert!(engine.position().is_finite());
        assert!(!engine.render(MAX_BLOCK + 1));
    }
    #[test]
    fn malformed_web_assets_are_rejected() {
        let mut engine = WebEngine::new(44100, false, true).unwrap();
        assert!(engine.load_font(b"broken").is_err());
        assert!(engine.load_sample(&[1.0], 44100).is_err());
        assert!(engine.load_backing(&[0.1, 0.1], 0).is_err());
        assert!(DecodedAudio::new(b"broken".to_vec(), "wav").is_err());
        assert!(WebEngine::new(0, false, true).is_err());
    }
}
