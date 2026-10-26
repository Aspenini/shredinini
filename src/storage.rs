use crate::model::{AudioConfig, Preset, ToneParameters};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub version: u32,
    pub audio: AudioConfig,
    pub parameters: ToneParameters,
    pub current_preset: String,
    pub user_presets: Vec<Preset>,
    pub media: crate::media::Settings,
    pub library: Vec<crate::media::LibraryEntry>,
    pub backing_path: Option<PathBuf>,
    pub sample_path: Option<PathBuf>,
    pub soundfont_path: Option<PathBuf>,
    pub favorites: Vec<crate::model::SynthSound>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            version: 1,
            audio: AudioConfig::default(),
            parameters: ToneParameters::default(),
            current_preset: "Crystal Clean".into(),
            user_presets: Vec::new(),
            media: Default::default(),
            library: Vec::new(),
            backing_path: None,
            sample_path: None,
            soundfont_path: None,
            favorites: Vec::new(),
        }
    }
}

pub fn session_path() -> Option<PathBuf> {
    // Keep the original storage identifier so the rename preserves saved tones and devices.
    directories::ProjectDirs::from("", "", "Shredinini")
        .map(|d| d.data_local_dir().join("session.json"))
}

fn decode(text: &str) -> Result<Session> {
    let mut session: Session = serde_json::from_str(text)?;
    if session.version != 1 {
        bail!("Unsupported settings version {}", session.version);
    }
    session.parameters.sanitize();
    session.media.sanitize();
    for p in &mut session.user_presets {
        p.parameters.sanitize();
    }
    if ![0, 128, 256, 512].contains(&session.audio.buffer_frames) {
        session.audio.buffer_frames = 128;
    }
    session.audio.input_channel = session.audio.input_channel.min(63);
    Ok(session)
}

pub fn load(path: &Path) -> (Session, Option<String>) {
    match fs::read_to_string(path) {
        Ok(text) => match decode(&text) {
            Ok(s) => (s, None),
            Err(e) => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let backup = path.with_extension(format!("invalid-{stamp}.json"));
                match fs::copy(path, &backup) {
                    Ok(_) => (
                        Session::default(),
                        Some(format!(
                            "Settings could not be read ({e}). Defaults loaded; original preserved at {}.",
                            backup.display()
                        )),
                    ),
                    Err(copy_error) => (
                        Session::default(),
                        Some(format!(
                            "Settings could not be read ({e}). Backup failed: {copy_error}. Saving is disabled until the settings file is repaired."
                        )),
                    ),
                }
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Session::default(), None),
        Err(e) => (
            Session::default(),
            Some(format!(
                "Settings could not be opened: {e}. Saving is disabled until the settings file is accessible."
            )),
        ),
    }
}

pub fn save(path: &Path, session: &Session) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::File::create(&temp).context("Create settings temporary file")?;
    file.write_all(&serde_json::to_vec_pretty(session)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path).context("Replace settings file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_recover() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("storage-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("session.json");
        let mut s = Session::default();
        s.parameters.tone = 0.2;
        s.user_presets.push(Preset {
            name: "My tone".into(),
            parameters: s.parameters,
        });
        save(&path, &s).unwrap();
        save(&path, &s).unwrap(); // Windows replacement of an existing file.
        let (loaded, warning) = load(&path);
        assert!(warning.is_none());
        assert_eq!(loaded.parameters, s.parameters);
        assert_eq!(loaded.user_presets[0].name, "My tone");
        fs::write(&path, "{broken").unwrap();
        let (loaded, warning) = load(&path);
        assert_eq!(loaded.parameters, ToneParameters::default());
        assert!(warning.is_some());
        assert!(fs::read_dir(&root).unwrap().count() >= 2);
        for entry in fs::read_dir(&root).unwrap() {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn reject_future_version_and_bound_values() {
        assert!(decode(r#"{"version":99}"#).is_err());
        let s =
            decode(r#"{"parameters":{"tone":8.0,"output_db":100.0},"audio":{"buffer_frames":9}}"#)
                .unwrap();
        assert_eq!(s.parameters.tone, 1.0);
        assert_eq!(s.parameters.output_db, 0.0);
        assert_eq!(s.audio.buffer_frames, 128);
    }
    #[test]
    fn old_sessions_default_to_guitar_and_keyboard_settings_round_trip() {
        use crate::model::{InstrumentSource, SynthSound};
        let old = decode(r#"{"version":1,"audio":{"backend":"Wasapi"}}"#).unwrap();
        assert_eq!(old.audio.source, InstrumentSource::Guitar);
        let mut session = old;
        session.audio.source = InstrumentSource::GuitarAndMidi;
        session.audio.midi_port = "mpk-mini-port".into();
        session.audio.synth_sound = SynthSound::SoftPad;
        session.media.sample_root = 48;
        session.media.sample_loop = true;
        session.sample_path = Some(PathBuf::from("my-sound.wav"));
        session.backing_path = Some(PathBuf::from("my-track.mp3"));
        session.favorites.push(SynthSound::GrandPiano);
        session.library.push(crate::media::LibraryEntry {
            path: session.sample_path.clone().unwrap(),
            role: crate::media::Role::Sample,
        });
        let restored = decode(&serde_json::to_string(&session).unwrap()).unwrap();
        assert_eq!(restored.audio, session.audio);
        assert_eq!(restored.media.sample_root, 48);
        assert!(restored.media.sample_loop);
        assert_eq!(restored.sample_path, session.sample_path);
        assert_eq!(restored.backing_path, session.backing_path);
        assert_eq!(restored.favorites, session.favorites);
        assert_eq!(restored.library[0].path, session.library[0].path);
    }
}
