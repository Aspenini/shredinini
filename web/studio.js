import {
  sounds,
  factories,
  defaults,
  restore,
  formatTime,
  validPreset,
} from "./state.js";

const $ = (id) => document.getElementById(id);
const sourceHasKeys = () => ["keys", "both"].includes(session.source);
const sourceHasAudio = () => ["guitar", "both"].includes(session.source);
const message = (text) => {
  $("notice").hidden = !text;
  $("notice").querySelector("span").textContent = text;
};
let session = defaults();
try {
  session = restore(localStorage.getItem("shredini.session"));
} catch (error) {
  try {
    localStorage.setItem(
      `shredini.invalid.${Date.now()}`,
      localStorage.getItem("shredini.session") || "",
    );
  } catch {}
  message(
    `Saved settings could not be read: ${error.message} Defaults restored.`,
  );
}
let context,
  metricsTimer,
  node,
  stream,
  inputNode,
  splitter,
  access,
  midiInput,
  decoder,
  decoderId = 0,
  db;
let page = "studio",
  family = "All sounds",
  busy = false,
  running = false,
  token = 0,
  modulePromise,
  factoryPromise,
  importRole,
  dropFile,
  rename = false;
let library = [],
  assets = { backing: null, sample: null, font: null },
  trackPlaying = false,
  trackPosition = 0,
  renderBlock = 0,
  peaks = 0;
const pendingDecodes = new Map(),
  held = new Set(),
  pointers = new Map();
const heldKeys = new Set();

function save() {
  try {
    localStorage.setItem("shredini.session", JSON.stringify(session));
  } catch {
    message(
      "Browser storage is unavailable. This session works, but settings cannot be saved.",
    );
  }
}
function send(type, payload = {}) {
  node?.port.postMessage({ type, ...payload });
}
function sendTone() {
  send("tone", session.tone);
  updateTone();
  save();
}
function sendSettings() {
  send("sample", session.settings);
  send("click", session.settings);
  send("track", { ...session.settings, playing: trackPlaying });
  save();
}
function element(tag, text, className) {
  const el = document.createElement(tag);
  if (text !== undefined) el.textContent = text;
  if (className) el.className = className;
  return el;
}
function button(text, action, className) {
  const el = element("button", text, className);
  el.addEventListener("click", action);
  return el;
}
function noteName(note) {
  return (
    ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"][
      note % 12
    ] +
    (Math.floor(note / 12) - 1)
  );
}
function showPage(next) {
  page = next;
  document
    .querySelectorAll("[data-page]")
    .forEach((el) => el.classList.toggle("active", el.dataset.page === page));
  $("pageTitle").textContent = {
    studio: "Your studio",
    sounds: "Sound library",
    files: "Your audio",
    pinned: "Pinned sounds",
  }[page];
  $("library").hidden = !["sounds", "pinned"].includes(page);
  $("files").hidden = page !== "files";
  drawSounds();
}
function refreshHero() {
  const sound = sounds.find((s) => s.id === session.sound);
  $("instrumentFamily").textContent = sourceHasKeys()
    ? sound.family
    : "LIVE AUDIO";
  $("instrumentName").textContent = sourceHasKeys()
    ? sound.name
    : session.source === "files"
      ? "Listening room"
      : "Your electric guitar";
  $("instrumentDescription").textContent = sourceHasKeys()
    ? sound.description
    : session.source === "files"
      ? "A place for your backing tracks, loops, and ideas."
      : "A clean starting point for your guitar. Shape your tone, add some space, and play.";
  $("keyboard").style.opacity = sourceHasKeys() ? "1" : ".3";
  $("audition").hidden = !sourceHasKeys();
  $("connectKeyboard").hidden = !sourceHasKeys();
  $("sampler").hidden = !(sourceHasKeys() && session.sound === 11);
  $("midiLabel").textContent = sourceHasKeys()
    ? midiInput?.name || "Play here, or connect your MIDI keyboard."
    : "Headroom for your next idea.";
  $("source").value = session.source;
}
function updateTone() {
  for (const key of ["tone", "warmth", "reverb", "delay"]) {
    $(`${key}Value`).textContent = `${Math.round(session.tone[key] * 100)}%`;
    $(key).value = session.tone[key];
  }
  for (const key of ["compression", "bypass"])
    $(key).checked = session.tone[key];
  for (const key of ["master", "trim"]) {
    $(key).value = session.tone[key];
    $(`${key}Value`).textContent = `${session.tone[key]} dB`;
  }
  const preset = [...factories, ...session.presets].find(
    (p) => p.name === session.preset,
  );
  $("edited").hidden =
    !!preset &&
    ["tone", "warmth", "reverb", "delay", "compression"].every(
      (k) => preset[k] === session.tone[k],
    );
}
function updateSettings() {
  const s = session.settings;
  for (const [id, key] of [
    ["root", "root"],
    ["sampleStart", "start"],
    ["sampleEnd", "end"],
    ["trackGain", "trackGain"],
    ["bpm", "bpm"],
  ])
    $(id).value = s[key];
  for (const key of ["sampleLoop", "trackLoop", "click"])
    $(key).checked = s[key];
  $("rootLabel").textContent = noteName(s.root);
  $("channel").value = session.channel + 1;
}
function presets() {
  $("presets").replaceChildren();
  for (const preset of [...factories, ...session.presets]) {
    const option = element("option", preset.name);
    option.value = preset.name;
    $("presets").append(option);
  }
  $("presets").value = session.preset;
}
function drawSounds() {
  const grid = $("soundGrid");
  grid.replaceChildren();
  const search = $("search").value.trim().toLowerCase();
  for (const sound of sounds.filter(
    (s) =>
      (family === "All sounds" || s.family === family) &&
      s.name.toLowerCase().includes(search) &&
      (page !== "pinned" || session.favorites.includes(s.id)),
  )) {
    const card = element(
        "article",
        undefined,
        `sound-card${sound.id === session.sound ? " selected" : ""}`,
      ),
      top = element("div", undefined, "top");
    top.append(
      element("span", sound.family.toUpperCase(), "eyebrow"),
      button(
        session.favorites.includes(sound.id) ? "Pinned" : "Pin",
        () => {
          session.favorites = session.favorites.includes(sound.id)
            ? session.favorites.filter((id) => id !== sound.id)
            : [...session.favorites, sound.id];
          save();
          drawSounds();
        },
        "pin",
      ),
    );
    card.append(
      top,
      button(sound.name, () => chooseSound(sound.id), "choose"),
      element("span", sound.kind, "kind"),
    );
    grid.append(card);
  }
  if (!grid.children.length)
    grid.append(
      element(
        "p",
        "No sounds here yet. Try another family, or pin a sound from the library.",
        "muted",
      ),
    );
}
async function chooseSound(id) {
  // Sampled-bank changes require rebuilding an already-running synth-only node.
  const needsBank = [3, 4, 5, 6, 7, 8, 9, 10].includes(id);
  if (running && (!sourceHasKeys() || (needsBank && !node.hasFont)))
    await stop();
  if (!sourceHasKeys()) session.source = "keys";
  session.sound = id;
  send("sound", { sound: id });
  send("panic");
  held.clear();
  paintKeys();
  refreshHero();
  showPage("studio");
  save();
}
function drawWave(canvas, wave, progress = 0) {
  const width = Math.max(1, canvas.clientWidth),
    height = Number(canvas.getAttribute("height")),
    ratio = devicePixelRatio || 1;
  canvas.width = Math.round(width * ratio);
  canvas.height = Math.round(height * ratio);
  const ctx = canvas.getContext("2d");
  ctx.scale(ratio, ratio);
  ctx.fillStyle = "#111a23";
  ctx.fillRect(0, 0, width, height);
  const bins = wave?.length
    ? wave
    : Array.from(
        { length: 180 },
        (_, i) => 0.03 + Math.sin(i * 0.11) ** 2 * 0.09,
      );
  const step = width / bins.length;
  for (let i = 0; i < bins.length; i++) {
    const h = Math.max(2, Math.min(1, bins[i]) * height * 0.85);
    ctx.fillStyle = i / bins.length <= progress ? "#76dfc0" : "#526577";
    ctx.fillRect(i * step, height / 2 - h / 2, Math.max(1, step * 0.6), h);
  }
  if (progress > 0) {
    ctx.fillStyle = "#a3f0d8";
    ctx.fillRect(Math.min(width - 1, width * progress), 2, 1, height - 4);
  }
}
function paintTracks() {
  const track = assets.backing,
    sample = assets.sample;
  $("trackName").textContent = track
    ? track.name
    : "Bring a song, a loop, or a recording.";
  $("sampleName").textContent = sample
    ? `${sample.name} · ${formatTime(sample.pcm.length / 2 / sample.rate)} · ${sample.rate} Hz`
    : "Import a recording to make it your own.";
  $("playTrack").disabled = !track || busy;
  $("rewind").disabled = !track || busy;
  $("seek").disabled = !track;
  $("playTrack").textContent = trackPlaying ? "Pause track" : "Play track";
  const duration = track ? track.pcm.length / 2 / track.rate : 0,
    progress = duration ? Math.min(1, trackPosition / duration) : 0;
  $("trackTime").textContent =
    `${formatTime(trackPosition)} / ${formatTime(duration)}`;
  $("seek").value = progress;
  drawWave($("trackWave"), track?.waveform, progress);
  if (!$("sampler").hidden) drawWave($("sampleWave"), sample?.waveform);
}
function paintKeys() {
  document
    .querySelectorAll(".key")
    .forEach((key) =>
      key.classList.toggle("held", held.has(Number(key.dataset.note))),
    );
}
function midi(status, a, b) {
  if (!running || !sourceHasKeys()) return;
  send("midi", { status, a, b });
  const kind = status & 0xf0;
  if (kind === 0x90 && b) {
    held.add(a);
  } else if (kind === 0x80 || (kind === 0x90 && !b)) {
    held.delete(a);
  }
  paintKeys();
}
function keyboard() {
  let white = 0;
  const count = 15;
  for (let note = 60; note <= 84; note++) {
    const black = [1, 3, 6, 8, 10].includes(note % 12),
      key = element("button", undefined, `key${black ? " black" : ""}`);
    key.dataset.note = note;
    key.setAttribute("aria-label", noteName(note));
    key.style.left = `${((black ? white - 0.32 : white) / count) * 100}%`;
    key.style.width = `${((black ? 0.64 : 1) / count) * 100}%`;
    if (!black) white++;
    key.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      if (!running) {
        message(
          "Press Start playing first, then play the keyboard here or use A–K on your computer keyboard.",
        );
        return;
      }
      key.setPointerCapture(event.pointerId);
      pointers.set(event.pointerId, note);
      midi(0x90, note, 100);
    });
    const release = (event) => {
      const n = pointers.get(event.pointerId);
      if (n !== undefined) {
        midi(0x80, n, 0);
        pointers.delete(event.pointerId);
      }
    };
    key.addEventListener("pointerup", release);
    key.addEventListener("pointercancel", release);
    key.addEventListener("lostpointercapture", release);
    $("keyboard").append(key);
  }
}
function controlsLocked() {
  for (const id of [
    "source",
    "inputDevice",
    "outputDevice",
    "channel",
    "midiDevice",
  ])
    $(id).disabled = running || busy;
  $("start").disabled = busy;
  $("startHero").disabled = busy;
  $("audition").disabled = busy;
  $("start").textContent = busy
    ? "Connecting…"
    : running
      ? "Stop"
      : "Start audio";
  $("startHero").textContent = running ? "Stop audio" : "Start playing";
  $("status").textContent = busy ? "CONNECTING" : running ? "LIVE" : "STANDBY";
  $("status").classList.toggle("live", running);
  paintTracks();
}
function openDatabase() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open("shredini-studio", 1);
    request.onupgradeneeded = () =>
      request.result.createObjectStore("files", { keyPath: "id" });
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
function transaction(mode, action) {
  return new Promise((resolve, reject) => {
    const tx = db.transaction("files", mode);
    const request = action(tx.objectStore("files"));
    let value;
    request.onsuccess = () => {
      value = request.result;
    };
    tx.oncomplete = () => resolve(value);
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}
async function initializeLibrary() {
  try {
    db = await openDatabase();
    library = (await transaction("readonly", (store) => store.getAll())).filter(
      validStoredAsset,
    );
    for (const role of ["sample", "backing", "font"])
      assets[role] =
        library.find((a) => String(a.id) === session.assets[role]) || null;
  } catch (error) {
    message(
      `Local file storage is unavailable (${error.message}). Imported audio can still play during this visit.`,
    );
  }
  paintFiles();
  paintTracks();
  updateBank();
}
function validStoredAsset(a) {
  return (
    a &&
    ["string", "number"].includes(typeof a.id) &&
    typeof a.name === "string" &&
    (a.kind === "font"
      ? a.bytes instanceof ArrayBuffer &&
        a.bytes.byteLength <= 256 * 1024 * 1024
      : a.kind === "audio" &&
        a.pcm instanceof Float32Array &&
        a.pcm.length > 0 &&
        a.pcm.length % 2 === 0 &&
        a.pcm.length <= 57_600_000 &&
        Number.isInteger(a.rate) &&
        a.rate >= 8000 &&
        a.rate <= 192000)
  );
}
function paintFiles() {
  $("fileList").replaceChildren();
  for (const asset of library) {
    const row = element("article", undefined, "file-row card"),
      copy = element("div"),
      actions = element("div", undefined, "actions");
    copy.append(
      element("p", asset.name),
      element(
        "p",
        asset.kind === "font"
          ? "SOUNDFONT BANK"
          : `${formatTime(asset.pcm.length / 2 / asset.rate)} · ${asset.rate} Hz`,
        "small muted",
      ),
    );
    if (asset.kind === "font")
      actions.append(button("Use bank", () => activateAsset("font", asset)));
    else
      actions.append(
        button("Use as track", () => activateAsset("backing", asset)),
        button("Use as sample", () => activateAsset("sample", asset)),
      );
    actions.append(
      button(
        "Remove",
        async () => {
          if (
            !confirm(
              `Remove “${asset.name}” from this browser’s library? Your original file is not changed.`,
            )
          )
            return;
          await stop();
          for (const role of ["backing", "sample", "font"])
            if (assets[role]?.id === asset.id) {
              assets[role] = null;
              session.assets[role] = "";
            }
          try {
            if (db)
              await transaction("readwrite", (store) => store.delete(asset.id));
          } catch (error) {
            message(error.message);
            return;
          }
          library = library.filter((a) => a.id !== asset.id);
          save();
          paintFiles();
          paintTracks();
          updateBank();
        },
        "text-button",
      ),
    );
    row.append(copy, actions);
    $("fileList").append(row);
  }
  if (!library.length)
    $("fileList").append(
      element(
        "p",
        "Your imported files will appear here. Start with a backing track or a recording.",
        "muted",
      ),
    );
}
function updateBank() {
  $("bankName").textContent =
    assets.font?.name || "GeneralUser GS · factory bank";
  $("bankName").nextElementSibling.textContent = assets.font
    ? "Custom General MIDI SoundFont"
    : "Sampled instruments by S. Christian Collins";
  $("factoryBank").hidden = !assets.font;
}
async function activateAsset(role, asset) {
  await stop();
  assets[role] = asset;
  session.assets[role] = asset.id;
  if (role === "sample") {
    session.settings.start = 0;
    session.settings.end = 1;
    session.sound = 11;
    if (!sourceHasKeys()) session.source = "keys";
    showPage("studio");
  }
  if (role === "backing") {
    trackPosition = 0;
    trackPlaying = false;
    showPage("studio");
  }
  save();
  refreshHero();
  updateSettings();
  paintFiles();
  paintTracks();
  updateBank();
}
function decodeFile(file) {
  return new Promise(async (resolve, reject) => {
    try {
      if (!decoder) {
        decoder = new Worker(new URL("./decoder.js", import.meta.url), {
          type: "module",
        });
        decoder.onmessage = ({ data }) => {
          const promise = pendingDecodes.get(data.id);
          pendingDecodes.delete(data.id);
          if (!promise) return;
          data.error
            ? promise.reject(new Error(data.error))
            : promise.resolve(data);
        };
        decoder.onerror = () => {
          for (const p of pendingDecodes.values())
            p.reject(new Error("The audio decoder stopped. Try another file."));
          pendingDecodes.clear();
          decoder.terminate();
          decoder = null;
        };
      }
      const bytes = await file.arrayBuffer(),
        id = ++decoderId;
      pendingDecodes.set(id, { resolve, reject });
      decoder.postMessage(
        { id, bytes, extension: file.name.split(".").pop().toLowerCase() },
        [bytes],
      );
    } catch (error) {
      reject(error);
    }
  });
}
async function importFile(role, file) {
  if (!file) return;
  await stop();
  try {
    if (file.size > 256 * 1024 * 1024)
      throw new Error("Browser imports are limited to 256 MB per file.");
    message(`Loading ${file.name}…`);
    const asset =
      role === "font"
        ? {
            id: crypto.randomUUID(),
            name: file.name,
            kind: "font",
            bytes: await file.arrayBuffer(),
          }
        : {
            id: crypto.randomUUID(),
            name: file.name,
            kind: "audio",
            ...(await decodeFile(file)),
          };
    asset.id = crypto.randomUUID();
    // Validate SF2 before persisting, on a short-lived worker rather than audio.
    if (role === "font") await validateFont(asset.bytes);
    library.push(asset);
    let persisted = true;
    try {
      if (db) await transaction("readwrite", (store) => store.put(asset));
      else persisted = false;
    } catch {
      persisted = false;
    }
    await activateAsset(role, asset);
    message(
      persisted
        ? `${file.name} is ready.`
        : "File loaded for this visit. Browser storage could not save it; import it again next time.",
    );
  } catch (error) {
    message(`Could not import ${file.name}: ${error.message || error}`);
  }
}
function validateFont(bytes) {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./validate-font.js", import.meta.url), {
      type: "module",
    });
    const timer = setTimeout(() => {
      worker.terminate();
      reject(new Error("SoundFont validation timed out."));
    }, 30000);
    worker.onmessage = ({ data }) => {
      clearTimeout(timer);
      worker.terminate();
      data.error ? reject(new Error(data.error)) : resolve();
    };
    worker.onerror = () => {
      clearTimeout(timer);
      worker.terminate();
      reject(new Error("Invalid SoundFont."));
    };
    worker.postMessage(bytes);
  });
}

async function refreshDevices() {
  try {
    const devices = await navigator.mediaDevices.enumerateDevices();
    for (const [id, kind, selected] of [
      ["inputDevice", "audioinput", session.input],
      ["outputDevice", "audiooutput", session.output],
    ]) {
      const select = $(id);
      select.replaceChildren();
      const option = element(
        "option",
        kind === "audioinput" ? "Default input" : "Browser default output",
      );
      option.value = "";
      select.append(option);
      const list = devices.filter(
        (d) =>
          d.kind === kind &&
          d.deviceId !== "default" &&
          d.deviceId !== "communications",
      );
      list.forEach((d, index) => {
        const opt = element(
          "option",
          d.label ||
            `${kind === "audioinput" ? "Input" : "Output"} ${index + 1}`,
        );
        opt.value = d.deviceId;
        select.append(opt);
      });
      if (selected && !list.some((d) => d.deviceId === selected)) {
        const unavailable = element("option", "Saved device unavailable");
        unavailable.value = selected;
        select.append(unavailable);
      }
      select.value = selected;
    }
    const output = devices.find((d) => d.deviceId === session.output);
    $("outputName").textContent =
      output?.label ||
      (!session.output ? "Browser audio output" : "Saved output unavailable");
    if (
      running &&
      session.output &&
      !devices.some(
        (d) => d.kind === "audiooutput" && d.deviceId === session.output,
      )
    ) {
      await stop();
      message(
        "Your selected output disconnected. Reconnect it and press Start.",
      );
    }
  } catch (error) {
    message(`Device discovery: ${error.message}`);
  }
}
async function connectMidi() {
  if (!navigator.requestMIDIAccess) {
    message(
      "This browser does not expose Web MIDI. Use the on-screen keys, or a browser with Web MIDI support.",
    );
    return;
  }
  try {
    access ??= await navigator.requestMIDIAccess({ sysex: false });
    access.onstatechange = async (event) => {
      if (
        midiInput &&
        event.port.id === midiInput.id &&
        event.port.state === "disconnected"
      ) {
        await stop();
        message(
          "Your selected MIDI keyboard disconnected. Reconnect and select it again.",
        );
      }
      renderMidi();
    };
    renderMidi();
    bindMidi();
  } catch (error) {
    message(
      `MIDI access was not granted: ${error.message}. The on-screen keyboard still works.`,
    );
  }
}
function renderMidi() {
  const select = $("midiDevice");
  select.replaceChildren();
  const option = element("option", "On-screen keyboard");
  option.value = "none";
  select.append(option);
  for (const port of access?.inputs.values() || []) {
    const option = element("option", port.name || "MIDI input");
    option.value = port.id;
    select.append(option);
  }
  if (
    session.midi &&
    session.midi !== "none" &&
    ![...(access?.inputs.values() || [])].some((p) => p.id === session.midi)
  ) {
    const option = element("option", "Saved keyboard unavailable");
    option.value = session.midi;
    select.append(option);
  }
  select.value = session.midi || "none";
}
function bindMidi() {
  if (midiInput) midiInput.onmidimessage = null;
  midiInput = null;
  const ports = [...(access?.inputs.values() || [])];
  if (!session.midi && ports.length) {
    session.midi = (ports.find((p) => /mpk/i.test(p.name)) || ports[0]).id;
    save();
    renderMidi();
  }
  const port = ports.find(
    (p) => p.id === session.midi && p.state === "connected",
  );
  if (port) {
    midiInput = port;
    port.onmidimessage = (e) => {
      if (e.data.length >= 3) midi(e.data[0], e.data[1], e.data[2]);
    };
  }
  refreshHero();
}
async function compiledModule() {
  modulePromise ??= fetch("./pkg/shredini_bg.wasm")
    .then((r) => {
      if (!r.ok)
        throw new Error(
          "Web engine is missing. Run the web build script first.",
        );
      return r.arrayBuffer();
    })
    .then((bytes) => WebAssembly.compile(bytes))
    .catch((error) => {
      modulePromise = null;
      throw error;
    });
  return modulePromise;
}
async function factoryFont() {
  factoryPromise ??= fetch("./assets/GeneralUser-GS.sf2")
    .then((r) => {
      if (!r.ok)
        throw new Error("Factory instrument bank could not be loaded.");
      return r.arrayBuffer();
    })
    .catch((error) => {
      factoryPromise = null;
      throw error;
    });
  return factoryPromise;
}
async function start() {
  if (running || busy) return running;
  if (!isSecureContext) {
    message(
      "Open Shredini over HTTPS or localhost to use browser audio and MIDI.",
    );
    return false;
  }
  if (!globalThis.AudioWorkletNode) {
    message(
      "AudioWorklet is unavailable in this browser. Please use a current desktop browser.",
    );
    return false;
  }
  const current = ++token;
  busy = true;
  controlsLocked();
  try {
    context = new AudioContext({ latencyHint: "interactive" });
    await context.resume();
    const ctx = context;
    if (session.output) {
      if (!ctx.setSinkId)
        throw new Error(
          "This browser cannot select an output. Clear the saved output in setup to use the browser default.",
        );
      await ctx.setSinkId(session.output);
    }
    ctx.addEventListener("sinkchange", () => {
      if (running && session.output && ctx.sinkId !== session.output) {
        stop();
        message(
          "The selected output changed. Audio stopped; choose your output and restart.",
        );
      }
    });
    const module = await compiledModule();
    if (current !== token) return false;
    const sampled = [3, 4, 5, 6, 7, 8, 9, 10].includes(session.sound);
    const font =
      sourceHasKeys() && sampled
        ? assets.font?.bytes || (await factoryFont())
        : null;
    if (current !== token) return false;
    await ctx.audioWorklet.addModule("./worklet.js");
    if (current !== token) return false;
    if (sourceHasAudio()) {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          deviceId: session.input ? { exact: session.input } : undefined,
          echoCancellation: false,
          noiseSuppression: false,
          autoGainControl: false,
          channelCount: { ideal: 2 },
        },
        video: false,
      });
      if (current !== token) {
        stream.getTracks().forEach((t) => t.stop());
        return false;
      }
      for (const track of stream.getAudioTracks())
        track.onended = () => {
          stop();
          message(
            "The selected input disconnected. Reconnect it and press Start.",
          );
        };
      const channels =
        stream.getAudioTracks()[0]?.getSettings().channelCount || 1;
      if (session.channel >= channels)
        throw new Error(
          `Selected input has ${channels} channel(s). Choose a lower guitar channel.`,
        );
    }
    const processorOptions = {
      module,
      audio: sourceHasAudio(),
      keyboard: sourceHasKeys(),
      font,
      sound: session.sound,
      tone: session.tone,
      settings: session.settings,
      sample: assets.sample
        ? { pcm: assets.sample.pcm, rate: assets.sample.rate }
        : null,
      backing: assets.backing
        ? { pcm: assets.backing.pcm, rate: assets.backing.rate }
        : null,
    };
    node = new AudioWorkletNode(ctx, "shredini", {
      numberOfInputs: sourceHasAudio() ? 1 : 0,
      numberOfOutputs: 1,
      outputChannelCount: [2],
      channelCount: 1,
      channelCountMode: "explicit",
      processorOptions,
    });
    node.hasFont = !!font;
    await new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error("The audio engine did not finish loading.")),
        30000,
      );
      node.port.onmessage = ({ data }) => {
        if (data.type === "ready") {
          clearTimeout(timer);
          resolve();
        } else if (data.type === "error") {
          clearTimeout(timer);
          reject(new Error(data.message));
          if (running) {
            stop();
            message(data.message);
          }
        } else if (data.type === "meters") meters(data);
      };
      node.onprocessorerror = () => {
        clearTimeout(timer);
        reject(new Error("The audio processor stopped."));
        stop();
        message("The audio processor stopped. Press Start to reconnect.");
      };
    });
    if (current !== token) return false;
    node.connect(ctx.destination);
    if (stream) {
      inputNode = ctx.createMediaStreamSource(stream);
      splitter = ctx.createChannelSplitter(Math.max(session.channel + 1, 2));
      inputNode.connect(splitter);
      splitter.connect(node, session.channel, 0);
    }
    running = true;
    metricsTimer = setInterval(() => send("metrics"), 100);
    busy = false;
    controlsLocked();
    ctx.onstatechange = () => {
      if (running && ctx.state === "suspended") {
        stop();
        message("The browser suspended audio. Press Start to resume.");
      }
    };
    $("audioInfo").textContent = `${ctx.sampleRate / 1000} kHz · Web Audio`;
    if (sourceHasKeys() && access) bindMidi();
    await refreshDevices();
    updateDiagnostics();
    return true;
  } catch (error) {
    if (current === token) {
      await stop();
      message(`Could not start audio: ${error.message || error}`);
    }
    return false;
  } finally {
    if (current === token) {
      busy = false;
      controlsLocked();
    }
  }
}
async function stop() {
  clearInterval(metricsTimer);
  metricsTimer = undefined;
  ++token;
  running = false;
  busy = false;
  trackPlaying = false;
  held.clear();
  heldKeys.clear();
  pointers.clear();
  paintKeys();
  if (node) {
    send("stop");
    node.disconnect();
    node.port.onmessage = null;
    node.onprocessorerror = null;
    node = null;
  }
  inputNode?.disconnect();
  inputNode = null;
  splitter?.disconnect();
  splitter = null;
  if (stream) {
    for (const track of stream.getTracks()) {
      track.onended = null;
      track.stop();
    }
    stream = null;
  }
  const previous = context;
  context = null;
  if (previous) {
    previous.onstatechange = null;
    if (previous.state !== "closed") await previous.close().catch(() => {});
  }
  $("inputMeter").value = 0;
  $("outputMeter").value = 0;
  $("audioInfo").textContent = "Ready when you are";
  controlsLocked();
}
function updateDiagnostics() {
  if (!context) return;
  const base = context.baseLatency,
    output = context.outputLatency;
  $("diagnostics").textContent =
    `${context.sampleRate} Hz · stereo output · ${renderBlock || "pending"} frames per worklet block. Browser base latency: ${Number.isFinite(base) ? (base * 1000).toFixed(1) + " ms" : "unavailable"}; output latency: ${Number.isFinite(output) ? (output * 1000).toFixed(1) + " ms" : "unavailable"}. Output peaks guarded: ${peaks}. These are browser estimates, not measured round-trip latency. The browser does not expose a reliable hardware dropout counter.`;
}
function meters(data) {
  $("inputMeter").value = Math.min(1, data.input);
  $("outputMeter").value = Math.min(1, data.output);
  $("peakWarning").hidden = !data.peaks;
  $("activity").textContent =
    `${data.voices} active notes · ${data.events} MIDI events`;
  trackPlaying = data.playing;
  trackPosition = data.position;
  renderBlock = data.block;
  peaks = data.peaks;
  paintTracks();
  updateDiagnostics();
}
async function playTrack() {
  if (!assets.backing) return;
  if (!running) {
    if (!(await start())) return;
  }
  const duration = assets.backing.pcm.length / 2 / assets.backing.rate;
  if (trackPosition >= duration) {
    trackPosition = 0;
    send("seek", { fraction: 0 });
  }
  trackPlaying = !trackPlaying;
  send("track", { ...session.settings, playing: trackPlaying });
  paintTracks();
}

document
  .querySelectorAll("[data-page]")
  .forEach((el) =>
    el.addEventListener("click", () => showPage(el.dataset.page)),
  );
document.querySelectorAll("[data-import]").forEach((el) =>
  el.addEventListener("click", () => {
    importRole = el.dataset.import;
    $(importRole === "font" ? "fontFile" : "audioFile").click();
  }),
);
for (const id of ["audioFile", "fontFile"])
  $(id).addEventListener("change", (event) => {
    const file = event.target.files[0];
    event.target.value = "";
    importFile(importRole, file);
  });
$("notice")
  .querySelector("button")
  .addEventListener("click", () => message(""));
for (const id of ["start", "startHero"])
  $(id).addEventListener("click", () => (running || busy ? stop() : start()));
$("audition").addEventListener("click", async () => {
  if (running || (await start())) send("audition");
});
$("panic").addEventListener("click", () => {
  send("panic");
  held.clear();
  heldKeys.clear();
  paintKeys();
});
$("source").addEventListener("change", () => {
  session.source = $("source").value;
  save();
  refreshHero();
});
for (const key of ["tone", "warmth", "reverb", "delay", "master", "trim"])
  $(key).addEventListener("input", () => {
    session.tone[key] = Number($(key).value);
    sendTone();
  });
for (const key of ["compression", "bypass"])
  $(key).addEventListener("change", () => {
    session.tone[key] = $(key).checked;
    sendTone();
  });
for (const [id, key] of [
  ["root", "root"],
  ["sampleStart", "start"],
  ["sampleEnd", "end"],
  ["trackGain", "trackGain"],
  ["bpm", "bpm"],
])
  $(id).addEventListener("input", () => {
    const n = Number($(id).value);
    if (!Number.isFinite(n)) return;
    session.settings[key] = n;
    session.settings.root = Math.round(
      Math.min(127, Math.max(0, session.settings.root)),
    );
    session.settings.start = Math.min(
      0.99,
      Math.max(0, session.settings.start),
    );
    session.settings.end = Math.min(
      1,
      Math.max(session.settings.start + 0.001, session.settings.end),
    );
    session.settings.bpm = Math.min(240, Math.max(30, session.settings.bpm));
    updateSettings();
    sendSettings();
  });
for (const key of ["sampleLoop", "trackLoop", "click"])
  $(key).addEventListener("change", () => {
    session.settings[key] = $(key).checked;
    sendSettings();
  });
$("playTrack").addEventListener("click", playTrack);
$("rewind").addEventListener("click", () => {
  trackPlaying = false;
  trackPosition = 0;
  send("track", { ...session.settings, playing: false });
  send("seek", { fraction: 0 });
  paintTracks();
});
$("seek").addEventListener("input", () => {
  const fraction = Number($("seek").value);
  if (assets.backing)
    trackPosition =
      (fraction * assets.backing.pcm.length) / 2 / assets.backing.rate;
  send("seek", { fraction });
  paintTracks();
});
$("trackWave").addEventListener("click", (event) => {
  if (!assets.backing) return;
  const r = event.currentTarget.getBoundingClientRect(),
    fraction = Math.max(0, Math.min(1, (event.clientX - r.left) / r.width));
  trackPosition =
    (fraction * assets.backing.pcm.length) / 2 / assets.backing.rate;
  send("seek", { fraction });
  paintTracks();
});
$("search").addEventListener("input", drawSounds);
for (const name of [
  "All sounds",
  "Keys",
  "Guitars",
  "Bass",
  "Orchestral",
  "Drums",
  "Synth",
  "Sampler",
])
  $("families").append(
    button(
      name,
      () => {
        family = name;
        document
          .querySelectorAll("#families button")
          .forEach((el) =>
            el.classList.toggle("active", el.textContent === name),
          );
        drawSounds();
      },
      name === family ? "active" : "",
    ),
  );
$("setupOpen").addEventListener("click", () => {
  refreshDevices();
  renderMidi();
  $("setup").showModal();
});
document
  .querySelectorAll("[data-close]")
  .forEach((el) =>
    el.addEventListener("click", () => $(el.dataset.close).close()),
  );
$("refreshDevices").addEventListener("click", refreshDevices);
$("midiConnect").addEventListener("click", connectMidi);
$("connectKeyboard").addEventListener("click", connectMidi);
for (const [id, key] of [
  ["inputDevice", "input"],
  ["outputDevice", "output"],
  ["midiDevice", "midi"],
])
  $(id).addEventListener("change", () => {
    session[key] = $(id).value;
    if (key === "midi") bindMidi();
    save();
    refreshDevices();
  });
$("channel").addEventListener("change", () => {
  session.channel = Math.min(
    63,
    Math.max(0, Number($("channel").value) - 1 || 0),
  );
  save();
});
$("factoryBank").addEventListener("click", async () => {
  await stop();
  assets.font = null;
  session.assets.font = "";
  save();
  updateBank();
});
$("presets").addEventListener("change", () => {
  const preset = [...factories, ...session.presets].find(
    (p) => p.name === $("presets").value,
  );
  if (preset) {
    session.preset = preset.name;
    for (const key of ["tone", "warmth", "reverb", "delay", "compression"])
      session.tone[key] = preset[key];
    sendTone();
  }
});
function presetEditor(manage) {
  rename = manage;
  const custom = session.presets.some((p) => p.name === session.preset);
  $("presetName").value = manage ? session.preset : "";
  $("presetDialogTitle").textContent = manage
    ? "Manage your tone"
    : "Save your tone";
  $("presetConfirm").textContent = manage ? "Rename preset" : "Save preset";
  $("presetUpdate").hidden = !manage || !custom;
  $("presetDelete").hidden = !manage || !custom;
  $("presetName").disabled = manage && !custom;
  $("presetConfirm").disabled = manage && !custom;
  $("presetError").textContent =
    manage && !custom
      ? "Factory presets are read-only. Use Save tone to create a variation."
      : "";
  $("presetDialog").showModal();
  if (!manage || custom) $("presetName").focus();
}
$("presetSave").addEventListener("click", () => presetEditor(false));
$("presetManage").addEventListener("click", () => presetEditor(true));
$("presetConfirm").addEventListener("click", () => {
  const name = $("presetName").value.trim();
  if (!validPreset(name, session.presets, rename ? session.preset : "")) {
    $("presetError").textContent = "Use a unique name, 1–60 characters.";
    return;
  }
  if (rename) {
    session.presets.find((p) => p.name === session.preset).name = name;
  } else {
    session.presets.push({
      name,
      ...Object.fromEntries(
        ["tone", "warmth", "reverb", "delay", "compression"].map((k) => [
          k,
          session.tone[k],
        ]),
      ),
    });
  }
  session.preset = name;
  presets();
  save();
  updateTone();
  $("presetDialog").close();
});
$("presetUpdate").addEventListener("click", () => {
  const preset = session.presets.find((p) => p.name === session.preset);
  if (preset) {
    for (const k of ["tone", "warmth", "reverb", "delay", "compression"])
      preset[k] = session.tone[k];
    save();
    updateTone();
    $("presetDialog").close();
  }
});
$("presetDelete").addEventListener("click", () => {
  if (!confirm(`Delete preset “${session.preset}”?`)) return;
  session.presets = session.presets.filter((p) => p.name !== session.preset);
  session.preset = "Crystal Clean";
  presets();
  save();
  updateTone();
  $("presetDialog").close();
});
const keyMap = {
  a: 60,
  w: 61,
  s: 62,
  e: 63,
  d: 64,
  f: 65,
  t: 66,
  g: 67,
  y: 68,
  h: 69,
  u: 70,
  j: 71,
  k: 72,
};
window.addEventListener("keydown", (event) => {
  if (
    event.repeat ||
    event.ctrlKey ||
    event.metaKey ||
    event.altKey ||
    event.target.closest("input,select,textarea,dialog")
  )
    return;
  const note = keyMap[event.key.toLowerCase()];
  if (note !== undefined && running && !heldKeys.has(note)) {
    event.preventDefault();
    heldKeys.add(note);
    midi(0x90, note, 100);
  }
});
window.addEventListener("keyup", (event) => {
  const note = keyMap[event.key.toLowerCase()];
  if (heldKeys.delete(note)) midi(0x80, note, 0);
});
window.addEventListener("blur", () => {
  send("panic");
  heldKeys.clear();
  held.clear();
  paintKeys();
});
window.addEventListener("pagehide", () => {
  save();
  stop();
  decoder?.terminate();
});
window.addEventListener("resize", paintTracks);
document.addEventListener("dragover", (event) => {
  event.preventDefault();
  document.body.classList.add("dragging");
});
document.addEventListener("dragleave", (event) => {
  if (!event.relatedTarget) document.body.classList.remove("dragging");
});
document.addEventListener("drop", (event) => {
  event.preventDefault();
  document.body.classList.remove("dragging");
  const file = event.dataTransfer.files[0];
  if (!file) return;
  if (/\.sf2$/i.test(file.name)) {
    importFile("font", file);
    return;
  }
  dropFile = file;
  $("dropName").textContent = file.name;
  $("dropDialog").showModal();
});
for (const [id, role] of [
  ["dropBacking", "backing"],
  ["dropSample", "sample"],
])
  $(id).addEventListener("click", () => {
    $("dropDialog").close();
    importFile(role, dropFile);
    dropFile = null;
  });
navigator.mediaDevices?.addEventListener("devicechange", refreshDevices);
const canChooseOutput =
  typeof AudioContext !== "undefined" && "setSinkId" in AudioContext.prototype;
$("outputHelp").textContent = canChooseOutput
  ? "Choose an output, or use the operating system’s browser default. Device labels may require microphone permission."
  : "This browser uses the operating system’s default audio output. Explicit output selection is unavailable.";
if (!canChooseOutput && !session.output) $("outputDevice").disabled = true;
$("compatibility").textContent =
  `Web MIDI: ${navigator.requestMIDIAccess ? "available (permission required)" : "unavailable; use on-screen keys"}. Browser audio requires HTTPS or localhost. File decoding uses the same Rust engine as the desktop app.`;
keyboard();
presets();
updateTone();
updateSettings();
refreshHero();
showPage("studio");
paintTracks();
renderMidi();
await initializeLibrary();
await refreshDevices();
