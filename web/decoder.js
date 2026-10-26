import init, { DecodedAudio } from "./pkg/shredini.js";
let ready;
self.onmessage = async ({ data }) => {
  let clip;
  try {
    ready ??= init();
    await ready;
    clip = new DecodedAudio(new Uint8Array(data.bytes), data.extension);
    const pcm = clip.pcm();
    const waveform = clip.waveform();
    self.postMessage({ id: data.id, pcm, rate: clip.rate(), waveform }, [
      pcm.buffer,
      waveform.buffer,
    ]);
  } catch (error) {
    self.postMessage({ id: data.id, error: String(error) });
  } finally {
    clip?.free();
  }
};
