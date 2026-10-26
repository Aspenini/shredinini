export const sounds = [
  {
    id: 3,
    name: "Studio Grand",
    family: "Keys",
    kind: "SAMPLED",
    description: "A sampled grand piano. Clear, expressive, and ready to play.",
  },
  {
    id: 4,
    name: "Velvet Rhodes",
    family: "Keys",
    kind: "SAMPLED",
    description: "Soft electric piano with a little warmth around the edges.",
  },
  {
    id: 5,
    name: "Steel Strings",
    family: "Guitars",
    kind: "SAMPLED",
    description: "Bright acoustic strings for picked melodies and open chords.",
  },
  {
    id: 6,
    name: "Clean Electric",
    family: "Guitars",
    kind: "SAMPLED",
    description:
      "A sampled clean electric guitar. Add your own space and texture.",
  },
  {
    id: 7,
    name: "Finger Bass",
    family: "Bass",
    kind: "SAMPLED",
    description: "A rounded fingered bass, with room for a groove.",
  },
  {
    id: 8,
    name: "Chamber Strings",
    family: "Orchestral",
    kind: "SAMPLED",
    description: "Sustained strings for melodies, layers, and slower moments.",
  },
  {
    id: 9,
    name: "Concert Flute",
    family: "Orchestral",
    kind: "SAMPLED",
    description: "A breathy sampled flute for a different kind of lead.",
  },
  {
    id: 10,
    name: "Studio Drums",
    family: "Drums",
    kind: "SAMPLED",
    description:
      "A General MIDI drum kit. Try C2 for a kick and D2 for a snare.",
  },
  {
    id: 0,
    name: "Electric Keys",
    family: "Keys",
    kind: "SYNTHESIZER",
    description: "Our original FM keys. Bright, responsive, and lightweight.",
  },
  {
    id: 1,
    name: "Soft Pad",
    family: "Synth",
    kind: "SYNTHESIZER",
    description: "A soft, slowly opening synth for chords and atmosphere.",
  },
  {
    id: 2,
    name: "Round Bass",
    family: "Bass",
    kind: "SYNTHESIZER",
    description: "A simple synthesized bass with a round foundation.",
  },
  {
    id: 11,
    name: "Your Sample",
    family: "Sampler",
    kind: "YOUR AUDIO",
    description: "Turn your own recording into a playable keyboard instrument.",
  },
];
export const factories = [
  {
    name: "Crystal Clean",
    tone: 0.65,
    warmth: 0.12,
    reverb: 0.24,
    delay: 0.15,
    compression: true,
  },
  {
    name: "Warm Clean",
    tone: 0.38,
    warmth: 0.2,
    reverb: 0.18,
    delay: 0.08,
    compression: true,
  },
  {
    name: "Ambient Clean",
    tone: 0.58,
    warmth: 0.08,
    reverb: 0.58,
    delay: 0.36,
    compression: true,
  },
];
export function defaults() {
  return {
    version: 1,
    source: "keys",
    sound: 3,
    preset: "Crystal Clean",
    tone: { ...factories[0], trim: 0, master: -12, bypass: false },
    settings: {
      root: 60,
      start: 0,
      end: 1,
      sampleLoop: false,
      trackGain: 0.65,
      trackLoop: false,
      click: false,
      bpm: 100,
    },
    input: "",
    output: "",
    channel: 0,
    midi: "",
    favorites: [],
    presets: [],
    assets: { backing: "", sample: "", font: "" },
  };
}
const bound = (v, lo, hi, fallback) =>
  Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : fallback;
export function restore(text) {
  const d = defaults();
  if (!text) return d;
  const p = JSON.parse(text);
  if (p.version !== 1)
    throw new Error("Unknown settings version. Defaults restored.");
  const tone = { ...d.tone, ...p.tone },
    settings = { ...d.settings, ...p.settings };
  for (const k of ["tone", "warmth", "reverb", "delay"])
    tone[k] = bound(tone[k], 0, 1, d.tone[k]);
  tone.trim = bound(tone.trim, -24, 24, 0);
  tone.master = bound(tone.master, -60, 0, -12);
  for (const k of ["compression", "bypass"]) tone[k] = Boolean(tone[k]);
  settings.root = Math.round(bound(settings.root, 0, 127, 60));
  settings.start = bound(settings.start, 0, 0.99, 0);
  settings.end = bound(settings.end, settings.start + 0.001, 1, 1);
  settings.trackGain = bound(settings.trackGain, 0, 1, 0.65);
  settings.bpm = bound(settings.bpm, 30, 240, 100);
  for (const k of ["sampleLoop", "trackLoop", "click"])
    settings[k] = Boolean(settings[k]);
  return {
    ...d,
    assets: Object.fromEntries(
      ["backing", "sample", "font"].map((k) => [
        k,
        typeof p.assets?.[k] === "string"
          ? p.assets[k]
          : Number.isInteger(p.assets?.[k])
            ? String(p.assets[k])
            : "",
      ]),
    ),
    source: ["keys", "guitar", "both", "files"].includes(p.source)
      ? p.source
      : "keys",
    sound: sounds.some((s) => s.id === p.sound) ? p.sound : 3,
    preset: typeof p.preset === "string" ? p.preset.slice(0, 60) : d.preset,
    tone,
    settings,
    input: typeof p.input === "string" ? p.input : "",
    output: typeof p.output === "string" ? p.output : "",
    midi: typeof p.midi === "string" ? p.midi : "",
    channel: Math.round(bound(p.channel, 0, 63, 0)),
    favorites: Array.isArray(p.favorites)
      ? p.favorites.filter((id) => sounds.some((s) => s.id === id))
      : [],
    presets: Array.isArray(p.presets)
      ? p.presets
          .slice(0, 200)
          .filter(
            (p) =>
              typeof p.name === "string" &&
              p.name.length > 0 &&
              p.name.length <= 60,
          )
          .map((p) => ({
            name: p.name,
            ...Object.fromEntries(
              ["tone", "warmth", "reverb", "delay"].map((k) => [
                k,
                bound(p[k], 0, 1, d.tone[k]),
              ]),
            ),
            compression: Boolean(p.compression),
          }))
      : [],
  };
}
export function formatTime(seconds) {
  const n = Math.max(0, Math.floor(seconds || 0));
  return `${Math.floor(n / 60)}:${String(n % 60).padStart(2, "0")}`;
}
export function validPreset(name, presets, except = "") {
  return (
    !!name.trim() &&
    name.trim().length <= 60 &&
    !/[\x00-\x1f]/.test(name) &&
    ![...factories, ...presets].some(
      (p) =>
        p.name.toLowerCase() === name.trim().toLowerCase() && p.name !== except,
    )
  );
}
