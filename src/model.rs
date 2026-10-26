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
    Platform,
}
impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Asio => "ASIO · same interface",
            Self::Wasapi => "WASAPI · flexible routing",
            Self::Platform => {
                if cfg!(target_os = "macos") {
                    "CoreAudio"
                } else if cfg!(target_os = "linux") {
                    "ALSA · system audio"
                } else {
                    "System audio"
                }
            }
        }
    }
    pub fn standard() -> Self {
        if cfg!(target_os = "windows") {
            Self::Wasapi
        } else {
            Self::Platform
        }
    }
    pub fn available() -> &'static [Self] {
        #[cfg(all(target_os = "windows", feature = "asio"))]
        {
            &[Self::Asio, Self::Wasapi]
        }
        #[cfg(all(target_os = "windows", not(feature = "asio")))]
        {
            &[Self::Wasapi]
        }
        #[cfg(not(target_os = "windows"))]
        {
            &[Self::Platform]
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
            synth_sound: SynthSound::GrandPiano,
            backend: Backend::standard(),
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
    Playback,
}
impl InstrumentSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Guitar => "Guitar / audio",
            Self::MidiKeyboard => "MIDI keyboard",
            Self::GuitarAndMidi => "Guitar + MIDI",
            Self::Playback => "Audio files",
        }
    }
    pub fn has_audio(self) -> bool {
        matches!(self, Self::Guitar | Self::GuitarAndMidi)
    }
    pub fn has_midi(self) -> bool {
        matches!(self, Self::MidiKeyboard | Self::GuitarAndMidi)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SynthSound {
    #[default]
    ElectricKeys,
    SoftPad,
    Bass,
    GrandPiano,
    Rhodes,
    AcousticGuitar,
    CleanGuitar,
    FingerBass,
    Strings,
    Flute,
    Drums,
    UserSample,
}
impl SynthSound {
    pub fn label(self) -> &'static str {
        match self {
            Self::ElectricKeys => "Electric Keys",
            Self::SoftPad => "Soft Pad",
            Self::Bass => "Round Bass",
            Self::GrandPiano => "Studio Grand",
            Self::Rhodes => "Velvet Rhodes",
            Self::AcousticGuitar => "Steel Strings",
            Self::CleanGuitar => "Clean Electric",
            Self::FingerBass => "Finger Bass",
            Self::Strings => "Chamber Strings",
            Self::Flute => "Concert Flute",
            Self::Drums => "Studio Drums",
            Self::UserSample => "Your Sample",
        }
    }
    pub fn index(self) -> u32 {
        match self {
            Self::ElectricKeys => 0,
            Self::SoftPad => 1,
            Self::Bass => 2,
            Self::GrandPiano => 3,
            Self::Rhodes => 4,
            Self::AcousticGuitar => 5,
            Self::CleanGuitar => 6,
            Self::FingerBass => 7,
            Self::Strings => 8,
            Self::Flute => 9,
            Self::Drums => 10,
            Self::UserSample => 11,
        }
    }
    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::SoftPad,
            2 => Self::Bass,
            3 => Self::GrandPiano,
            4 => Self::Rhodes,
            5 => Self::AcousticGuitar,
            6 => Self::CleanGuitar,
            7 => Self::FingerBass,
            8 => Self::Strings,
            9 => Self::Flute,
            10 => Self::Drums,
            11 => Self::UserSample,
            _ => Self::ElectricKeys,
        }
    }
    pub const ALL: [Self; 12] = [
        Self::GrandPiano,
        Self::Rhodes,
        Self::AcousticGuitar,
        Self::CleanGuitar,
        Self::FingerBass,
        Self::Strings,
        Self::Flute,
        Self::Drums,
        Self::ElectricKeys,
        Self::SoftPad,
        Self::Bass,
        Self::UserSample,
    ];
    pub fn program(self) -> Option<i32> {
        match self {
            Self::GrandPiano => Some(0),
            Self::Rhodes => Some(4),
            Self::AcousticGuitar => Some(25),
            Self::CleanGuitar => Some(27),
            Self::FingerBass => Some(33),
            Self::Strings => Some(48),
            Self::Flute => Some(73),
            Self::Drums => Some(0),
            _ => None,
        }
    }
    pub fn category(self) -> &'static str {
        match self {
            Self::GrandPiano | Self::Rhodes | Self::ElectricKeys => "Keys",
            Self::AcousticGuitar | Self::CleanGuitar => "Guitars",
            Self::FingerBass | Self::Bass => "Bass",
            Self::Strings | Self::Flute => "Orchestral",
            Self::Drums => "Drums",
            Self::SoftPad => "Synth",
            Self::UserSample => "Sampler",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::GrandPiano => "A sampled acoustic piano. Clear, expressive, and ready to play.",
            Self::Rhodes => "Soft electric piano with bell-like detail and a warm body.",
            Self::AcousticGuitar => "Natural picked steel strings with a bright, intimate sound.",
            Self::CleanGuitar => "A sampled clean electric guitar for melodic lines and chords.",
            Self::FingerBass => "A rounded fingerstyle electric bass with a focused low end.",
            Self::Strings => "A sampled string ensemble for warm chords and cinematic layers.",
            Self::Flute => "A breathy concert flute with a natural acoustic character.",
            Self::Drums => "General MIDI drums. Pads and keys trigger the selected kit.",
            Self::UserSample => {
                "Turn an audio file into an instrument. Set its root note and play."
            }
            Self::SoftPad => "An airy, gentle synthesizer pad with a slow bloom.",
            Self::Bass => "A pure, rounded synth bass with subtle harmonics.",
            Self::ElectricKeys => "Our original FM keys. Bright, responsive, and lightweight.",
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
