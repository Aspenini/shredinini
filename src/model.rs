use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToneParameters {
    pub tone: f32,
    pub warmth: f32,
    pub reverb: f32,
    pub delay: f32,
    pub compression: bool,
    pub input_trim_db: f32,
    pub output_db: f32,
    pub bypass: bool,
}

impl Default for ToneParameters {
    fn default() -> Self {
        Self {
            tone: 0.65,
            warmth: 0.12,
            reverb: 0.24,
            delay: 0.15,
            compression: true,
            input_trim_db: 0.0,
            output_db: -12.0,
            bypass: false,
        }
    }
}

impl ToneParameters {
    pub fn sanitize(&mut self) {
        fn bounded(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                fallback
            }
        }
        let d = Self::default();
        self.tone = bounded(self.tone, 0.0, 1.0, d.tone);
        self.warmth = bounded(self.warmth, 0.0, 1.0, d.warmth);
        self.reverb = bounded(self.reverb, 0.0, 1.0, d.reverb);
        self.delay = bounded(self.delay, 0.0, 1.0, d.delay);
        self.input_trim_db = bounded(self.input_trim_db, -24.0, 24.0, d.input_trim_db);
        self.output_db = bounded(self.output_db, -60.0, 0.0, d.output_db);
    }
}

/// UI writes targets; the audio thread owns and smooths its local copy.
pub struct SharedParameters {
    floats: [AtomicU32; 6],
    compression: AtomicBool,
    bypass: AtomicBool,
}

impl SharedParameters {
    pub fn new(p: ToneParameters) -> Self {
        let result = Self {
            floats: std::array::from_fn(|_| AtomicU32::new(0)),
            compression: AtomicBool::new(false),
            bypass: AtomicBool::new(false),
        };
        result.publish(p);
        result
    }
    pub fn publish(&self, mut p: ToneParameters) {
        p.sanitize();
        for (a, v) in self.floats.iter().zip([
            p.tone,
            p.warmth,
            p.reverb,
            p.delay,
            p.input_trim_db,
            p.output_db,
        ]) {
            a.store(v.to_bits(), Relaxed);
        }
        self.compression.store(p.compression, Relaxed);
        self.bypass.store(p.bypass, Relaxed);
    }
    pub fn read(&self) -> ToneParameters {
        let f: [f32; 6] = std::array::from_fn(|i| f32::from_bits(self.floats[i].load(Relaxed)));
        ToneParameters {
            tone: f[0],
            warmth: f[1],
            reverb: f[2],
            delay: f[3],
            input_trim_db: f[4],
            output_db: f[5],
            compression: self.compression.load(Relaxed),
            bypass: self.bypass.load(Relaxed),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Backend {
    #[default]
    Asio,
    Wasapi,
}
impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Asio => "ASIO · same interface",
            Self::Wasapi => "WASAPI · flexible routing",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub source: InstrumentSource,
    pub midi_port: String,
    pub synth_sound: SynthSound,
    pub backend: Backend,
    pub input_id: String,
    pub output_id: String,
    pub input_channel: usize,
    /// Zero means driver default.
    pub buffer_frames: u32,
}
impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            source: InstrumentSource::Guitar,
            midi_port: String::new(),
            synth_sound: SynthSound::ElectricKeys,
            backend: Backend::Asio,
            input_id: String::new(),
            output_id: String::new(),
            input_channel: 0,
            buffer_frames: 128,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstrumentSource {
    #[default]
    Guitar,
    MidiKeyboard,
    GuitarAndMidi,
}
impl InstrumentSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Guitar => "Guitar / audio",
            Self::MidiKeyboard => "MIDI keyboard",
            Self::GuitarAndMidi => "Guitar + MIDI",
        }
    }
    pub fn has_audio(self) -> bool {
        self != Self::MidiKeyboard
    }
    pub fn has_midi(self) -> bool {
        self != Self::Guitar
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SynthSound {
    #[default]
    ElectricKeys,
    SoftPad,
    Bass,
}
impl SynthSound {
    pub fn label(self) -> &'static str {
        match self {
            Self::ElectricKeys => "Electric Keys",
            Self::SoftPad => "Soft Pad",
            Self::Bass => "Round Bass",
        }
    }
    pub fn index(self) -> u32 {
        match self {
            Self::ElectricKeys => 0,
            Self::SoftPad => 1,
            Self::Bass => 2,
        }
    }
    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::SoftPad,
            2 => Self::Bass,
            _ => Self::ElectricKeys,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub parameters: ToneParameters,
}
pub fn factory_presets() -> Vec<Preset> {
    vec![
        Preset {
            name: "Crystal Clean".into(),
            parameters: ToneParameters::default(),
        },
        Preset {
            name: "Warm Clean".into(),
            parameters: ToneParameters {
                tone: 0.38,
                warmth: 0.25,
                reverb: 0.15,
                delay: 0.08,
                ..Default::default()
            },
        },
        Preset {
            name: "Ambient Clean".into(),
            parameters: ToneParameters {
                tone: 0.58,
                warmth: 0.08,
                reverb: 0.65,
                delay: 0.45,
                ..Default::default()
            },
        },
    ]
}
