import "./worklet-utf8.js";
import { initSync, WebEngine } from "./pkg/shredini.js";

class ShrediniProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.alive = true;
    this.block = 0;
    this.inputPeak = 0;
    this.outputPeak = 0;
    this.failure = false;
    try {
      const p = options.processorOptions;
      const { memory } = initSync({ module: p.module });
      this.memory = memory;
      this.engine = new WebEngine(sampleRate, p.audio, p.keyboard);
      if (p.font) this.engine.load_font(new Uint8Array(p.font));
      if (p.sample) this.engine.load_sample(p.sample.pcm, p.sample.rate);
      if (p.backing) this.engine.load_backing(p.backing.pcm, p.backing.rate);
      this.engine.select_sound(p.sound);
      this.command({ type: "tone", ...p.tone });
      this.command({ type: "sample", ...p.settings });
      this.command({ type: "track", ...p.settings, playing: false });
      this.command({ type: "click", ...p.settings });
      this.refreshViews();
      this.port.onmessage = (e) => this.command(e.data);
      this.port.postMessage({ type: "ready" });
    } catch (error) {
      this.alive = false;
      this.port.postMessage({ type: "error", message: String(error) });
    }
  }
  refreshViews() {
    this.input = new Float32Array(
      this.memory.buffer,
      this.engine.input_ptr(),
      2048,
    );
    this.output = new Float32Array(
      this.memory.buffer,
      this.engine.output_ptr(),
      4096,
    );
  }
  command(p) {
    const e = this.engine;
    switch (p.type) {
      case "tone":
        e.tone(
          p.tone,
          p.warmth,
          p.reverb,
          p.delay,
          p.compression,
          p.trim,
          p.master,
          p.bypass,
        );
        break;
      case "sound":
        e.select_sound(p.sound);
        break;
      case "sample":
        e.sample_controls(p.root, p.start, p.end, p.sampleLoop);
        break;
      case "track":
        e.track(p.playing, p.trackGain, p.trackLoop);
        break;
      case "seek":
        e.seek(p.fraction);
        break;
      case "click":
        e.click(p.click, p.bpm);
        break;
      case "midi":
        e.midi(p.status, p.a, p.b);
        break;
      case "audition":
        e.audition();
        break;
      case "panic":
        e.panic();
        break;
      case "metrics":
        // Status messages allocate outside process(), on explicit UI requests.
        if (this.failure) {
          this.port.postMessage({
            type: "error",
            message:
              "Browser audio buffers changed unexpectedly. Restart audio.",
          });
          break;
        }
        this.port.postMessage({
          type: "meters",
          input: this.inputPeak,
          output: this.outputPeak,
          position: e.position(),
          playing: e.playing(),
          voices: e.voices(),
          events: e.midi_events(),
          peaks: e.protected_peaks(),
          block: this.block,
        });
        this.inputPeak = 0;
        this.outputPeak = 0;
        break;
      case "stop":
        e.panic();
        this.alive = false;
        break;
    }
  }
  process(inputs, outputs) {
    const channels = outputs[0];
    if (!this.alive || !this.engine || !channels?.[0]) return false;
    const frames = channels[0].length;
    if (
      frames > 2048 ||
      this.input.buffer !== this.memory.buffer ||
      this.failure
    ) {
      this.failure = true;
      for (const channel of channels) channel.fill(0);
      return true;
    }
    const input = inputs[0]?.[0];
    for (let n = 0; n < frames; n++) this.input[n] = input ? input[n] : 0;
    if (!this.engine.render(frames)) return false;
    for (let c = 0; c < channels.length; c++) {
      const output = channels[c];
      for (let n = 0; n < frames; n++)
        output[n] = this.output[n * 2 + Math.min(c, 1)];
    }
    this.block = frames;
    this.inputPeak = Math.max(this.inputPeak, this.engine.input_peak());
    this.outputPeak = Math.max(this.outputPeak, this.engine.output_peak());
    return true;
  }
}
registerProcessor("shredini", ShrediniProcessor);
