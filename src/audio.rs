//! Device ownership stays on the control thread. Application callbacks only touch
//! preallocated audio state, an SPSC ring, and lock-free scalar atomics.
use crate::{
    dsp::Processor,
    midi,
    model::{AudioConfig, Backend, InstrumentSource, SharedParameters},
};
use anyhow::{Context, Result, bail};
use cpal::{
    Device, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use ringbuf::{HeapCons, HeapProd, HeapRb, traits::*};
use rubato::{
    Adjustable, Async, FixedAsync, Resampler, SincInterpolationParameters, WindowFunction,
    audioadapter_buffers::direct::InterleavedSlice,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

const CHUNK: usize = 128;
const RING_CAPACITY: usize = 16384;

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub backend: Backend,
    pub input_channels: usize,
    pub output_channels: usize,
    pub default_output: bool,
}
#[derive(Default)]
pub struct Diagnostics {
    pub input_peak: AtomicU32,
    pub output_peak: AtomicU32,
    pub input_clips: AtomicU64,
    pub output_clips: AtomicU64,
    pub underruns: AtomicU64,
    pub overruns: AtomicU64,
    pub xruns: AtomicU64,
    pub resyncs: AtomicU64,
    pub input_frames: AtomicU64,
    pub output_frames: AtomicU64,
    pub queue_frames: AtomicU32,
    pub app_latency_ms: AtomicU32,
    pub correction_ppm: AtomicU32,
    pub fatal: AtomicBool,
    pub realtime_denied: AtomicBool,
}
impl Diagnostics {
    pub fn reset(&self) {
        for a in [
            &self.input_peak,
            &self.output_peak,
            &self.queue_frames,
            &self.app_latency_ms,
            &self.correction_ppm,
        ] {
            a.store(0, Relaxed);
        }
        for a in [
            &self.input_clips,
            &self.output_clips,
            &self.underruns,
            &self.overruns,
            &self.xruns,
            &self.resyncs,
            &self.input_frames,
            &self.output_frames,
        ] {
            a.store(0, Relaxed);
        }
        self.fatal.store(false, Relaxed);
        self.realtime_denied.store(false, Relaxed);
    }
}
fn peak(a: &AtomicU32, value: f32) {
    a.fetch_max(value.abs().to_bits(), Relaxed);
}

pub enum Command {
    Refresh,
    Start(AudioConfig),
    Stop,
    Shutdown,
}
pub enum Event {
    Devices(Vec<DeviceInfo>, String),
    MidiDevices(Vec<midi::Port>, String),
    Running(RunningInfo),
    Stopped,
    Failed(String),
}
#[derive(Clone)]
pub struct RunningInfo {
    pub source: InstrumentSource,
    pub input_rate: u32,
    pub output_rate: u32,
    pub input_buffer: Option<u32>,
    pub output_buffer: Option<u32>,
    pub input_channels: usize,
    pub output_channels: usize,
    pub resampling: bool,
    pub input_format: String,
    pub output_format: String,
}

pub struct Engine {
    pub commands: SyncSender<Command>,
    pub events: Receiver<Event>,
    pub diagnostics: Arc<Diagnostics>,
    pub midi: Arc<midi::State>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Engine {
    pub fn new(parameters: Arc<SharedParameters>) -> Self {
        let (commands, rx) = mpsc::sync_channel(8);
        let (tx, events) = mpsc::channel();
        let diagnostics = Arc::new(Diagnostics::default());
        let d = diagnostics.clone();
        let midi = Arc::new(midi::State::default());
        let m = midi.clone();
        let handle=thread::Builder::new().name("audio-control".into()).spawn(move || {
            let mut streams:Option<ActiveStreams>=None;
            let mut last_counts=(0,0);
            let mut last_activity=Instant::now();
            let mut last_midi_check=Instant::now();
            let (devices,note)=enumerate_devices();
            let _=tx.send(Event::Devices(devices,note));
            let (ports,note)=midi::enumerate();let _=tx.send(Event::MidiDevices(ports,note));
            loop {
                match rx.recv_timeout(Duration::from_millis(25)) {
                    Ok(Command::Shutdown)|Err(mpsc::RecvTimeoutError::Disconnected)=>break,
                    Ok(Command::Refresh)=>{
                        if streams.is_none() {
                            let (devices,note)=enumerate_devices(); let _=tx.send(Event::Devices(devices,note));
                            let (ports,note)=midi::enumerate();let _=tx.send(Event::MidiDevices(ports,note));
                        }
                    }
                    Ok(Command::Stop)=>{streams=None;m.clear_activity();let _=tx.send(Event::Stopped);}
                    Ok(Command::Start(config))=>{
                        streams=None; d.reset();m.clear_activity();m.messages.store(0,Relaxed);m.dropped.store(0,Relaxed);m.set_sound(config.synth_sound);
                        match open_streams(&config,parameters.clone(),d.clone(),m.clone()) {
                            Ok((active,info))=>{
                                streams=Some(active); last_counts=(0,0);last_activity=Instant::now();last_midi_check=Instant::now();
                                let _=tx.send(Event::Running(info));
                            }
                            Err(e)=>{let _=tx.send(Event::Failed(format!("{e:#}")));}
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout)=>{}
                }
                if let Some(active)=streams.as_ref() {
                    let counts=(d.input_frames.load(Relaxed),d.output_frames.load(Relaxed));
                    if (active.input.is_none() || counts.0!=last_counts.0) && counts.1!=last_counts.1 {
                        last_activity=Instant::now();last_counts=counts;
                    }
                    if let Some(id)=active.midi_port.as_ref() && last_midi_check.elapsed()>Duration::from_secs(1) {
                            last_midi_check=Instant::now();
                            let (ports,_)=midi::enumerate();
                            if !ports.iter().any(|p|p.id==*id) {d.fatal.store(true,Relaxed);}
                    }
                    if d.fatal.load(Relaxed) || last_activity.elapsed()>Duration::from_secs(3) {
                        d.fatal.store(true,Relaxed); streams=None;m.clear_activity();
                        let _=tx.send(Event::Failed("Audio or MIDI device stopped or changed. Check its connection and driver, then use Reconnect. The selected output has not been changed.".into()));
                    }
                }
            }
            drop(streams);
        }).expect("create audio control thread");
        Self {
            commands,
            events,
            diagnostics,
            midi,
            thread: Some(handle),
        }
    }
    pub fn send(&self, command: Command) -> bool {
        self.commands.try_send(command).is_ok()
    }
}

struct ActiveStreams {
    input: Option<Stream>,
    _output: Stream,
    _midi: Option<midir::MidiInputConnection<()>>,
    midi_port: Option<String>,
}
impl Drop for Engine {
    fn drop(&mut self) {
        // No UI-thread join: some device drivers can take a long time to close.
        let _ = self.commands.try_send(Command::Shutdown);
        self.thread.take();
    }
}

fn host(backend: Backend) -> Result<cpal::Host> {
    match backend {
        Backend::Wasapi => Ok(cpal::host_from_id(cpal::HostId::Wasapi)?),
        Backend::Asio => {
            #[cfg(feature = "asio")]
            {
                Ok(cpal::host_from_id(cpal::HostId::Asio)?)
            }
            #[cfg(not(feature = "asio"))]
            {
                bail!("This build has no ASIO support. Select WASAPI or use the ASIO release.")
            }
        }
    }
}
pub fn enumerate_devices() -> (Vec<DeviceInfo>, String) {
    let mut result = Vec::new();
    let mut notes = Vec::new();
    for backend in [Backend::Asio, Backend::Wasapi] {
        match host(backend).and_then(|h| {
            Ok((
                h.default_output_device().and_then(|d| d.id().ok()),
                h.devices()?,
            ))
        }) {
            Ok((default_output, devices)) => {
                for d in devices {
                    let Ok(id) = d.id() else { continue };
                    let input_channels = d
                        .supported_input_configs()
                        .ok()
                        .and_then(|c| c.map(|r| r.channels() as usize).max())
                        .unwrap_or(0);
                    let output_channels = d
                        .supported_output_configs()
                        .ok()
                        .and_then(|c| c.map(|r| r.channels() as usize).max())
                        .unwrap_or(0);
                    if input_channels + output_channels > 0 {
                        result.push(DeviceInfo {
                            id: id.to_string(),
                            name: d.to_string(),
                            backend,
                            input_channels,
                            output_channels,
                            default_output: default_output.as_ref() == Some(&id),
                        });
                    }
                }
            }
            Err(e) => notes.push(format!("{}: {e}", backend.label())),
        }
    }
    if !result.iter().any(|d| d.backend == Backend::Asio) {
        notes.push(
            "No ASIO device found. Install the IK driver for TONEX ONE, or select WASAPI.".into(),
        );
    }
    (result, notes.join("\n"))
}
pub fn print_devices() {
    let (devices, note) = enumerate_devices();
    for d in devices {
        println!(
            "{} | {} | in {} / out {} | {}",
            d.backend.label(),
            d.name,
            d.input_channels,
            d.output_channels,
            d.id
        );
    }
    if !note.is_empty() {
        println!("{note}");
    }
}

fn usable_format(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32
            | SampleFormat::F64
            | SampleFormat::I8
            | SampleFormat::I16
            | SampleFormat::I24
            | SampleFormat::I32
            | SampleFormat::I64
            | SampleFormat::U8
            | SampleFormat::U16
            | SampleFormat::U32
            | SampleFormat::U64
    )
}
fn select_config(
    device: &Device,
    input: bool,
    preferred: u32,
    min_channels: usize,
    exact_rate: bool,
) -> Result<SupportedStreamConfig> {
    let ranges: Vec<_> = if input {
        device.supported_input_configs()?.collect()
    } else {
        device.supported_output_configs()?.collect()
    };
    if let Some(range) = ranges
        .iter()
        .filter(|r| {
            r.channels() as usize >= min_channels
                && usable_format(r.sample_format())
                && r.min_sample_rate() <= preferred
                && r.max_sample_rate() >= preferred
        })
        .min_by_key(|r| {
            (
                r.channels(),
                if r.sample_format() == SampleFormat::F32 {
                    0
                } else {
                    1
                },
            )
        })
    {
        return Ok((*range).with_sample_rate(preferred));
    }
    if exact_rate {
        bail!(
            "The selected ASIO interface cannot run input and output at {preferred} Hz with these channels."
        );
    }
    let default = if input {
        device.default_input_config()?
    } else {
        device.default_output_config()?
    };
    if default.channels() as usize >= min_channels && usable_format(default.sample_format()) {
        return Ok(default);
    }
    bail!(
        "No supported {} format with at least {min_channels} channels",
        if input { "input" } else { "output" }
    )
}
fn stream_config(config: &SupportedStreamConfig, requested: u32) -> StreamConfig {
    let mut c = config.config();
    c.buffer_size = if requested == 0 {
        cpal::BufferSize::Default
    } else {
        cpal::BufferSize::Fixed(requested)
    };
    c
}

fn open_streams(
    config: &AudioConfig,
    parameters: Arc<SharedParameters>,
    d: Arc<Diagnostics>,
    midi_state: Arc<midi::State>,
) -> Result<(ActiveStreams, RunningInfo)> {
    if !config.source.has_audio() {
        return open_keyboard(config, parameters, d, midi_state);
    }
    let h = host(config.backend)?;
    if config.backend == Backend::Asio && config.input_id != config.output_id {
        bail!(
            "ASIO requires input and output on the same interface. Select WASAPI for computer output."
        );
    }
    let input = h
        .device_by_id(&config.input_id.parse().context("Select a guitar input")?)
        .context("Selected input is not connected")?;
    let output = h
        .device_by_id(&config.output_id.parse().context("Select an output")?)
        .context("Selected output is not connected")?;
    let ic = select_config(&input, true, 44100, config.input_channel + 1, false)?;
    let oc = select_config(
        &output,
        false,
        ic.sample_rate(),
        2,
        config.backend == Backend::Asio,
    )?;
    let mut is = stream_config(&ic, config.buffer_frames);
    let mut os = stream_config(&oc, config.buffer_frames);
    let mut last_error = None;
    // A fixed buffer is a request. If the driver rejects it, try its own default
    // and report the actual sizes rather than pretending that 128 was accepted.
    for attempt in 0..2 {
        if attempt == 1 {
            if config.buffer_frames == 0 {
                break;
            }
            is.buffer_size = cpal::BufferSize::Default;
            os.buffer_size = cpal::BufferSize::Default;
            d.reset();
        }
        match prepare_pair(
            &input,
            &output,
            &is,
            &os,
            ic.sample_format(),
            oc.sample_format(),
            config,
            parameters.clone(),
            d.clone(),
            midi_state.clone(),
        ) {
            Ok((i, o, resampling, connection)) => {
                let info = RunningInfo {
                    source: config.source,
                    input_rate: is.sample_rate,
                    output_rate: os.sample_rate,
                    input_buffer: i.buffer_size().ok(),
                    output_buffer: o.buffer_size().ok(),
                    input_channels: is.channels as usize,
                    output_channels: os.channels as usize,
                    resampling,
                    input_format: format!("{:?}", ic.sample_format()),
                    output_format: format!("{:?}", oc.sample_format()),
                };
                // Both ASIO streams must be built before either is started.
                i.play().context("Start guitar capture")?;
                o.play().context("Start playback")?;
                return Ok((
                    ActiveStreams {
                        input: Some(i),
                        _output: o,
                        _midi: connection,
                        midi_port: config.source.has_midi().then(|| config.midi_port.clone()),
                    },
                    info,
                ));
            }
            Err(e) => last_error = Some(e),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unable to open audio streams"))).context(
        "Could not open the selected devices; check driver access, channel, and buffer settings",
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_pair(
    input: &Device,
    output: &Device,
    ic: &StreamConfig,
    oc: &StreamConfig,
    ifmt: SampleFormat,
    ofmt: SampleFormat,
    config: &AudioConfig,
    parameters: Arc<SharedParameters>,
    d: Arc<Diagnostics>,
    midi_state: Arc<midi::State>,
) -> Result<(Stream, Stream, bool, Option<midir::MidiInputConnection<()>>)> {
    let (producer, consumer) = HeapRb::<f32>::new(RING_CAPACITY).split();
    let resampling = config.backend == Backend::Wasapi;
    let requested = match ic.buffer_size {
        cpal::BufferSize::Fixed(n) => n as usize,
        _ => 256,
    };
    let target = if resampling {
        requested
            .saturating_mul(2)
            .max((ic.sample_rate as usize) / 100)
    } else {
        requested.max(CHUNK)
    };
    let mut monitor = Monitor::new(
        consumer,
        ic.sample_rate,
        oc.sample_rate,
        resampling,
        target,
        parameters,
        d.clone(),
    )?;
    let connection = if config.source.has_midi() {
        Some(attach_keyboard(&mut monitor, config, midi_state)?)
    } else {
        None
    };
    macro_rules! capture {
        ($t:ty) => {
            build_capture::<$t>(input, ic.clone(), config.input_channel, producer, d.clone())?
        };
    }
    let i = match ifmt {
        SampleFormat::F32 => capture!(f32),
        SampleFormat::F64 => capture!(f64),
        SampleFormat::I8 => capture!(i8),
        SampleFormat::I16 => capture!(i16),
        SampleFormat::I24 => capture!(cpal::I24),
        SampleFormat::I32 => capture!(i32),
        SampleFormat::I64 => capture!(i64),
        SampleFormat::U8 => capture!(u8),
        SampleFormat::U16 => capture!(u16),
        SampleFormat::U32 => capture!(u32),
        SampleFormat::U64 => capture!(u64),
        _ => bail!("Unsupported capture sample format"),
    };
    let o = open_playback(output, oc, ofmt, monitor, d)?;
    Ok((i, o, resampling, connection))
}

fn open_playback(
    output: &Device,
    oc: &StreamConfig,
    ofmt: SampleFormat,
    monitor: Monitor,
    d: Arc<Diagnostics>,
) -> Result<Stream> {
    macro_rules! playback {
        ($t:ty) => {
            build_playback::<$t>(output, oc.clone(), monitor, d)?
        };
    }
    let o = match ofmt {
        SampleFormat::F32 => playback!(f32),
        SampleFormat::F64 => playback!(f64),
        SampleFormat::I8 => playback!(i8),
        SampleFormat::I16 => playback!(i16),
        SampleFormat::I24 => playback!(cpal::I24),
        SampleFormat::I32 => playback!(i32),
        SampleFormat::I64 => playback!(i64),
        SampleFormat::U8 => playback!(u8),
        SampleFormat::U16 => playback!(u16),
        SampleFormat::U32 => playback!(u32),
        SampleFormat::U64 => playback!(u64),
        _ => bail!("Unsupported playback sample format"),
    };
    Ok(o)
}

fn attach_keyboard(
    monitor: &mut Monitor,
    config: &AudioConfig,
    state: Arc<midi::State>,
) -> Result<midir::MidiInputConnection<()>> {
    let (producer, consumer) = HeapRb::<midi::Message>::new(midi::QUEUE_CAPACITY).split();
    let connection = midi::connect(&config.midi_port, producer, state.clone())?;
    monitor.instrument = Some(midi::Instrument::new(consumer, state, monitor.output_rate));
    Ok(connection)
}

fn open_keyboard(
    config: &AudioConfig,
    parameters: Arc<SharedParameters>,
    d: Arc<Diagnostics>,
    state: Arc<midi::State>,
) -> Result<(ActiveStreams, RunningInfo)> {
    let h = host(config.backend)?;
    let output = h
        .device_by_id(&config.output_id.parse().context("Select a stereo output")?)
        .context("Selected output is disconnected")?;
    let preferred = output.default_output_config()?.sample_rate();
    let supported = select_config(&output, false, preferred, 2, false)?;
    let mut stream_config = stream_config(&supported, config.buffer_frames);
    let mut error = None;
    for attempt in 0..2 {
        if attempt == 1 {
            if config.buffer_frames == 0 {
                break;
            }
            stream_config.buffer_size = cpal::BufferSize::Default;
            d.reset();
        }
        let (_, consumer) = HeapRb::<f32>::new(1).split();
        let mut monitor = Monitor::new(
            consumer,
            stream_config.sample_rate,
            stream_config.sample_rate,
            false,
            128,
            parameters.clone(),
            d.clone(),
        )?;
        monitor.audio_enabled = false;
        let connection = attach_keyboard(&mut monitor, config, state.clone())?;
        match open_playback(
            &output,
            &stream_config,
            supported.sample_format(),
            monitor,
            d.clone(),
        ) {
            Ok(output) => {
                let info = RunningInfo {
                    source: config.source,
                    input_rate: 0,
                    output_rate: stream_config.sample_rate,
                    input_buffer: None,
                    output_buffer: output.buffer_size().ok(),
                    input_channels: 0,
                    output_channels: stream_config.channels as usize,
                    resampling: false,
                    input_format: "MIDI notes".into(),
                    output_format: format!("{:?}", supported.sample_format()),
                };
                output.play().context("Start keyboard audio")?;
                return Ok((
                    ActiveStreams {
                        input: None,
                        _output: output,
                        _midi: Some(connection),
                        midi_port: Some(config.midi_port.clone()),
                    },
                    info,
                ));
            }
            Err(e) => error = Some(e),
        }
    }
    Err(error.unwrap_or_else(|| anyhow::anyhow!("Unable to open output")))
}

fn stream_error(d: &Diagnostics, error: cpal::Error) {
    match error.kind() {
        cpal::ErrorKind::Xrun => {
            d.xruns.fetch_add(1, Relaxed);
        }
        cpal::ErrorKind::RealtimeDenied => d.realtime_denied.store(true, Relaxed),
        _ => d.fatal.store(true, Relaxed),
    }
}
fn capture_samples<T: Sample>(
    data: &[T],
    channels: usize,
    channel: usize,
    producer: &mut HeapProd<f32>,
    d: &Diagnostics,
) where
    f32: FromSample<T>,
{
    let mut maximum = 0.0_f32;
    let mut clips = 0;
    let mut dropped = 0;
    for frame in data.chunks_exact(channels) {
        let x = f32::from_sample(frame[channel]);
        let x = if x.is_finite() { x } else { 0.0 };
        maximum = maximum.max(x.abs());
        if x.abs() >= 0.999 {
            clips += 1;
        }
        if producer.try_push(x).is_err() {
            dropped += 1;
        }
    }
    peak(&d.input_peak, maximum);
    d.input_clips.fetch_add(clips, Relaxed);
    d.overruns.fetch_add(dropped, Relaxed);
    d.input_frames
        .fetch_add((data.len() / channels) as u64, Relaxed);
}
fn build_capture<T>(
    device: &Device,
    config: StreamConfig,
    channel: usize,
    mut producer: HeapProd<f32>,
    d: Arc<Diagnostics>,
) -> Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    let errors = d.clone();
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _| capture_samples(data, channels, channel, &mut producer, &d),
        move |e| stream_error(&errors, e),
        Some(Duration::from_secs(3)),
    )?)
}
fn build_playback<T>(
    device: &Device,
    config: StreamConfig,
    mut monitor: Monitor,
    d: Arc<Diagnostics>,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let errors = d.clone();
    Ok(device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            monitor.render(data, channels);
        },
        move |e| stream_error(&errors, e),
        Some(Duration::from_secs(3)),
    )?)
}

/// PI clock controller. More queued input requires a smaller output/input ratio.
struct DriftController {
    integral: f64,
    correction: f64,
}
impl DriftController {
    fn new() -> Self {
        Self {
            integral: 0.0,
            correction: 0.0,
        }
    }
    fn update(&mut self, occupancy: usize, target: usize, dt: f64) -> f64 {
        let error = (occupancy as f64 - target as f64) / target as f64;
        self.integral = (self.integral + error * dt * 0.00015).clamp(-0.0015, 0.0015);
        let desired = (-error * 0.001 - self.integral).clamp(-0.002, 0.002);
        self.correction += (desired - self.correction) * (dt / 0.5).min(1.0);
        1.0 + self.correction
    }
}

pub struct Monitor {
    instrument: Option<midi::Instrument>,
    audio_enabled: bool,
    consumer: HeapCons<f32>,
    resampler: Option<Async<f32>>,
    input: Vec<f32>,
    converted: Vec<f32>,
    position: usize,
    available: usize,
    primed: bool,
    target: usize,
    input_rate: u32,
    output_rate: u32,
    filter_delay: usize,
    drift: DriftController,
    processor: Processor,
    parameters: Arc<SharedParameters>,
    d: Arc<Diagnostics>,
    fade: f32,
    bypass: f32,
    bypass_target: f32,
    last_overruns: u64,
}
impl Monitor {
    fn new(
        consumer: HeapCons<f32>,
        input_rate: u32,
        output_rate: u32,
        resampling: bool,
        target: usize,
        parameters: Arc<SharedParameters>,
        d: Arc<Diagnostics>,
    ) -> Result<Self> {
        let resampler = if resampling {
            Some(Async::<f32>::new_sinc(
                output_rate as f64 / input_rate as f64,
                1.01,
                &SincInterpolationParameters::new(128, WindowFunction::BlackmanHarris2),
                CHUNK,
                1,
                FixedAsync::Output,
            )?)
        } else {
            None
        };
        let input_length = resampler.as_ref().map_or(CHUNK, |r| r.input_frames_max());
        let filter_delay = resampler.as_ref().map_or(0, |r| r.output_delay());
        let p = parameters.read();
        Ok(Self {
            instrument: None,
            audio_enabled: true,
            consumer,
            resampler,
            input: vec![0.0; input_length],
            converted: vec![0.0; CHUNK],
            position: 0,
            available: 0,
            primed: false,
            target: target.min(RING_CAPACITY / 4),
            input_rate,
            output_rate,
            filter_delay,
            drift: DriftController::new(),
            processor: Processor::new(output_rate, p),
            parameters,
            d,
            fade: 0.0,
            bypass: if p.bypass { 1.0 } else { 0.0 },
            bypass_target: if p.bypass { 1.0 } else { 0.0 },
            last_overruns: 0,
        })
    }
    fn refill(&mut self) -> bool {
        let mut need = self
            .resampler
            .as_ref()
            .map_or(CHUNK, |r| r.input_frames_next());
        let mut queued = self.consumer.occupied_len();
        let overruns = self.d.overruns.load(Relaxed);
        if queued > self.target + need * 8 || overruns != self.last_overruns {
            let drop_count = queued.saturating_sub(self.target + need);
            self.consumer.skip(drop_count);
            queued -= drop_count;
            self.fade = 0.0;
            self.d.resyncs.fetch_add(1, Relaxed);
            self.last_overruns = overruns;
        }
        if !self.primed {
            if queued < self.target + need {
                return false;
            }
            self.primed = true;
            self.fade = 0.0;
        }
        if let Some(resampler) = self.resampler.as_mut() {
            // Use occupancy after the next chunk, the persistent queue component.
            let ratio = self.drift.update(
                queued.saturating_sub(need),
                self.target,
                CHUNK as f64 / self.output_rate as f64,
            );
            if resampler.set_resample_ratio_relative(ratio, true).is_err() {
                self.d.fatal.store(true, Relaxed);
                return false;
            }
            need = resampler.input_frames_next();
            self.d
                .correction_ppm
                .store((((ratio - 1.0) * 1e6_f64) as f32).to_bits(), Relaxed);
        }
        if queued < need {
            self.d.underruns.fetch_add(1, Relaxed);
            self.primed = false;
            self.fade = 0.0;
            return false;
        }
        self.consumer.pop_slice(&mut self.input[..need]);
        if let Some(resampler) = self.resampler.as_mut() {
            let input = InterleavedSlice::new(&self.input, 1, need);
            let output = InterleavedSlice::new_mut(&mut self.converted, 1, CHUNK);
            match (input, output) {
                (Ok(input), Ok(mut output)) => {
                    match resampler.process_into_buffer(&input, &mut output, None) {
                        Ok((_, written)) => self.available = written,
                        Err(_) => {
                            self.d.fatal.store(true, Relaxed);
                            return false;
                        }
                    }
                }
                _ => {
                    self.d.fatal.store(true, Relaxed);
                    return false;
                }
            }
        } else {
            self.converted.copy_from_slice(&self.input[..CHUNK]);
            self.available = CHUNK;
        }
        self.position = 0;
        self.d.queue_frames.store((queued - need) as u32, Relaxed);
        let ms = 1000.0
            * ((queued - need) as f32 / self.input_rate as f32
                + (self.available + self.filter_delay) as f32 / self.output_rate as f32);
        self.d.app_latency_ms.store(ms.to_bits(), Relaxed);
        self.available > 0
    }
    fn render<T: Sample + FromSample<f32>>(&mut self, output: &mut [T], channels: usize) {
        let p = self.parameters.read();
        self.processor.set_parameters(p);
        self.bypass_target = if p.bypass { 1.0 } else { 0.0 };
        let mut maximum = 0.0_f32;
        let mut clips = 0;
        let frames = output.len() / channels;
        if let Some(instrument) = &mut self.instrument {
            instrument.begin_block();
        }
        let mut capture_available = self.audio_enabled;
        let mut source_peak = 0.0_f32;
        for (index, frame) in output.chunks_exact_mut(channels).enumerate() {
            if capture_available && self.position >= self.available && !self.refill() {
                capture_available = false;
            }
            if self.d.fatal.load(Relaxed) || (!capture_available && self.instrument.is_none()) {
                // Startup prefill is intentional silence, not a dropout.
                output[index * channels..].fill(T::EQUILIBRIUM);
                break;
            }
            let mut mono = 0.0;
            if capture_available {
                mono = self.converted[self.position];
                self.position += 1;
            }
            if let Some(instrument) = &mut self.instrument {
                mono += instrument.tick();
            }
            source_peak = source_peak.max(mono.abs());
            self.bypass +=
                (self.bypass_target - self.bypass) * (1.0 / (0.020 * self.output_rate as f32));
            let stereo = self.processor.tick(mono, self.bypass);
            self.fade = (self.fade + 1.0 / (0.005 * self.output_rate as f32)).min(1.0);
            frame.fill(T::EQUILIBRIUM);
            for i in 0..2 {
                let x = stereo[i] * self.fade;
                maximum = maximum.max(x.abs());
                if x.abs() > 0.98 {
                    clips += 1;
                }
                frame[i] = T::from_sample(x);
            }
        }
        peak(&self.d.output_peak, maximum);
        self.d.output_clips.fetch_add(clips, Relaxed);
        self.d.output_frames.fetch_add(frames as u64, Relaxed);
        if let Some(instrument) = &self.instrument {
            instrument.publish_activity();
        }
        if !self.audio_enabled {
            peak(&self.d.input_peak, source_peak);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ToneParameters;
    #[test]
    fn variable_callbacks_and_underrun_recovery() {
        let (mut producer, consumer) = HeapRb::<f32>::new(RING_CAPACITY).split();
        let d = Arc::new(Diagnostics::default());
        let p = Arc::new(SharedParameters::new(ToneParameters::default()));
        let mut monitor = Monitor::new(consumer, 44100, 44100, false, 128, p, d.clone()).unwrap();
        let mut output = [1.0_f32; 1024];
        monitor.render(&mut output, 2);
        assert!(output.iter().all(|x| *x == 0.0));
        assert_eq!(d.underruns.load(Relaxed), 0);
        producer.push_slice(&[0.1; 2048]);
        for size in [62, 254, 510, 1024] {
            monitor.render(&mut output[..size], 2);
        }
        assert!(output.iter().all(|x| x.is_finite() && x.abs() < 1.0));
        for _ in 0..10 {
            monitor.render(&mut output, 2);
        }
        assert!(d.underruns.load(Relaxed) > 0);
        producer.push_slice(&[0.1; 1024]);
        monitor.render(&mut output, 2);
        assert!(output.iter().any(|x| x.abs() > 0.0));
    }
    #[test]
    fn drift_and_rate_conversion_stay_bounded() {
        for (input_rate, output_rate, ppm) in [
            (44100, 48000, 500.0),
            (44100, 48000, -500.0),
            (44100, 44100, 400.0),
            (48000, 44100, -400.0),
        ] {
            let (mut producer, consumer) = HeapRb::<f32>::new(RING_CAPACITY).split();
            let d = Arc::new(Diagnostics::default());
            let p = Arc::new(SharedParameters::new(ToneParameters {
                bypass: true,
                ..Default::default()
            }));
            let mut monitor =
                Monitor::new(consumer, input_rate, output_rate, true, 512, p, d.clone()).unwrap();
            producer.push_slice(&[0.1; 768]);
            let mut clock = 0.0;
            let mut out = [0.0_f32; 256];
            let mut min = usize::MAX;
            let mut max = 0;
            // Simulate 90 seconds with +/- 500 ppm hardware-clock mismatch.
            for block in 0..(output_rate as usize * 90 / 128) {
                clock += 128.0 * input_rate as f64 / output_rate as f64 * (1.0 + ppm / 1e6);
                let n = clock.floor() as usize;
                clock -= n as f64;
                for _ in 0..n {
                    assert!(producer.try_push(0.1).is_ok());
                }
                monitor.render(&mut out, 2);
                if block > output_rate as usize * 20 / 128 {
                    let q = monitor.consumer.occupied_len();
                    min = min.min(q);
                    max = max.max(q);
                }
                assert!(out.iter().all(|x| x.is_finite()));
            }
            assert_eq!(
                d.underruns.load(Relaxed),
                0,
                "{input_rate}->{output_rate} at {ppm} ppm"
            );
            assert_eq!(d.overruns.load(Relaxed), 0);
            assert!(min > 100 && max < 1100, "queue range {min}..{max}");
            let correction = f32::from_bits(d.correction_ppm.load(Relaxed));
            eprintln!(
                "{input_rate} -> {output_rate}, clock {ppm:+} ppm: queue {min}..{max} frames, correction {correction:+.1} ppm, no dropouts"
            );
            assert!(
                (correction + ppm as f32).abs() < 150.0,
                "correction {correction}, clock {ppm}"
            );
        }
    }
    #[test]
    fn application_callbacks_do_not_allocate_after_construction() {
        for resampling in [false, true] {
            let (mut producer, consumer) = HeapRb::<f32>::new(RING_CAPACITY).split();
            let d = Arc::new(Diagnostics::default());
            let p = Arc::new(SharedParameters::new(ToneParameters::default()));
            let rate = if resampling { 48000 } else { 44100 };
            let mut monitor =
                Monitor::new(consumer, 44100, rate, resampling, 512, p.clone(), d.clone()).unwrap();
            let input = [0.12_f32; 256];
            let mut output = [0.0_f32; 256];
            capture_samples(&[0.1_f32; 1536], 2, 0, &mut producer, &d);
            let allocations = crate::allocation_probe::count(|| {
                for block in 0..1000 {
                    p.publish(ToneParameters {
                        warmth: (block % 100) as f32 / 100.0,
                        bypass: block % 2 == 0,
                        ..Default::default()
                    });
                    capture_samples(&input, 2, 0, &mut producer, &d);
                    monitor.render(&mut output, 2);
                }
                // Include starvation and recovery in the checked path.
                for _ in 0..32 {
                    monitor.render(&mut output, 2);
                }
                for _ in 0..32 {
                    capture_samples(&input, 2, 0, &mut producer, &d);
                }
                monitor.render(&mut output, 2);
            });
            assert_eq!(allocations, 0, "resampling={resampling}");
        }
    }
    #[test]
    fn midi_only_and_layered_callbacks_work_without_allocating() {
        for audio_enabled in [false, true] {
            let (mut capture, consumer) = HeapRb::<f32>::new(RING_CAPACITY).split();
            let (mut notes, midi_queue) =
                HeapRb::<midi::Message>::new(midi::QUEUE_CAPACITY).split();
            let d = Arc::new(Diagnostics::default());
            let p = Arc::new(SharedParameters::new(ToneParameters {
                delay: 0.0,
                reverb: 0.0,
                ..Default::default()
            }));
            let state = Arc::new(midi::State::default());
            let mut monitor =
                Monitor::new(consumer, 48000, 48000, false, 128, p, d.clone()).unwrap();
            monitor.audio_enabled = audio_enabled;
            monitor.instrument = Some(midi::Instrument::new(midi_queue, state.clone(), 48000));
            let mut out = [0.0_f32; 256];
            capture.push_slice(&[0.0; 512]);
            let mut energy = 0.0_f32;
            let allocations = crate::allocation_probe::count(|| {
                notes.try_push(midi::Message::On(0, 60, 127)).unwrap();
                for block in 0..1500 {
                    if audio_enabled {
                        capture.push_slice(&[0.0; 128]);
                    }
                    if block == 400 {
                        notes.try_push(midi::Message::Off(0, 60)).unwrap();
                    }
                    monitor.render(&mut out, 2);
                    for x in out {
                        assert!(x.is_finite() && x.abs() < 1.0);
                        energy += x.abs();
                    }
                }
            });
            assert_eq!(allocations, 0);
            assert!(energy > 1.0);
            assert_eq!(d.underruns.load(Relaxed), 0);
            assert_eq!(state.voices.load(Relaxed), 0);
            d.fatal.store(true, Relaxed);
            monitor.render(&mut out, 2);
            assert!(out.iter().all(|x| *x == 0.0));
        }
    }
}
