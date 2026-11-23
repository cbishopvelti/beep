/* Headless Bangle.js 2 emulator harness for Bleep Test.

   Needs EspruinoWebIDE and BangleApps checked out side by side in
   $BANGLE_DIR (default /opt/bangle); see tests/README.md.

   const h = await require("./harness").create();
   await h.reset();            // factory reset + install Bleep Test + boot
   h.eval("1+1")              // -> 2 (JSON round trip from the watch)
   h.tx("code")               // run code, ignore the result
   await h.wait(1500)          // let 1.5 s pass on the watch's (virtual) clock
   await h.load()              // load() the default clock (reruns boot code)

   The watch's clock is virtual (tests/emulator.js): it only moves in
   h.wait(), which runs the watch's timers as they fall due, so waits take
   no real time and every run sees the same times. Each test starts at
   START (below) plus the time the boot takes.
*/
const fs = require("fs");
const path = require("path");
const installer = require("../tools/install");
const { load, BANGLE_DIR } = require("./emulator");
const ROOT = path.join(__dirname, "..");

function check() {
  for (const d of ["EspruinoWebIDE/emu", "BangleApps/core/lib/emulator.js"])
    if (!fs.existsSync(path.join(BANGLE_DIR, d))) {
      console.error(`Missing ${path.join(BANGLE_DIR, d)}. Run the emulator setup script (tests/README.md) or set BANGLE_DIR.`);
      process.exit(2);
    }
}

const sleep = ms => new Promise(r => setTimeout(r, ms));

/* When every test starts on the watch's clock: a fixed time, well away from
   midnight and minute boundaries, so runs are repeatable. BLEEP_TEST_TIME
   (anything Date.parse takes, e.g. 2026-06-15T23:59:30Z) runs the suite at
   another time, to try it near midnight say. */
const START = process.env.BLEEP_TEST_TIME ? Date.parse(process.env.BLEEP_TEST_TIME)
  : Date.UTC(2026, 5, 15, 10, 0, 0);
if (isNaN(START)) throw new Error("BLEEP_TEST_TIME: can't read " + process.env.BLEEP_TEST_TIME);

/* Files to install besides Bleep Test itself: a test-only boot file.
   Bleep Test is installed with the same code as the Web IDE installer
   (tools/install.js), straight from metadata.json. */
function extraFiles() {
  // Test-only boot code (runs first): record who asked for GPS and HRM power
  // in GPS_USERS and HRM_USERS, e.g. GPS_USERS.bleeptest == 1. (The emulator
  // has no GPS or heart rate sensor: tests emit "GPS" and "HRM" events.)
  const spy = (fn, list) =>
    `(function(){var o=Bangle.${fn};global.${list}={};` +
    `Bangle.${fn}=function(on,id){if(on)${list}[id]=1;else delete ${list}[id];` +
    `return o.apply(Bangle,arguments)};})();\n`;
  const files = [{ name: "0test.boot.js", content: spy("setGPSPower", "GPS_USERS") + spy("setHRMPower", "HRM_USERS") }];
  return files;
}

async function create(opts) {
  opts = opts || {};
  check();
  const h = { log: [], errors: [] };
  let rx = ""; // raw serial output from the watch
  const emu = h.emu = await load({
    start: START,
    rxCallback: c => { rx += String.fromCharCode(c); },
    consoleOutputCallback: line => {
      h.log.push(line);
      if (opts.verbose) console.log("  EMU>", line);
      if (/Uncaught\b|^ERROR:\s|ASSERT FAILED/m.test(line)) h.errors.push(line);
    }
  });

  h.tx = js => emu.tx(`\x10${js}\n`);

  /* Evaluate on the watch and return the value (via JSON, between
     \x01..\x02 markers on the serial output so long values survive). */
  h.eval = js => {
    // A load() on the watch (a test finishing, say) resets the REPL. If
    // that lands while this command is being sent, the watch only sees the
    // tail of it and answers with a syntax or reference error and no markers.
    // So with no answer, ask again; the errors of a cut command don't count.
    for (let tries = 0; ; tries++) {
      const errs = h.errors.length;
      rx = "";
      emu.tx(`\x10print("\\x01"+JSON.stringify(${js})+"\\x02")\n`);
      const m = /\x01([^]*?)\x02/.exec(rx);
      if (m) return m[1] === "undefined" ? undefined : JSON.parse(m[1]);
      if (tries >= 2) throw new Error(`eval(${js}) failed: ${rx.trim()}`);
      const cut = h.errors.splice(errs).filter(e => !/Uncaught (Syntax|Reference)Error/.test(e));
      h.errors.push(...cut);
    }
  };

  h.wait = async ms => emu.advance(ms);

  /* load() an app (default: the clock) and wait until it is running */
  h.load = async file => {
    h.tx(`global.LOADING=1;` + (file ? `load(${JSON.stringify(file)})` : "load()"));
    for (let i = 0; i < 100; i++) {
      await h.wait(20);
      if (h.eval("global.LOADING===undefined && global.__FILE__!==undefined")) break;
    }
    await h.wait(100);
  };

  const lines = extraFiles().map(installer.fileCmds).concat(installer.installCode(ROOT))
    .join("").split("\n").filter(l => l);
  /* The first reset installs and boots, then snapshots the whole watch; the
     rest restore that snapshot (milliseconds instead of seconds). */
  let booted;
  h.reset = async () => {
    rx = "";
    h.log.length = 0;
    h.errors.length = 0;
    if (booted) return emu.restore(booted);
    // skip the first-boot welcome app
    h.tx(`require("Storage").writeJSON("welcome.json",{welcomed:true})`);
    for (const l of lines) h.tx(l);
    // load() rebuilds .boot0 (the .js hash changed) and runs boot code, like a reboot
    await h.load();
    if (h.errors.length) throw new Error("uncaught error while booting: " + h.errors.join(" | "));
    h.log.length = 0;
    booted = emu.snapshot();
  };

  h.close = () => emu.stopIdle();
  return h;
}

/* Tiny test runner: test(name, async h => {...}); run() */
const tests = [];
function test(name, fn) { tests.push({ name, fn }); }

function assert(cond, msg) {
  if (!cond) throw new Error(msg || "assertion failed");
}
assert.eq = (a, b, msg) => {
  const ja = JSON.stringify(a), jb = JSON.stringify(b);
  if (ja !== jb) throw new Error(`${msg || "not equal"}: got ${ja}, expected ${jb}`);
};
assert.near = (a, b, tol, msg) => {
  if (typeof a != "number" || Math.abs(a - b) > tol)
    throw new Error(`${msg || "not near"}: got ${a}, expected ${b} +/- ${tol}`);
};

async function run() {
  const verbose = process.argv.includes("-v");
  const only = process.argv.find(a => a.startsWith("--grep="));
  const h = await create({ verbose });
  let failed = 0, ran = 0;
  const t0 = Date.now();
  for (const t of tests) {
    if (only && !t.name.includes(only.slice(7))) continue;
    ran++;
    try {
      await h.reset();
      await t.fn(h);
      if (h.errors.length) throw new Error("uncaught error on watch: " + h.errors.join(" | "));
      console.log(`ok   - ${t.name}`);
    } catch (e) {
      failed++;
      console.log(`FAIL - ${t.name}\n       ${e.message}`);
      if (!verbose) console.log("       last console lines:\n" + h.log.slice(-8).map(l => "         " + l).join("\n"));
    }
  }
  h.close();
  console.log(`\n${ran - failed}/${ran} passed in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  process.exit(failed ? 1 : 0);
}

module.exports = { create, test, assert, run, sleep };
