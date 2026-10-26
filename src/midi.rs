//! USB MIDI input and an allocation-free, sixteen-voice instrument.
use crate::{media, model::SynthSound};
use anyhow::{Context, Result, bail};
use midir::{Ignore, MidiInput, MidiInputConnection};
use ringbuf::{HeapCons, HeapProd, HeapRb, traits::*};
use std::sync::{
    Arc,
    atomic::{AtomicU32, AtomicU64, Ordering::Relaxed},
};

pub const QUEUE_CAPACITY: usize = 2048;

#[derive(Clone, Debug)]
pub struct Port {
    pub id: String,
    pub name: String,
}
pub fn enumerate() -> (Vec<Port>, String) {
    match MidiInput::new("Shredini MIDI discovery") {
        Ok(input) => {
            let ports = input
                .ports()
                .iter()
                .filter_map(|p| {
                    input
                        .port_name(p)
                        .ok()
                        .map(|name| Port { id: p.id(), name })
                })
                .collect::<Vec<_>>();
            let note = if ports.is_empty() {
                "No MIDI input found. Connect the MPK mini over USB and refresh.".into()
            } else {
                String::new()
            };
            (ports, note)
        }
        Err(e) => (Vec::new(), format!("MIDI discovery failed: {e}")),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Message {
    On(u8, u8, u8),
    Off(u8, u8),
    Control(u8, u8, u8),
    Bend(u8, u16),
}
pub fn decode(bytes: &[u8]) -> Option<Message> {
    if bytes.len() < 3 || bytes[1] >= 128 || bytes[2] >= 128 {
        return None;
    }
    let channel = bytes[0] & 15;
    match bytes[0] & 0xf0 {
        0x90 if bytes[2] > 0 => Some(Message::On(channel, bytes[1], bytes[2])),
        0x80 | 0x90 => Some(Message::Off(channel, bytes[1])),
        0xb0 => Some(Message::Control(channel, bytes[1], bytes[2])),
        0xe0 => Some(Message::Bend(
            channel,
            bytes[1] as u16 | (bytes[2] as u16) << 7,
        )),
        _ => None,
    }
}

#[derive(Default)]
pub struct State {
    pub messages: AtomicU64,
    pub dropped: AtomicU64,
    pub panic: AtomicU64,
    pub voices: AtomicU32,
    pub last_note: AtomicU32,
    pub notes: [AtomicU64; 2],
    pub sound: AtomicU32,
    pub audition: AtomicU64,
}
impl State {
    pub fn set_sound(&self, sound: SynthSound) {
        self.sound.store(sound.index(), Relaxed);
    }
    pub fn all_notes_off(&self) {
        self.panic.fetch_add(1, Relaxed);
    }
    pub fn test_sound(&self) {
        self.audition.fetch_add(1, Relaxed);
    }
    pub fn clear_activity(&self) {
        self.voices.store(0, Relaxed);
        self.last_note.store(0, Relaxed);
        for n in &self.notes {
            n.store(0, Relaxed);
        }
    }
}

pub fn connect(
    id: &str,
    mut producer: HeapProd<Message>,
    state: Arc<State>,
) -> Result<MidiInputConnection<()>> {
    let mut input = MidiInput::new("Shredini keyboard").map_err(|e| anyhow::anyhow!("{e}"))?;
    input.ignore(Ignore::SysexAndTime);
    let port = input
        .ports()
        .into_iter()
        .find(|p| p.id() == id)
        .context("Selected MIDI keyboard is disconnected. Refresh and select it again.")?;
    input
        .connect(
            &port,
            "Shredini notes",
            move |_, bytes, _| {
                if let Some(message) = decode(bytes) {
                    state.messages.fetch_add(1, Relaxed);
                    if let Message::On(_, note, _) = message {
                        state.last_note.store(note as u32 + 1, Relaxed);
                    }
                    if producer.try_push(message).is_err() {
                        state.dropped.fetch_add(1, Relaxed);
                        state.all_notes_off();
                    }
                }
            },
            (),
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "Cannot open MIDI keyboard: {e}. Close other apps that may be using this MIDI port."
            )
        })
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    held: bool,
    sustained: bool,
    channel: u8,
    note: u8,
    age: u64,
    phase: f32,
    step: f32,
    level: f32,
    velocity: f32,
    attacking: bool,
    sample_position: f64,
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            held: false,
            sustained: false,
            channel: 0,
            note: 0,
            age: 0,
            phase: 0.0,
            step: 0.0,
            level: 0.0,
            velocity: 0.0,
            attacking: true,
            sample_position: 0.0,
        }
    }
}

pub struct Instrument {
    pub font: Option<Box<media::FontPlayer>>,
    pub sample: Option<Arc<media::Clip>>,
    pub media: Arc<media::State>,
    queue: HeapCons<Message>,
    pub state: Arc<State>,
    voices: [Voice; 16],
    rates: [f32; 128],
    rate: f32,
    sustain: [bool; 16],
    bend: [f32; 16],
    modulation: [f32; 16],
    volume: [f32; 16],
    expression: [f32; 16],
    clock: u64,
    lfo: f32,
    sound: SynthSound,
    attack: f32,
    decay: f32,
    release: f32,
    sustain_level: f32,
    last_panic: u64,
    last_audition: u64,
    preview_remaining: u32,
    preview_note: u8,
}
impl Instrument {
    pub fn new(queue: HeapCons<Message>, state: Arc<State>, rate: u32) -> Self {
        let last_audition = state.audition.load(Relaxed);
        let mut result = Self {
            font: None,
            sample: None,
            media: Arc::new(media::State::default()),
            queue,
            state,
            voices: [Voice::default(); 16],
            rates: std::array::from_fn(|n| {
                440.0 * 2.0_f32.powf((n as f32 - 69.0) / 12.0) / rate as f32
            }),
            rate: rate as f32,
            sustain: [false; 16],
            bend: [1.0; 16],
            modulation: [0.0; 16],
            volume: [1.0; 16],
            expression: [1.0; 16],
            clock: 0,
            lfo: 0.0,
            sound: SynthSound::ElectricKeys,
            attack: 0.0,
            decay: 0.0,
            release: 0.0,
            sustain_level: 0.0,
            last_panic: 0,
            last_audition,
            preview_remaining: 0,
            preview_note: 60,
        };
        result.select_sound(SynthSound::from_index(result.state.sound.load(Relaxed)));
        result
    }
    fn select_sound(&mut self, sound: SynthSound) {
        self.sound = sound;
        let (attack, decay, release, sustain) = match sound {
            SynthSound::ElectricKeys => (0.004, 1.4, 0.24, 0.20),
            SynthSound::SoftPad => (0.12, 0.9, 0.75, 0.70),
            SynthSound::Bass => (0.006, 0.35, 0.14, 0.55),
            SynthSound::UserSample => (0.003, 0.5, 0.08, 1.0),
            _ => (0.004, 1.4, 0.24, 0.2),
        };
        self.attack = 1.0 / (attack * self.rate);
        self.decay = (-1.0 / (decay * self.rate)).exp();
        self.release = (-1.0 / (release * self.rate)).exp();
        self.sustain_level = sustain;
        self.refresh_font();
    }
    pub fn refresh_font(&mut self) {
        if let Some(font) = &mut self.font {
            font.select(self.sound);
        }
    }
    pub fn reset_notes(&mut self) {
        self.silence();
    }
    fn silence(&mut self) {
        if let Some(font) = &mut self.font {
            font.synth.note_off_all(true);
        }
        self.preview_remaining = 0;
        self.voices.fill(Voice::default());
        self.sustain.fill(false);
        self.state.clear_activity();
    }
    pub fn begin_block(&mut self) {
        let panic = self.state.panic.load(Relaxed);
        if panic != self.last_panic {
            self.last_panic = panic;
            self.queue.skip(self.queue.occupied_len());
            self.silence();
        }
        let sound = SynthSound::from_index(self.state.sound.load(Relaxed));
        if sound != self.sound {
            self.silence();
            self.select_sound(sound);
        }
        let audition = self.state.audition.load(Relaxed);
        if audition != self.last_audition {
            self.last_audition = audition;
            self.preview_remaining = (self.rate * 0.4) as u32;
            self.preview_note = if self.sound == SynthSound::Drums {
                36
            } else if self.sound == SynthSound::UserSample {
                self.media.root.load(Relaxed).min(127) as u8
            } else {
                60
            };
            self.handle(Message::On(15, self.preview_note, 100));
        }
        for _ in 0..QUEUE_CAPACITY {
            let Some(message) = self.queue.try_pop() else {
                break;
            };
            self.handle(message);
        }
    }
    fn handle(&mut self, message: Message) {
        if self.sound.program().is_some()
            && let Some(font) = &mut self.font
        {
            let channel = |c: u8| {
                if self.sound == SynthSound::Drums {
                    9
                } else {
                    c as i32
                }
            };
            match message {
                Message::On(c, n, v) => font.synth.note_on(channel(c), n as i32, v as i32),
                Message::Off(c, n) => font.synth.note_off(channel(c), n as i32),
                Message::Control(c, n, v) => {
                    font.synth
                        .process_midi_message(channel(c), 0xb0, n as i32, v as i32)
                }
                Message::Bend(c, v) => font.synth.process_midi_message(
                    channel(c),
                    0xe0,
                    (v & 127) as i32,
                    (v >> 7) as i32,
                ),
            }
        }
        match message {
            Message::On(channel, note, velocity) => {
                self.clock = self.clock.wrapping_add(1);
                let index = self
                    .voices
                    .iter()
                    .position(|v| !v.active)
                    .unwrap_or_else(|| {
                        self.voices
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, v)| (v.held, v.age))
                            .map_or(0, |(i, _)| i)
                    });
                self.voices[index] = Voice {
                    active: true,
                    held: true,
                    sustained: false,
                    channel,
                    note,
                    age: self.clock,
                    phase: 0.0,
                    step: self.rates[note as usize],
                    level: 0.0,
                    velocity: velocity as f32 / 127.0,
                    attacking: true,
                    sample_position: self.sample.as_ref().map_or(0.0, |clip| {
                        f32::from_bits(self.media.sample_start.load(Relaxed)) as f64
                            * clip.frames.len() as f64
                    }),
                };
            }
            Message::Off(channel, note) => {
                for v in &mut self.voices {
                    if v.channel == channel && v.note == note {
                        v.held = false;
                        v.sustained = self.sustain[channel as usize];
                    }
                }
            }
            Message::Bend(channel, value) => {
                self.bend[channel as usize] =
                    2.0_f32.powf(((value as f32 - 8192.0) / 8192.0) * 2.0 / 12.0)
            }
            Message::Control(channel, controller, value) => {
                let c = channel as usize;
                match controller {
                    1 => self.modulation[c] = value as f32 / 127.0,
                    7 => self.volume[c] = value as f32 / 127.0,
                    11 => self.expression[c] = value as f32 / 127.0,
                    64 => {
                        self.sustain[c] = value >= 64;
                        if !self.sustain[c] {
                            for v in &mut self.voices {
                                if v.channel == channel {
                                    v.sustained = false;
                                }
                            }
                        }
                    }
                    120 => {
                        for v in &mut self.voices {
                            if v.channel == channel {
                                v.active = false;
                            }
                        }
                    }
                    123 => {
                        for v in &mut self.voices {
                            if v.channel == channel {
                                v.held = false;
                                v.sustained = self.sustain[c];
                            }
                        }
                    }
                    121 => {
                        self.sustain[c] = false;
                        self.bend[c] = 1.0;
                        self.modulation[c] = 0.0;
                        self.volume[c] = 1.0;
                        self.expression[c] = 1.0;
                        for v in &mut self.voices {
                            if v.channel == channel {
                                v.sustained = false;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    pub fn tick(&mut self) -> f32 {
        if self.preview_remaining > 0 {
            self.preview_remaining -= 1;
            if self.preview_remaining == 0 {
                self.handle(Message::Off(15, self.preview_note));
            }
        }
        let tau = std::f32::consts::TAU;
        let vibrato = (self.lfo * tau).sin();
        self.lfo = (self.lfo + 5.0 / self.rate).fract();
        let mut sum = if self.sound.program().is_some() {
            self.font.as_mut().map_or(0.0, |f| f.tick())
        } else {
            0.0
        };
        for v in &mut self.voices {
            if !v.active {
                continue;
            }
            if !v.held && !v.sustained {
                v.level *= self.release;
                v.attacking = false;
            } else if v.attacking {
                v.level = (v.level + self.attack).min(1.0);
                if v.level >= 1.0 {
                    v.attacking = false;
                }
            } else {
                v.level = self.sustain_level + (v.level - self.sustain_level) * self.decay;
            }
            if v.level < 0.00001 && !v.held && !v.sustained {
                v.active = false;
                continue;
            }
            let c = v.channel as usize;
            let step = v.step * self.bend[c] * (1.0 + vibrato * self.modulation[c] * 0.009);
            v.phase = (v.phase + step).fract();
            let p = v.phase * tau;
            if self.sound.program().is_some() {
                continue;
            }
            if self.sound == SynthSound::UserSample {
                if let Some(clip) = &self.sample {
                    let start = f32::from_bits(self.media.sample_start.load(Relaxed)) as f64
                        * clip.frames.len() as f64;
                    let end = (f32::from_bits(self.media.sample_end.load(Relaxed)) as f64
                        * clip.frames.len() as f64)
                        .max(start + 1.0);
                    if v.sample_position >= end {
                        if self.media.sample_loop.load(Relaxed) && (v.held || v.sustained) {
                            v.sample_position = start;
                        } else {
                            v.active = false;
                            continue;
                        }
                    }
                    let frame = clip.at(v.sample_position);
                    let gain = (0.65 / clip.peak.max(0.01)).min(16.0);
                    sum += (frame[0] + frame[1])
                        * 0.5
                        * gain
                        * v.level
                        * v.velocity.sqrt()
                        * self.volume[c]
                        * self.expression[c];
                    let root = self.media.root.load(Relaxed).min(127) as usize;
                    v.sample_position += (v.step / self.rates[root] * clip.rate as f32 / self.rate
                        * self.bend[c]) as f64;
                } else {
                    v.active = false;
                }
                continue;
            }
            let wave = match self.sound {
                SynthSound::ElectricKeys => {
                    let index = (1.0 - step * 8.0).max(0.0) * 0.9 * v.level * v.velocity;
                    (p + (2.0 * p).sin() * index).sin()
                }
                SynthSound::SoftPad => {
                    (p.sin()
                        + if step < 0.22 {
                            0.25 * (2.0 * p).sin()
                        } else {
                            0.0
                        }
                        + if step < 0.14 {
                            0.10 * (3.0 * p).sin()
                        } else {
                            0.0
                        })
                        * 0.74
                }
                SynthSound::Bass => {
                    (p.sin()
                        + if step < 0.22 {
                            0.18 * (2.0 * p).sin()
                        } else {
                            0.0
                        })
                        * 0.84
                }
                _ => 0.0,
            };
            sum += wave * v.level * v.velocity.sqrt() * 0.13 * self.volume[c] * self.expression[c];
        }
        sum
    }
    pub fn publish_activity(&self) {
        let mut bits = [0_u64; 2];
        let mut count = 0;
        for v in &self.voices {
            if v.active {
                count += 1;
                bits[v.note as usize / 64] |= 1_u64 << (v.note % 64);
            }
        }
        self.state.voices.store(count, Relaxed);
        for (a, b) in self.state.notes.iter().zip(bits) {
            a.store(b, Relaxed);
        }
    }
}

/// Small diagnostics utility: enumerate or observe a keyboard without opening a microphone.
pub fn probe(seconds: u64) -> Result<()> {
    let (ports, note) = enumerate();
    for p in &ports {
        println!("MIDI input: {} | {}", p.name, p.id);
    }
    if ports.is_empty() {
        bail!("{note}");
    }
    let port = ports
        .iter()
        .find(|p| p.name.to_lowercase().contains("mpk"))
        .unwrap_or(&ports[0]);
    let (producer, mut consumer) = HeapRb::<Message>::new(QUEUE_CAPACITY).split();
    let state = Arc::new(State::default());
    let _connection = connect(&port.id, producer, state.clone())?;
    println!(
        "Connected to {}. Listening for {seconds} seconds; press keys/pads.",
        port.name
    );
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < seconds {
        while let Some(message) = consumer.try_pop() {
            println!("{message:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    println!(
        "Received {} supported MIDI messages; {} dropped.",
        state.messages.load(Relaxed),
        state.dropped.load(Relaxed)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (HeapProd<Message>, Instrument) {
        let (p, c) = HeapRb::<Message>::new(QUEUE_CAPACITY).split();
        (p, Instrument::new(c, Arc::new(State::default()), 48000))
    }
    #[test]
    fn parse_notes_and_controls() {
        assert_eq!(decode(&[0x92, 60, 100]), Some(Message::On(2, 60, 100)));
        assert_eq!(decode(&[0x90, 60, 0]), Some(Message::Off(0, 60)));
        assert_eq!(decode(&[0xe0, 0, 64]), Some(Message::Bend(0, 8192)));
        assert_eq!(decode(&[0x90, 60]), None);
        assert_eq!(decode(&[0xf8]), None);
        assert_eq!(decode(&[0x90, 200, 1]), None);
    }
    #[test]
    fn user_sample_pitch_trim_loop_and_release() {
        let (_, mut instrument) = setup();
        instrument.state.set_sound(SynthSound::UserSample);
        instrument.sample = Some(Arc::new(media::Clip {
            frames: vec![[0.2; 2]; 4800],
            rate: 48000,
            peak: 0.2,
        }));
        instrument.media.publish(&media::Settings {
            sample_start: 0.25,
            sample_end: 0.75,
            sample_loop: true,
            ..Default::default()
        });
        instrument.begin_block();
        instrument.handle(Message::On(0, 72, 100));
        assert_eq!(instrument.voices[0].sample_position, 1200.0);
        instrument.tick();
        assert!((instrument.voices[0].sample_position - 1202.0).abs() < 0.01);
        let count = crate::allocation_probe::count(|| {
            for _ in 0..48000 {
                assert!(instrument.tick().is_finite());
            }
            assert!(instrument.voices[0].active);
            assert!(instrument.voices[0].sample_position < 3602.0);
            instrument.handle(Message::Off(0, 72));
            for _ in 0..48000 {
                instrument.tick();
            }
        });
        assert_eq!(count, 0);
        assert!(!instrument.voices[0].active);
    }
    #[test]
    fn voices_velocity_sustain_channels_and_panic() {
        let (mut p, mut synth) = setup();
        assert_eq!(synth.tick(), 0.0);
        p.try_push(Message::On(0, 69, 127)).unwrap();
        synth.begin_block();
        let mut energy = 0.0;
        for _ in 0..4800 {
            energy += synth.tick().abs();
        }
        assert!(energy > 10.0);
        p.try_push(Message::Control(0, 64, 127)).unwrap();
        p.try_push(Message::Off(0, 69)).unwrap();
        synth.begin_block();
        for _ in 0..48000 {
            synth.tick();
        }
        synth.publish_activity();
        assert_eq!(synth.state.voices.load(Relaxed), 1);
        p.try_push(Message::Control(1, 64, 0)).unwrap();
        synth.begin_block();
        assert!(synth.voices[0].sustained);
        p.try_push(Message::Control(0, 64, 0)).unwrap();
        synth.begin_block();
        for _ in 0..48000 * 4 {
            synth.tick();
        }
        synth.publish_activity();
        assert_eq!(synth.state.voices.load(Relaxed), 0);
        for note in 30..60 {
            p.try_push(Message::On(0, note, 100)).unwrap();
        }
        synth.begin_block();
        synth.publish_activity();
        assert_eq!(synth.state.voices.load(Relaxed), 16);
        synth.state.all_notes_off();
        synth.begin_block();
        assert_eq!(synth.tick(), 0.0);
    }
    #[test]
    fn pitch_and_velocity_are_audible() {
        let (_, mut synth) = setup();
        synth.handle(Message::On(0, 69, 127));
        assert!((synth.voices[0].step * 48000.0 - 440.0).abs() < 0.001);
        synth.handle(Message::Bend(0, 16383));
        assert!((synth.bend[0] - 2.0_f32.powf(2.0 / 12.0)).abs() < 0.001);
        let (_, mut quiet) = setup();
        quiet.handle(Message::On(0, 69, 20));
        let mut loud_energy = 0.0;
        let mut quiet_energy = 0.0;
        for _ in 0..4800 {
            loud_energy += synth.tick().abs();
            quiet_energy += quiet.tick().abs();
        }
        assert!(quiet_energy < loud_energy * 0.6);
    }
    #[test]
    fn polyphony_patch_changes_and_processing_do_not_allocate() {
        let (mut p, mut synth) = setup();
        let count = crate::allocation_probe::count(|| {
            for sound in [
                SynthSound::ElectricKeys,
                SynthSound::SoftPad,
                SynthSound::Bass,
            ] {
                synth.state.set_sound(sound);
                for n in 36..60 {
                    p.try_push(Message::On(0, n, 100)).unwrap();
                }
                synth.begin_block();
                for _ in 0..4800 {
                    assert!(synth.tick().is_finite());
                }
                synth.publish_activity();
            }
            synth.state.all_notes_off();
            synth.begin_block();
        });
        assert_eq!(count, 0);
    }
    #[test]
    fn test_sound_works_without_midi_notes_and_stops() {
        let (_, mut synth) = setup();
        let mut energy = 0.0;
        synth.state.test_sound();
        let allocations = crate::allocation_probe::count(|| {
            synth.begin_block();
            for _ in 0..24000 {
                energy += synth.tick().abs();
            }
            for _ in 0..48000 * 4 {
                synth.tick();
            }
            for _ in 0..100 {
                assert_eq!(synth.tick(), 0.0);
            }
        });
        assert_eq!(allocations, 0);
        assert!(energy > 100.0);
        assert_eq!(synth.state.messages.load(Relaxed), 0);
    }
}
