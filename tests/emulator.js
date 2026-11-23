/* The headless Bangle.js 2 emulator, with a virtual clock and snapshots.

   Loads the same files as BangleApps core/lib/emulator.js, but:

   - The watch's clock is virtual. The firmware reads the time with Date.now()
     and nothing else, so the emulator gets its own Date whose now() is
     `clock.t`. Time only moves in advance(ms), which steps from one watch
     timer to the next. A 6 s wait takes a few milliseconds, and the watch
     sees exactly the same times on every run (no real-clock jitter, no
     minute or midnight boundaries that depend on when the test ran).
   - snapshot() / restore() copy the whole watch (RAM and flash) so each test
     can start from the same booted watch without re-creating the emulator.
     Restoring RAM along with flash keeps Storage's file cache in step with
     the flash (restoring flash alone corrupts later writes).

   Snapshots are only taken between calls into the firmware, when its stack
   is empty, so RAM + flash + the clock + the pin values are its whole state.
*/
const fs = require("fs");
const path = require("path");
const vm = require("vm");

const BANGLE_DIR = process.env.BANGLE_DIR || "/opt/bangle";
const EMU_DIR = path.join(BANGLE_DIR, "EspruinoWebIDE/emu");
const SOURCES = ["emulator_banglejs2.js", "emu_banglejs2.js", "common.js"];

/* The emulator is a debug build: every call from the firmware out to
   JavaScript (EM_ASM) re-reads and checks its argument types. The firmware
   reads flash a byte at a time that way, and it runs code straight from
   flash, so that check took about half the test suite's run time. This reads
   all-integer arguments with the types cached per call site instead; any
   other call goes the original way. If a new emulator build doesn't have the
   code this replaces, it runs unpatched (slower, but the same). */
const EM_ASM_FROM = `  var runEmAsmFunction = (code, sigPtr, argbuf) => {
      var args = readEmAsmArgs(sigPtr, argbuf);`;
const EM_ASM_TO = `  var emAsmFlashRead, emAsmArgs = new Map();
  var runEmAsmFunction = (code, sigPtr, argbuf) => {
      if (code === emAsmFlashRead) return flashMemory[HEAP32[argbuf >> 2]];
      if (emAsmFlashRead === undefined)
        emAsmFlashRead = +Object.keys(ASM_CONSTS).find(k => String(ASM_CONSTS[k]).includes("hwFlashRead("));
      var n = emAsmArgs.get(sigPtr);
      if (n === undefined) {
        n = 0;
        for (var p = sigPtr; HEAPU8[p]; p++, n++) if (HEAPU8[p] != 105 && HEAPU8[p] != 112) { n = -1; break; }
        emAsmArgs.set(sigPtr, n);
      }
      var b = argbuf >> 2, f = ASM_CONSTS[code];
      switch (n) {
        case 0: return f();
        case 1: return f(HEAP32[b]);
        case 2: return f(HEAP32[b], HEAP32[b + 1]);
        case 3: return f(HEAP32[b], HEAP32[b + 1], HEAP32[b + 2]);
      }
      var args = readEmAsmArgs(sigPtr, argbuf);`;
function readSources() {
  const src = SOURCES.map(f => fs.readFileSync(path.join(EMU_DIR, f), "utf8"));
  if (src[0].includes(EM_ASM_FROM)) src[0] = src[0].replace(EM_ASM_FROM, EM_ASM_TO);
  else console.warn("tests/emulator.js: emulator build changed, running it unpatched (slower)");
  return src;
}

/* The emulator's files, compiled once as one function, so that `Date` in
   them is the virtual clock passed in and the emscripten Module stays
   reachable. (emulator.js eval()s them inside a function instead, which
   makes every variable lookup in them slow.) */
const EXPORTS = `return {
  Module,
  get flash() { return flashMemory; },
  getMaxFlash: () => maxFlashMemory,
  setMaxFlash: v => { maxFlashMemory = v; },
  pins: hwPinValue,
  setRx: cb => { jsRXCallback = cb; },
  setGfx: cb => { jsUpdateGfx = cb; },
  push: c => Module.ccall("jshPushIOCharEvent", "number", ["number", "number"], [DEVICE, c]),
  jsInit, jsIdle, jsStopIdle, jsTransmitString, jsGetGfxContents,
  GFX_WIDTH, GFX_HEIGHT
};`;
let compiled;
function evalEmulator(Date, onConsoleOutput) {
  if (!compiled) {
    const body = readSources().join("\n;\n") + "\n;\n" + EXPORTS;
    compiled = vm.runInThisContext(
      `(function (Date, onConsoleOutput, require, __dirname, __filename) {\n${body}\n})`,
      { filename: path.join(EMU_DIR, "emulator_banglejs2.js") });
  }
  return compiled(Date, onConsoleOutput, require, EMU_DIR, path.join(EMU_DIR, "emulator_banglejs2.js"));
}

/* opts: {start (ms since 1970), rxCallback(charCode), consoleOutputCallback(line)} */
async function load(opts) {
  const clock = { t: opts.start };
  class VDate extends Date {
    constructor(...a) { if (a.length) super(...a); else super(clock.t); }
    static now() { return clock.t; }
  }
  const e = evalEmulator(VDate, line => opts.consoleOutputCallback && opts.consoleOutputCallback(line));
  e.setRx(opts.rxCallback || (() => {}));
  e.setGfx(() => {});
  await new Promise(r => setTimeout(r, 0)); // as emulator.js: let the runtime start
  e.jsInit();
  e.jsIdle();
  e.jsTransmitString("Bangle.factoryReset()\n");
  e.jsStopIdle();

  const emu = {
    clock,
    GFX_WIDTH: e.GFX_WIDTH,
    GFX_HEIGHT: e.GFX_HEIGHT,
    get flashMemory() { return e.flash; },
    /* Send characters to the watch's console. The watch runs them at once;
       no time passes. emulator.js runs the watch's idle loop after every
       character, which made long commands slow; this hands them over 128 at
       a time (the watch's input queue takes about 400). */
    tx: s => {
      for (let i = 0; i < s.length; i += 128) {
        for (let j = i; j < Math.min(s.length, i + 128); j++) e.push(s.charCodeAt(j));
        emu.idle();
      }
    },
    /* Let the watch handle anything pending, without moving the clock */
    idle: () => { const n = e.jsIdle(); e.jsStopIdle(); return n; },
    stopIdle: () => e.jsStopIdle(),
    /* Move the watch's clock on by ms, running each timer when it is due.
       Steps are never longer than 100 ms, in case something on the watch
       polls rather than setting a timer. */
    advance: ms => {
      const end = clock.t + ms;
      for (;;) {
        const next = emu.idle(); // ms to the watch's next timer, at least 10
        if (clock.t >= end) return;
        clock.t = Math.min(end, clock.t + Math.min(next, 100));
      }
    },
    snapshot: () => ({
      ram: e.Module.HEAPU8.slice(),
      flash: e.flash.slice(),
      maxFlash: e.getMaxFlash(),
      pins: e.pins.slice(),
      t: clock.t
    }),
    restore: s => {
      e.jsStopIdle();
      e.Module.HEAPU8.set(s.ram);
      e.flash.set(s.flash);
      e.setMaxFlash(s.maxFlash);
      e.pins.set(s.pins);
      clock.t = s.t;
    },
    getGfxContents: rgba => e.jsGetGfxContents(rgba),
    getScreenshot: () => {
      const rgba = new Uint8Array(e.GFX_WIDTH * e.GFX_HEIGHT * 4);
      e.jsGetGfxContents(rgba);
      return new Uint32Array(rgba.buffer);
    }
  };
  emu.idle();
  return emu;
}

module.exports = { load, BANGLE_DIR };
