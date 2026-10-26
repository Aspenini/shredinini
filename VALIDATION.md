# Validation record

Validation performed on Windows x64 with Rust 1.99.1. This document distinguishes
software verification from checks that require the TONEX ONE hardware.

## Automated checks

`cargo test --release --locked`: **21 passed, 0 failed**, completed in 1.52 s
on the final build with the statically linked Windows C runtime.
Formatting and Clippy (`--all-targets --all-features -- -D warnings`) passed.
The filesystem's incremental-cache hard-link warning does not affect compilation.

- DSP: exact silence, finite/bounded output at 44.1/48/96 kHz, extreme controls,
  non-finite input handling, stereo impulse tails and decay, dry bypass, and
  smoothing of level changes.
- Audio pipeline: variable callback lengths, startup silence, starvation and
  recovery, and zero application callback allocations after construction,
  including live parameter updates, sinc resampling and recovery.
- Independent clocks: simulate 90 seconds for 44.1→48 kHz at ±500 ppm,
  44.1→44.1 kHz at +400 ppm, and 48→44.1 kHz at −400 ppm. Require finite output,
  no underruns/overruns, bounded queue depth, and converged rate correction.
- Persistence: round-trip user presets/settings, replace an existing file on
  Windows, recover malformed JSON while preserving the original, reject future
  schema versions, and bound persisted numeric controls.
- MIDI: note-on/off (including velocity-zero note-on), channel handling, pitch
  bend, velocity response, sustain/release, voice stealing and all-notes-off.
- All three instrument sounds and both MIDI-only and guitar+MIDI render paths
  produce bounded finite output without callback allocations. MIDI-only playback
  needs no capture samples, does not report capture underruns, and mutes on faults.
- Legacy sessions default to guitar; MIDI source, port and sound persist correctly.
- The Test sound tone generates without MIDI notes, ends automatically, and does
  not allocate or increment the hardware MIDI message counter.

Observed drift simulations (90 seconds each, 512-frame queue target):

| Input → output | Input clock offset | Queue range after warmup | Final correction | Dropouts |
| --- | --- | --- | --- | --- |
| 44.1 → 48 kHz | +500 ppm | 432–536 frames | −494.3 ppm | 0 |
| 44.1 → 48 kHz | −500 ppm | 333–568 frames | +526.3 ppm | 0 |
| 44.1 → 44.1 kHz | +400 ppm | 431–537 frames | −392.4 ppm | 0 |
| 48 → 44.1 kHz | −400 ppm | 354–559 frames | +416.1 ppm | 0 |

## Desktop checks

### Windows development build fix (October 25, 2026)

- Identified the cause from the build log: default ASIO compilation inherited an
  ESP32 `LIBCLANG_PATH`, selected the wrong SDK platform branch and failed the
  64-bit binding layout check. No global compiler settings were changed.
- Default Cargo features now omit ASIO. `cargo run --locked -- --list-midi`
  compiled and exited successfully in the original shell environment, with no
  compiler setup script or SDK/Clang override. It reported no connected MIDI input.
- `environment.ps1 -Asio` selected desktop LLVM and the Windows x64 target;
  `cargo check --locked --features asio` passed with ASIO enabled. The release
  script explicitly enables ASIO unless `-NoAsio` is requested.
- The default-feature release test suite passed all 21 tests (1.49 s), and Clippy
  with all targets/features passed. Both PowerShell scripts parsed successfully;
  the non-ASIO environment setup preserves the inherited Clang configuration.

### Instrument studio (Shredini 0.3.0, October 25, 2026)

- New automated coverage decodes synthetic WAV, MP3, FLAC and Ogg/Vorbis,
  checks rate/duration/finite PCM and mono duplication, and rejects malformed
  audio and SoundFonts. The embedded bank renders all eight sampled programs
  without callback allocations, including program changes.
- The user sampler verifies octave pitch, trim boundaries, held looping, note
  release and allocation-free rendering. Backing tests cover pause, seek, looping
  and rate conversion without allocations. Large replaced resources survive
  until the control thread drains the retirement ring. Media paths, sample
  settings, library and pinned sounds round-trip with existing sessions.
- Launched the redesigned native interface and visually checked its sound
  library, file library, tone controls and fixed master/transport bar. The final
  layout puts tone and backing playback on Studio, with a separate Sound library;
  that final layout adjustment awaits a second visual inspection.
- Imported a generated five-second stereo WAV through the native picker; its
  library entry, duration and waveform appeared. Started without capture or MIDI
  hardware using Audio files and the saved **Headphones (Arctis Nova 7P)** output.
  Observed end-of-file stop and repeated looping with output meter activity.
- Accepted WASAPI format: **48,000 Hz, stereo F32, 480 frames**, despite the
  128-frame request. The short playback check showed zero underruns, dropped
  input frames, driver xruns and resyncs. This is not a 30-minute soak or a latency
  measurement, and the user has not confirmed listening to this new build.
- Built a portable 0.3.0 package with its embedded bank and bank/ASIO licenses.
  Computer use was stopped with physical Escape before final packaged UI checks.
  Native sample editing, live custom-SF2 replacement, drag/drop, no-keyboard
  audition and minimum-size layout still need hands-on verification. No new
  physical keyboard performance check is claimed.

### Generic USB guitar input support (Shredini 0.2.2)

- The guitar engine enumerates generic Windows input devices and accepts a
  selected mono channel; it has no TONEX-specific device or sample-rate gate.
- Input-only cables use WASAPI capture plus an independently selected stereo
  output, with the existing tested sample-rate conversion and drift correction.
- Setup guidance now starts with USB guitar cables and generic interfaces;
  TONEX-specific instructions are optional. No IK driver is required for WASAPI.
- A physical USB guitar cable was not available for this update. Electrical
  input quality, actual driver compatibility and measured cable latency remain
  unvalidated for individual models.

### MIDI hardware check (Shredini 0.2.0)

- Detected the connected **MPK mini 3** through the Windows MIDI API.
- The packaged application selected MIDI keyboard / Electric Keys and opened
  WASAPI playback at **48 kHz** with no guitar capture device selected.
- Observed **72 incoming hardware MIDI messages**, last note **A3**, and **13
  active synth voices** during playing. The engine remained LIVE, confirming
  that MIDI note input reaches the real-time synthesizer on this machine.
- The actual saved session retained MIDI source, keyboard port, Electric Keys,
  selected output and tone edits in the existing application-data location.
- In 0.2.1, verified the displayed output is **Headphones (Arctis Nova 7P)**,
  matching the user's selection. Test sound opens MIDI/WASAPI playback from
  standby and returns the engine to LIVE at 48 kHz. On October 24, 2026, the user
  confirmed hearing the test note through the Arctis headphones.
- Physical sustain/joystick testing and USB unplug/replug testing remain separate
  from the automated protocol tests. No measured MIDI
  key-to-speaker latency is claimed.

### Original guitar/UI checks

- Native application launches and renders its main window.
- Windows endpoint enumeration works. On this machine it exposes WASAPI capture
  and stereo playback endpoints. No TONEX ONE or ASIO device was enumerated in
  the initial guitar validation.
- Monitoring does not start automatically.
- Native UI automation verified opening/closing setup, enumerating the input
  picker, loading Warm Clean and restoring Crystal Clean, and the save dialog's
  disabled action for an empty name. Monitoring was left stopped throughout.
- The final packaged executable launched successfully and restored Crystal Clean
  from the actual local application-data settings file. Its DLL imports contain
  only Windows system libraries, with no Visual C++ runtime DLL dependency.

## Awaiting TONEX ONE hardware

These checks are **not completed** and no measured guitar round-trip latency is
claimed:

1. Verify dry USB input, correct channel selection and hardware direct-monitor
   disablement with the installed pedal firmware/IK driver.
2. Verify ASIO input/output at 44.1 kHz and 128/256/512/default buffers; record
   the driver's accepted sizes and any unsupported configurations.
3. Verify TONEX WASAPI input with a separate wired 44.1/48 kHz output endpoint.
4. Disconnect/reconnect USB during monitoring; require stopped monitoring,
   silent playback, an actionable error, and manual reconnection to the same
   selected endpoints.
5. Play for 30 minutes on each routing path while moving controls, switching
   presets and operating settings. Record underruns, dropped frames, driver
   xruns, resyncs and audible artifacts.
6. Measure round-trip latency with an electrical loopback/impulse measurement
   at each working buffer size. Record the hardware, driver and firmware versions,
   rates, accepted buffers and measured milliseconds alongside app estimates.

| Route | Buffer requested | Buffer accepted | Duration | Dropouts | Round-trip |
| --- | --- | --- | --- | --- | --- |
| TONEX ASIO → TONEX output | 128/256/512/default | Pending | Pending | Pending | Not measured |
| TONEX WASAPI → wired PC output | 128/256/512/default | Pending | Pending | Pending | Not measured |
