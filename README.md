# Shredini

A standalone Windows instrument studio written in Rust. Play guitar through an
audio interface, a USB MIDI keyboard through the built-in synthesizer, or both
together. Four sliders and a compressor switch shape the sound.

## Play your MPK mini / MIDI keyboard

1. Connect the keyboard by USB and launch `shredini.exe`. With no configured
   guitar input, a detected MPK mini is selected automatically. Otherwise choose
   **MIDI keyboard** in the main Instrument selector.
2. Shredini suggests the Windows default stereo output when none is saved. Open
   **Audio & setup** to select a different speaker/headphone output or MIDI port.
   **WASAPI** works with computer audio; a guitar interface is not required.
3. Choose **Electric Keys**, **Soft Pad**, or **Round Bass**, press **Start**, and
   play. The note strip, voice count and MIDI activity counter show incoming notes.
   **Test sound** plays a short C4 tone through the same audio/effects path, even
   without pressing a key. The selected output is displayed beneath the note strip.
4. Keys and note-mode pads play the selected instrument on all MIDI channels.
   Velocity, sustain pedal (CC64), pitch bend (±2 semitones), modulation (CC1),
   volume (CC7), expression (CC11), and standard all-notes-off messages work.
5. **All notes off** clears held/sustained notes. Stop and Refresh devices after
   reconnecting a keyboard. A disconnected selected keyboard stops the engine;
   it never substitutes a different MIDI input while playing.

This is a sixteen-voice software synthesizer, not a sampled piano or a VST host.
Pads play pitched notes; a dedicated drum kit and arbitrary knob mappings are not
included. Instrument source, MIDI port and sound selection persist alongside the
existing tone settings. Tone presets continue to describe the effects; sound
selection is independent. Existing guitar sessions remain compatible.

Choose **Guitar + MIDI** to mix the guitar input and keyboard before the shared
effects. Stop before changing the source or devices; sounds can change while
playing. Selecting a different sound clears held synth voices.

If the MIDI port is busy, close other music apps using it and reconnect in
Shredini. The MPK mini 3 supports class-compliant USB MIDI.
[Akai documentation](https://support.akaipro.com/en/support/solutions/articles/69000798861-akai-pro-mpk-mini-mk3-frequently-asked-questions)

## Play guitar

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

- **Tone:** broad warm-to-bright tilt with a 65 Hz high-pass filter.
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

Requirements: current stable Rust for `x86_64-pc-windows-msvc`, Visual Studio C++
Build Tools with a Windows SDK, and LLVM/Clang for ASIO bindings. LLVM's usual
installation is `C:\Program Files\LLVM`; set `LIBCLANG_PATH` if it is elsewhere.

```powershell
.\scripts\build.ps1 -Package
```

The script downloads Steinberg's official SDK into `target/asio-sdk-download`
if `CPAL_ASIO_DIR` is unset, handles paths containing spaces, and packages the
release in `dist`. Set `CPAL_ASIO_DIR` to an existing SDK to avoid downloading it.
The SDK is a build dependency; the IK ASIO driver is a runtime dependency.
The SDK itself is not included in the portable package.
The Windows C runtime is linked statically, so the app does not need a separate
Visual C++ redistributable installation. Audio drivers are still required.

For WASAPI-only builds without the ASIO SDK or LLVM:

```powershell
.\scripts\build.ps1 -NoAsio -Package
```

The default application has no network functionality. Crate/SDK downloads happen
only during development. Dependencies are fixed by `Cargo.lock`.

## Development checks

After an ASIO build, set `CPAL_ASIO_DIR` and `LIBCLANG_PATH` as above when invoking
Cargo directly, or use `--no-default-features` for checks that do not need ASIO.

```powershell
. .\scripts\environment.ps1
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo run --locked -- --list-devices
cargo run --locked -- --list-midi
cargo run --locked -- --probe-midi
cargo run --locked -- --keyboard
cargo run --locked -- --setup
```

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
