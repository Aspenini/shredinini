# Third-party components

Shredini's original source is MIT licensed. Dependency versions and sources are
recorded in Cargo.lock. The application uses CPAL (Apache-2.0), egui/eframe
(MIT OR Apache-2.0), ringbuf (MIT OR Apache-2.0), Rubato (MIT), biquad (MIT),
Serde (MIT OR Apache-2.0), and their transitive dependencies under their respective
licenses. Dependency license texts remain available in their published sources.
USB MIDI connectivity uses midir (MIT). RustySynth (MIT) renders SoundFonts;
Symphonia (MPL-2.0) decodes audio files; rfd (MIT) provides native file dialogs.
The original oscillator/envelope and single-sample instrument code is part of
Shredini.

The browser edition uses wasm-bindgen (MIT OR Apache-2.0) to expose the same
Rust sound engine to its AudioWorklet and decoder Workers. Its original browser
interface and UTF-8 worklet fallback are covered by the project's MIT license.
Native Linux and macOS distributions use the corresponding CPAL/midir backends;
the optional Steinberg SDK is included only in Windows ASIO builds.

The embedded GeneralUser GS v2.0.3 SoundFont is by S. Christian Collins and its
contributors, under its own License v2.0. The complete unmodified license is in
GeneralUser-GS-LICENSE.txt in portable distributions and assets/ in the source.
It permits use in software and private/commercial music creation. Its license
also explains the author's uncertainty about the historical provenance of some
samples; retain and review that statement for commercial redistribution.
Source and pinned hash are recorded in assets/README.md. This bank is not covered
by Shredini's MIT license. User-imported audio and SoundFonts retain their own
rights and are referenced locally, never uploaded.

ASIO is a trademark and software of Steinberg Media Technologies GmbH.
ASIO-enabled builds include Steinberg ASIO SDK components. The SDK provides
Steinberg's ASIO license or the GPL v3 alternative, with some helper files
carrying their own permissive notices. The build package includes the SDK's
top-level license in ASIO-SDK-LICENSE.txt. The application source MIT license
does not replace the SDK's terms. The SDK remains in the development build
directory and is not repackaged here.

IK Multimedia and TONEX ONE names identify supported hardware. This application
does not include or redistribute IK software, drivers, Tone Models, or presets.
