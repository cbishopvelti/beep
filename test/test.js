// Node tests: run app.js against a fake Bangle.js with a virtual clock,
// then convert the resulting log to TCX and check it.
const fs = require("fs"), path = require("path"), vm = require("vm"), assert = require("assert");
const dir = path.join(__dirname, "../apps/bleeptest");
const src = fs.readFileSync(path.join(dir, "app.js"), "utf8");
const BleepLog = require(path.join(dir, "bleeplog.js"));

function makeEnv() {
  let now = 0; // ms virtual
  let timers = [], tid = 1;
  const files = {}, listeners = {};
  const startEpoch = Date.UTC(2026, 10, 23, 18, 0, 0);
  const sched = (fn, ms, rep) => { const t = { id: tid++, at: now + Math.max(0, ms), fn, rep: rep ? ms : 0 }; timers.push(t); return t.id; };
  const clear = id => { timers = timers.filter(t => t.id !== id); };
  const handler = new Proxy({}, { get: (o, k) => k === "theme" ? { fg: 1, bg: 0, dark: true } : () => g });
  const g = handler;
  let ui = null;
  const Bangle = {
    appRect: { x: 0, y: 24, w: 176, h: 152, x2: 175, y2: 175 },
    on: (e, f) => { (listeners[e] = listeners[e] || []).push(f); },
    removeListener: (e, f) => { listeners[e] = (listeners[e] || []).filter(x => x !== f); },
    setHRMPower: () => {}, buzz: () => Promise.resolve(), beep: () => Promise.resolve(),
    getOptions: () => ({ lockTimeout: 10000, backlightTimeout: 10000 }), setOptions: () => {},
    setLocked: () => {}, setLCDPower: () => {}, loadWidgets: () => {}, drawWidgets: () => {},
    setUI: o => { ui = o; },
  };
  const Storage = {
    open: (n, m) => { if (m === "w") files[n] = ""; return { write: s => { files[n] += s; }, erase: () => { delete files[n]; } }; },
    readJSON: n => files[n] ? JSON.parse(files[n]) : undefined,
    writeJSON: (n, v) => { files[n] = JSON.stringify(v); },
  };
  let menu = null;
  const ctx = {
    Bangle, g, process: { env: { HWVERSION: 2 } },
    E: { showMenu: m => { menu = m; }, on: (e, f) => { listeners["E." + e] = [f]; }, showPrompt: () => Promise.resolve(true) },
    require: n => { assert.strictEqual(n, "Storage"); return Storage; },
    getTime: () => now / 1000,
    Date: class extends Date { constructor(...a) { a.length ? super(...a) : super(startEpoch + now); } static now() { return startEpoch + now; } },
    setTimeout: (f, ms) => sched(f, ms, false), setInterval: (f, ms) => sched(f, ms, true),
    clearTimeout: clear, clearInterval: clear, Promise, Math, load: () => {}, console,
  };
  vm.createContext(ctx);
  vm.runInContext(src, ctx);
  function advance(ms) {
    const end = now + ms;
    for (;;) {
      timers.sort((a, b) => a.at - b.at || a.id - b.id);
      const t = timers[0];
      if (!t || t.at > end) break;
      now = t.at;
      if (t.rep) t.at += t.rep; else timers.shift();
      t.fn();
    }
    now = end;
  }
  const hrm = bpm => (listeners.HRM || []).forEach(f => f({ bpm, confidence: 80 }));
  return { ctx, files, advance, hrm, get menu() { return menu; }, get ui() { return ui; }, get state() { return vm.runInContext("state", ctx); }, get run() { return vm.runInContext("run", ctx); }, startEpoch };
}

// --- level table sanity ---
{
  const env = makeEnv();
  const ev = s => vm.runInContext(s, env.ctx);
  assert.strictEqual(ev("levelSpeed(1)"), 8);
  assert.strictEqual(ev("levelSpeed(2)"), 9);
  assert.strictEqual(ev("levelSpeed(21)"), 18.5);
  assert.strictEqual(ev("shuttleTime(1)"), 9);
  for (let l = 1; l <= 21; l++) {
    const dur = ev(`SHUTTLES[${l - 1}]*shuttleTime(${l})`);
    assert(dur > 58 && dur <= 66.5, `level ${l} lasts ${dur}s`);
  }
  assert.deepStrictEqual(JSON.parse(ev("JSON.stringify(resultFor(5,0))")), { l: 4, s: 9 });
  assert.strictEqual(ev("vo2max(8,5)"), 41.5); // Flouris 2005 formula
  console.log("ok level table");
}

// --- full simulated run, stop at level 8 shuttle 5 ---
{
  const env = makeEnv();
  env.menu["Start test"]();
  assert.strictEqual(env.state, "countdown");
  env.advance(5000);
  assert.strictEqual(env.state, "run");
  // compute time to finish level 8 shuttle 5
  const ev = s => vm.runInContext(s, env.ctx);
  let t = 0;
  for (let l = 1; l < 8; l++) t += ev(`SHUTTLES[${l - 1}]*shuttleTime(${l})`);
  t += 5 * ev("shuttleTime(8)");
  let bpm = 90;
  const step = 1000;
  let elapsed = 0;
  while (elapsed + step <= t * 1000 + 2000) { env.hrm(bpm = Math.min(195, bpm + 0.2)); env.advance(step); elapsed += step; }
  const r = env.run;
  assert.strictEqual(r.level, 8); assert.strictEqual(r.shuttle, 5);
  env.ui.btn();
  assert.strictEqual(env.state, "result");
  const hist = JSON.parse(env.files["bleep.json"]);
  assert.strictEqual(hist[0].l, 8); assert.strictEqual(hist[0].s, 5);
  assert.strictEqual(hist[0].d, (7 + 8 + 8 + 9 + 9 + 10 + 10 + 5) * 20);
  const csvName = hist[0].f;
  assert(/^bleep\.\d+\.csv$/.test(csvName) && csvName.length <= 27, csvName);
  const log = BleepLog.parse(env.files[csvName]);
  assert(log.end && log.end.level === 8);
  assert(Math.abs(log.samples.length - (t + 2 + 7)) < 3, "about one sample/second");
  const tcx = BleepLog.toTCX(log);
  fs.mkdirSync(path.join(__dirname, "out"), { recursive: true });
  fs.writeFileSync(path.join(__dirname, "out", "sample.tcx"), tcx);
  fs.writeFileSync(path.join(__dirname, "out", "sample.csv"), BleepLog.toCSV(log));
  assert.strictEqual((tcx.match(/<Lap /g) || []).length, 8);
  assert(tcx.includes("<Id>2026-11-23T18:00:05Z</Id>"));
  assert((tcx.match(/<HeartRateBpm>/g) || []).length > 400);
  // distances never decrease
  const d = [...tcx.matchAll(/<Trackpoint>\s*<Time>[^<]*<\/Time>\s*<DistanceMeters>([\d.]+)/g)].map(m => +m[1]);
  for (let i = 1; i < d.length; i++) assert(d[i] >= d[i - 1]);
  console.log("ok simulated run: result", hist[0].l + "." + hist[0].s, hist[0].d + "m, vo2", hist[0].vo2, "laps 8");
}

// --- cue timing: no drift over a whole test ---
{
  const env = makeEnv();
  env.menu["Start test"](); env.advance(5000);
  const ev = s => vm.runInContext(s, env.ctx);
  let total = 0;
  for (let l = 1; l <= 21; l++) total += ev(`SHUTTLES[${l - 1}]*shuttleTime(${l})`);
  env.advance(total * 1000 + 100);
  assert.strictEqual(env.state, "result", "completing level 21 finishes the test");
  const hist = JSON.parse(env.files["bleep.json"]);
  assert.strictEqual(hist[0].l, 21); assert.strictEqual(hist[0].s, 16);
  console.log("ok full test completes in", (total / 60).toFixed(1), "min");
}
console.log("all tests passed");
