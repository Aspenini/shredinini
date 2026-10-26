// AudioWorkletGlobalScope does not provide TextEncoder/TextDecoder in several
// engines. wasm-bindgen needs them during module initialization and for errors.
// These UTF-8-only fallbacks run during setup, never in the render loop.
if (typeof globalThis.TextEncoder === "undefined") {
  globalThis.TextEncoder = class {
    encodeInto(text, output) {
      let read = 0,
        written = 0;
      for (const char of String(text)) {
        let code = char.codePointAt(0);
        if (code >= 0xd800 && code <= 0xdfff) code = 0xfffd;
        const n = code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
        if (written + n > output.length) break;
        if (n === 1) output[written++] = code;
        else {
          output[written++] =
            (n === 2 ? 0xc0 : n === 3 ? 0xe0 : 0xf0) | (code >> (6 * (n - 1)));
          for (let shift = 6 * (n - 2); shift >= 0; shift -= 6)
            output[written++] = 0x80 | ((code >> shift) & 63);
        }
        read += char.length;
      }
      return { read, written };
    }
    encode(text = "") {
      const result = new Uint8Array(String(text).length * 3);
      const { written } = this.encodeInto(text, result);
      return result.slice(0, written);
    }
  };
}
if (typeof globalThis.TextDecoder === "undefined") {
  globalThis.TextDecoder = class {
    constructor(label = "utf-8", options = {}) {
      if (!/^utf-?8$/i.test(label))
        throw new TypeError("Only UTF-8 is supported.");
      this.fatal = !!options.fatal;
    }
    decode(input = new Uint8Array()) {
      const data = ArrayBuffer.isView(input)
        ? new Uint8Array(input.buffer, input.byteOffset, input.byteLength)
        : new Uint8Array(input);
      let result = "";
      for (let i = 0; i < data.length;) {
        const first = data[i++];
        let code = first,
          n = 0,
          min = 0;
        if (first < 0x80) {
          result += String.fromCodePoint(first);
          continue;
        }
        if (first >= 0xc2 && first <= 0xdf) {
          n = 1;
          code = first & 31;
          min = 0x80;
        } else if (first >= 0xe0 && first <= 0xef) {
          n = 2;
          code = first & 15;
          min = 0x800;
        } else if (first >= 0xf0 && first <= 0xf4) {
          n = 3;
          code = first & 7;
          min = 0x10000;
        }
        let valid = n > 0 && i + n <= data.length;
        for (let j = 0; j < n && valid; j++) {
          const next = data[i + j];
          if ((next & 0xc0) !== 0x80) valid = false;
          else code = (code << 6) | (next & 63);
        }
        valid =
          valid &&
          code >= min &&
          code <= 0x10ffff &&
          !(code >= 0xd800 && code <= 0xdfff);
        if (!valid) {
          if (this.fatal) throw new TypeError("Invalid UTF-8");
          result += "\ufffd";
          continue;
        }
        i += n;
        result += String.fromCodePoint(code);
      }
      return result;
    }
  };
}
