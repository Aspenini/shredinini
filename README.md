# Shredini

A standalone Windows instrument studio written in Rust. Play guitar through an
audio interface, a USB MIDI keyboard through sampled instruments or synthesizers,
or both together. A dark studio, sound library and simple tone controls keep the
focus on playing. No account, network connection or DAW is needed.

## Your studio

- **Instruments:** eight sampled sounds: Studio Grand, Velvet Rhodes, Steel
  Strings, Clean Electric, Finger Bass, Chamber Strings, Concert Flute and Studio
  Drums. Three original synth sounds remain available. Search, filter by family
  and pin favorites. The factory GeneralUser GS bank is embedded for offline use.
- **Backing tracks:** import WAV, MP3, FLAC or OGG/Vorbis, then play/pause, seek,
  loop and adjust track level while playing an instrument. Choose **Audio files**
  as the source for playback without a guitar or MIDI device. Tracks bypass the
  instrument effects and share the protected master output.
- **Keyboard samples:** import an audio file and play it chromatically on MIDI.
  Set its original/root note (default C4), trim the playable range and optionally
  loop while holding a key. This is a polyphonic single-sample instrument; pitch
  changes also change duration. Trim/loop points use linear interpolation, with
  no time stretching or automatic pitch detection.
- **Your banks:** import a General MIDI SF2 SoundFont to replace the factory bank.
  The same eight sound cards address standard GM programs; this release does not
  browse every custom bank/preset or support SFZ/Kontakt libraries.
- **Practice:** an adjustable 30–240 BPM metronome, output meter, master volume,
  sound audition and saved tone presets.

Use the sidebar import buttons or drop an audio file onto the studio and choose
backing track or keyboard sample. **Your audio** keeps a local library, including
the option to use the same file in either role. Imports decode on a worker before
playback; files are not streamed from disk. Limits are 512 MB per audio file and
57.6 million decoded stereo frames (20 minutes at 48 kHz), or 256 MB per SF2.
Mono files are duplicated to stereo; multichannel files use their first two
channels. The app stores original paths, not copies: moved/missing files show an
import error on the next launch. Audio, samples, trim settings, favorites and
device choices restore; playback always starts stopped.

## Play your MPK mini / MIDI keyboard

1. Connect the keyboard by USB and launch `shredini.exe`. With no configured
   guitar input, a detected MPK mini is selected automatically. Otherwise choose
   **MIDI keyboard** in the main Instrument selector.
2. Shredini suggests the Windows default stereo output when none is saved. Open
   **Audio & setup** to select a different speaker/headphone output or MIDI port.
   **WASAPI** works with computer audio; a guitar interface is not required.
3. Choose a sound card, press **Start playing**, and
   play. The note strip, voice count and MIDI activity counter show incoming notes.
   **Audition** plays a short note with the selected instrument through the same
   audio/effects path. Without a connected keyboard it starts preview mode;
   connect the keyboard and restart audio for live playing. The selected output
   remains visible in the bottom bar.
4. Keys and note-mode pads play the selected instrument on all MIDI channels.
   Velocity, sustain pedal (CC64), pitch bend (±2 semitones), modulation (CC1),
   volume (CC7), expression (CC11), and standard all-notes-off messages work.
5. **All notes off** clears held/sustained notes. Stop and Refresh devices after
   reconnecting a keyboard. A disconnected selected keyboard stops the engine;
   it never substitutes a different MIDI input while playing.

The sampled engine supports up to 64 voices; the original synth and user sampler
support sixteen voices. Selecting Studio Drums maps notes to the GM drum kit
(note 36 is kick); MIDI channel 10 also follows the bank's drum mapping. Arbitrary
knob mappings and VST hosting are not included.
Instrument source, MIDI port and sound selection persist alongside the
existing tone settings. Tone presets continue to describe the effects; sound
selection is independent. Existing guitar sessions remain compatible.

Choose **Guitar + MIDI** to mix the guitar input and keyboard before the shared
effects. Stop before changing the source or devices; sounds can change while
playing. Selecting a different sound clears held synth voices.

If the MIDI port is busy, close other music apps using it and reconnect in
Shredini. The MPK mini 3 supports class-compliant USB MIDI.
[Akai documentation](https://support.akaipro.com/en/support/solutions/articles/69000798861-akai-pro-mpk-mini-mk3-frequently-asked-questions)

## Play guitar

### USB guitar cable or any instrument interface

TONEX is optional. A guitar-to-USB cable containing its own analog-to-digital
converter acts as a small audio interface. The cable must expose an audio input
to Windows; a passive plug adapter cannot convert the signal.

1. Connect guitar → USB guitar cable → PC. Install the cable manufacturer's
   driver if its documentation requires one.
2. In Shredini, select **Guitar / audio**, open **Audio & setup**, and choose
   **WASAPI** when using the cable with separate speakers/headphones.
3. Select the cable's Windows device as **Guitar input**, normally **channel 1**.
   Device names vary by model. Mono capture is supported.
4. Select your computer speakers or headphones as **Stereo output**. An input-only
   cable does not need to supply its own audio output. Start monitoring to hear
   the guitar through Shredini's effects.

The engine negotiates supported formats and sample rates instead of requiring
TONEX's 44.1 kHz format. Separate devices use the existing adaptive resampler to
handle differing rates and clock drift. Latency and noise still depend on the
cable, driver and output device. A guitar/instrument input with suitable input
impedance is preferable to an adapter designed only for microphones.

For an example of an active interface cable, see the manufacturer's description
of [GUITAR 2 USB](https://www.behringer.com/en/products/0805-AAO). Individual cable
models have not yet been hardware-validated with Shredini.

### TONEX ONE (optional)

1. Connect guitar → TONEX ONE instrument input, then TONEX ONE → PC over USB.
2. Install IK's TONEX ONE ASIO driver using **IK Product Manager → Hardware**.
3. In TONEX Editor, use Stomp mode and bypass the pedal's internal processing.
   Turn **Direct Monitoring off** so you hear the app rather than a second dry
   copy of your guitar. These are manual hardware setup steps.
4. Run `shredini.exe` and open **Audio & setup**. Select the input device and
   guitar channel (normally channel 1).
5. For headphones connected to TONEX ONE, choose **ASIO**. ASIO capture and
   playback use the same interface. TONEX ONE USB audio runs at **44.1 kHz**.
6. For a separate wired computer output, choose **WASAPI** and select both
   endpoints. Conversion and clock correction are automatic. A Windows endpoint
   for the TONEX must be available for this path; the app reports an error if it
   is not exposed by the installed driver.
7. Start at **128 frames**; try **256**, **512**, or **driver default** if you
   hear crackling. Fixed buffer sizes are requests; diagnostics show the accepted
   sizes when the driver reports them.
8. Press **Start**. Output starts at **−12 dB**. Adjust the pedal's hardware input
   trim to avoid input clipping, then set the app output to a comfortable level.

[IK setup instructions](https://www.ikmultimedia.com/contact-support/quickfaq.php?id=1298&quick=QUICKHW)
and [TONEX ONE specifications](https://ikmultimedia.com/products/tonexone?p=manuals).

## Sounds and controls

- **Tone:** broad warm-to-bright tilt; guitar uses a 65 Hz high-pass filter and
  MIDI-only instruments use 20 Hz to retain bass fundamentals.
- **Warmth / Drive:** subtle gain-compensated soft saturation.
- **Compression:** fixed musical attack/release and ratio for even playing.
- **Delay:** filtered stereo echoes at 290/435 ms with restrained feedback.
- **Reverb:** damped algorithmic stereo space.
- **Bypass effects:** smoothly switches to dry input; input trim, output gain,
  and peak protection remain active. Stop mutes monitoring entirely.

Crystal Clean, Warm Clean, and Ambient Clean are included. **Save as** creates a
user preset; **Update**, **Rename**, and **Delete** manage it. Factory presets
cannot be overwritten. Preset loading preserves session input trim, output
volume, and bypass state to avoid unexpected level changes.

The last preset, unsaved edits, and selected devices restore on launch. Monitoring
always waits for Start. Settings save after a short idle period and on exit to
`%LOCALAPPDATA%\Shredinini\data\session.json` (the exact path appears in settings).
The original storage folder is retained so existing settings survive the rename.
Unreadable JSON or unsupported versions load defaults with a message; a copy of
the original is preserved before it can be replaced. There are no cloud services.

## Diagnostics and latency

The settings panel reports sample rates/formats, actual driver buffer sizes,
queue depth, clock correction, missing-buffer events, dropped input frames,
driver xruns, and recovery resyncs. Input and output meters have clipping flags
that reset when you start a new session.

The displayed **app queue + resampler** estimate includes queued capture data,
chunk staging, and conversion filter delay. Driver buffer durations are reported
separately when available. Neither number measures USB/converter delays or actual
round-trip latency. ASIO on the TONEX's own output is the preferred low-latency
path. Separate devices require more buffering. Bluetooth latency is not optimized.

On a disconnected/changed device or stalled stream, monitoring stops. Reconnect
uses the same selected devices; it never switches to a different speaker or mic.

## Build

For everyday development, install current stable Rust for
`x86_64-pc-windows-msvc` and Visual Studio C++ Build Tools with a Windows SDK:

```powershell
cargo run
```

Plain Cargo builds use WASAPI and require no ASIO SDK or LLVM/Clang. This supports
MIDI keyboards, USB guitar cables, audio interfaces with Windows endpoints, and
computer speakers/headphones. An existing ESP32 `LIBCLANG_PATH` will not affect
the default build. `--no-default-features` is no longer needed to omit ASIO.

For the full release including optional ASIO, also install **desktop** LLVM/Clang
(normally `C:\Program Files\LLVM`) and use:

```powershell
.\scripts\build.ps1 -Package
```

The script downloads Steinberg's official SDK into `target/asio-sdk-download`
if `CPAL_ASIO_DIR` is unset, handles paths containing spaces, and packages the
release in `dist`. Set `CPAL_ASIO_DIR` to an existing SDK to avoid downloading it.
The SDK is an ASIO build dependency. A manufacturer's ASIO driver is needed only
when using that interface through ASIO; WASAPI does not require the IK driver.
The SDK itself is not included in the portable package.
The script explicitly enables the `asio` feature, initializes the Visual Studio
environment, chooses installed desktop LLVM over an inherited ESP32 compiler,
and targets Windows x64 for binding generation. These changes apply only to that
PowerShell process. It does not change global environment variables. If desktop
LLVM is installed elsewhere, set `LIBCLANG_PATH` to its directory; the standard
Program Files installation takes priority when present.
The Windows C runtime is linked statically, so the app does not need a separate
Visual C++ redistributable installation. Audio drivers are still required.

For WASAPI-only builds without the ASIO SDK or LLVM:

```powershell
.\scripts\build.ps1 -NoAsio -Package
```

The default application has no network functionality. Crate/SDK downloads happen
only during development. Dependencies are fixed by `Cargo.lock`.

## Development checks

Default checks and device discovery need no ASIO setup:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run --locked -- --list-devices
cargo run --locked -- --list-midi
cargo run --locked -- --probe-midi
cargo run --locked -- --keyboard
cargo run --locked -- --setup
```

To develop or check ASIO after the build script has prepared its SDK:

```powershell
. .\scripts\environment.ps1 -Asio
cargo run --locked --features asio
cargo clippy --locked --all-targets --all-features -- -D warnings
```

If an old ASIO build fails with `CodeFragments.hpp` or a pointer-size assertion,
use the ASIO environment command above before retrying. Do not delete the entire
Cargo cache or replace your global ESP32 toolchain settings.

Optional rendered UI check (the app exits after saving the screenshot):

```powershell
$env:EFRAME_SCREENSHOT_TO = "$PWD\target\studio.png"
cargo run --locked --features screenshot
Remove-Item Env:EFRAME_SCREENSHOT_TO
```

## Architecture

`audio` owns enumeration and a dedicated stream-control thread. A bounded command
channel separates UI work from device management. Capture callbacks normalize
one selected channel into a preallocated SPSC ring. The output callback owns
DSP and scratch storage, consuming 128-frame blocks and adapting arbitrary driver
callback sizes. WASAPI uses Rubato's asynchronous sinc conversion with a bounded,
gradual PI correction for independent clock drift. ASIO uses its common hardware
clock without resampling. Starvation produces silence; recovery fades in over
5 ms, and excessive backlog is discarded to prevent stale monitoring.

`midi` discovers MIDI input ports and passes decoded messages through a bounded
SPSC queue. The output callback owns a preallocated sixteen-voice instrument;
MIDI-only mode opens no microphone or guitar capture stream. Guitar + MIDI mixes
the synthesizer with captured audio at the output sample rate. MIDI presence is
checked on the control thread, while idle keyboards remain playable indefinitely.
Queue overflow triggers all-notes-off recovery to prevent stuck notes.

`media` decodes local audio on a worker and prepares PCM, SoundFonts and synth
buffers before handing them to the output callback. Bounded update/retirement
rings transfer resources; old buffers and synth instances are destroyed on the
control thread. The factory bank uses RustySynth with 64-sample render blocks and
64 voices. Sampled instruments are summed to mono before the shared stereo
effects; backing tracks retain stereo and interpolate at the output clock.

`dsp` contains the effects. The callback reads sanitized atomic parameter targets;
gain and effects changes are smoothed over 20 ms. `model` defines configuration,
parameters and presets. `storage` writes versioned JSON through a synced temporary
file and replacement rename. `ui` contains the native egui interface.

Application callback paths perform no allocations, locks, filesystem access, or
logging. Tests instrument allocations, including the resampler and recovery.
This does not claim that third-party drivers or CPAL internals are lock-free;
CPAL's ASIO backend has its own synchronization.

See [VALIDATION.md](VALIDATION.md) for measured checks and outstanding hardware
validation. VST hosting, recording, amp/cab models, chorus, hardware control, and
other operating systems are outside this release.
