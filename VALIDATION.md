# Validation record

Validation performed on Windows x64 with Rust 1.99.1. This document distinguishes
software verification from checks that require the TONEX ONE hardware.

## Automated checks

`cargo test --release --locked`: **15 passed, 0 failed**, completed in 1.47 s
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
