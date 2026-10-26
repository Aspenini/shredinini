import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import { readFile } from "node:fs/promises";
test("worklet UTF-8 fallback initializes without browser encoding globals", async () => {
  const scope = vm.createContext({});
  vm.runInContext(
    await readFile(new URL("../worklet-utf8.js", import.meta.url), "utf8"),
    scope,
  );
  const text = "Shredini · C♯4 → 🎹";
  scope.text = text;
  const bytes = vm.runInContext("new TextEncoder().encode(text)", scope);
  assert.deepEqual([...bytes], [...new TextEncoder().encode(text)]);
  assert.equal(
    vm.runInContext(
      'new TextDecoder("utf-8",{fatal:true}).decode(new TextEncoder().encode(text))',
      scope,
    ),
    text,
  );
  assert.throws(() =>
    vm.runInContext(
      'new TextDecoder("utf-8",{fatal:true}).decode(new Uint8Array([0xc0,0xaf]))',
      scope,
    ),
  );
  assert.equal(
    vm.runInContext(
      'new TextEncoder().encodeInto("🎹",new Uint8Array(3)).read',
      scope,
    ),
    0,
  );
});
