import test from "node:test";
import assert from "node:assert/strict";
import { defaults, restore, formatTime, validPreset } from "../state.js";
test("session restores tone, devices and local asset references without starting playback", () => {
  const s = defaults();
  s.sound = 11;
  s.settings.root = 48;
  s.assets.sample = "test";
  s.input = "guitar";
  s.output = "headphones";
  s.presets.push({
    name: "My clean",
    tone: 0.4,
    warmth: 0.1,
    reverb: 0.2,
    delay: 0.1,
    compression: true,
  });
  const p = restore(JSON.stringify(s));
  assert.equal(p.assets.sample, "test");
  assert.equal(p.settings.root, 48);
  assert.equal(p.output, "headphones");
  assert.equal(p.sound, 11);
  assert.equal(p.presets[0].name, "My clean");
  assert.equal(p.playing, undefined);
  assert.equal(
    restore(JSON.stringify({ ...s, assets: { sample: 1 } })).assets.sample,
    "1",
  );
});
test("malformed or future settings fail; untrusted values are bounded", () => {
  assert.throws(() => restore("{broken"));
  assert.throws(() => restore('{"version":99}'));
  const p = restore(
    JSON.stringify({
      version: 1,
      source: "bad",
      sound: 99,
      tone: { master: 99, tone: -3 },
      settings: { start: 0.99, end: 0.1, bpm: 999, root: -4 },
    }),
  );
  assert.equal(p.source, "keys");
  assert.equal(p.sound, 3);
  assert.equal(p.tone.master, 0);
  assert.equal(p.tone.tone, 0);
  assert.equal(p.settings.bpm, 240);
  assert.ok(p.settings.end > p.settings.start);
  assert.equal(p.settings.root, 0);
});
test("factory presets cannot be overwritten and names stay unique", () => {
  assert.equal(validPreset("Crystal Clean", []), false);
  assert.equal(validPreset(" ", []), false);
  assert.equal(validPreset("a".repeat(61), []), false);
  assert.equal(validPreset("Mine", [{ name: "mine" }]), false);
  assert.equal(validPreset("Mine", [{ name: "Mine" }], "Mine"), true);
  assert.equal(formatTime(125.8), "2:05");
  assert.equal(formatTime(-4), "0:00");
});
