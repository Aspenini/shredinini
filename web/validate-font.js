import init, { WebEngine } from "./pkg/shredini.js";
self.onmessage = async ({ data }) => {
  let engine;
  try {
    await init();
    engine = new WebEngine(44100, false, true);
    engine.load_font(new Uint8Array(data));
    self.postMessage({ ok: true });
  } catch (error) {
    self.postMessage({ error: String(error) });
  } finally {
    engine?.free();
  }
};
