// Node tests: run the Bleep Test app, engine, boot file and widget against a fake
// Bangle.js with a virtual clock, including leaving the app mid-test.
const fs = require("fs"), path = require("path"), vm = require("vm"), assert = require("assert");
const dir = path.join(__dirname, "../apps/bleeptest");
const read = f => fs.readFileSync(path.join(dir, f), "utf8");
const SRC = { app: read("app.js"), lib: read("lib.js"), boot: read("boot.js"), wid: read("widget.js") };
const BleepLog = require(path.join(dir, "bleeplog.js"));
const meta = JSON.parse(read("metadata.json"));

const START = Date.UTC(2026, 10, 23, 18, 0, 0);

// One watch: storage and clock survive "load()", JS state doesn't.
function makeWatch() {
  const w = { now: 0, files: {}, timers: [], tid: 1, buzzes: [], ctx: null };
  w.advance = ms => {
    const end = w.now + ms;
    for (;;) {
      w.timers.sort((a, b) => a.at - b.at || a.id - b.id);
      const t = w.timers[0];
      if (!t || t.at > end) break;
      w.now = t.at;
      if (t.rep) t.at += t.rep; else w.timers.shift();
      t.fn();
    }
    w.now = end;
  };
  w.boot = appSrc => {
    if (w.ctx) w.ctx.__kill();
    w.timers = [];
    w.ctx = makeCtx(w);
    vm.runInContext(SRC.boot, w.ctx);
    if (appSrc) vm.runInContext(appSrc, w.ctx);
    else { vm.runInContext("Bangle.loadWidgets()", w.ctx); } // a clock face
    return w.ctx;
  };
  w.press = ms => { w.ctx.__press(ms); };
  w.menu = () => w.ctx.__menu();
  w.ev = s => vm.runInContext(s, w.ctx);
  return w;
}

function makeCtx(w) {
  const listeners = {}, kills = [], watches = [];
  let menu = null, ui = null;
  const g = new Proxy({}, { get: (o, k) => k === "theme" ? { fg: 1, bg: 0, dark: true } : () => g });
  const sched = (fn, ms, rep) => { const t = { id: w.tid++, at: w.now + Math.max(0, ms || 0), fn, rep: rep ? ms : 0 }; w.timers.push(t); return t.id; };
  const clear = id => { w.timers = w.timers.filter(t => t.id !== id); };
  const Storage = {
    open: (n, m) => { if (m === "w" || w.files[n] === undefined) w.files[n] = ""; return { write: s => { w.files[n] += s; }, erase: () => { delete w.files[n]; } }; },
    read: n => w.files[n], erase: n => { delete w.files[n]; },
    readJSON: n => w.files[n] ? JSON.parse(w.files[n]) : undefined,
    writeJSON: (n, v) => { w.files[n] = JSON.stringify(v); },
  };
  const modules = {};
  const ctx = {
    g, process: { env: { HWVERSION: 2 } }, BTN1: "BTN1", BTN2: "BTN2", console, Math, Promise, JSON,
    getTime: () => (START + w.now) / 1000,
    Date: class extends Date { constructor(...a) { a.length ? super(...a) : super(START + w.now); } static now() { return START + w.now; } },
    setTimeout: (f, ms) => sched(f, ms, false), setInterval: (f, ms) => sched(f, ms, true),
    clearTimeout: clear, clearInterval: clear,
    setWatch: (fn, pin, o) => { const x = { fn, o }; watches.push(x); return x; },
    clearWatch: x => { const i = watches.indexOf(x); if (i >= 0) watches.splice(i, 1); },
    load: () => { w.boot(null); },
    E: {
      showMenu: m => { menu = m; if (m && ui && ui.remove) { const r = ui.remove; ui = null; r(); } },
      on: (e, f) => { if (e === "kill") kills.push(f); },
      showPrompt: () => Promise.resolve(true),
    },
    Bangle: {
      appRect: { x: 0, y: 24, w: 176, h: 152, x2: 175, y2: 175 },
      on: (e, f) => { (listeners[e] = listeners[e] || []).push(f); },
      removeListener: (e, f) => { listeners[e] = (listeners[e] || []).filter(x => x !== f); },
      emit: (e, d) => (listeners[e] || []).slice().forEach(f => f(d)),
      setHRMPower: () => {}, buzz: (l) => { w.buzzes.push(w.now); return Promise.resolve(); }, beep: () => Promise.resolve(),
      getOptions: () => ({ lockTimeout: 10000, backlightTimeout: 10000 }), setOptions: () => {},
      setLocked: () => {}, setLCDPower: () => {}, drawWidgets: () => {},
      loadWidgets: () => { ctx.WIDGETS = {}; vm.runInContext(SRC.wid, ctx); },
      setUI: o => { if (ui && ui.remove) ui.remove(); ui = o || null; },
    },
    require: n => {
      if (n === "Storage") return Storage;
      assert.strictEqual(n, "bleeptest");
      if (!modules[n]) {
        const exports = {};
        vm.runInContext("(function(exports){" + SRC.lib + "\n})", ctx)(exports);
        modules[n] = exports;
      }
      return modules[n];
    },
    __kill: () => kills.forEach(f => f()),
    __menu: () => menu,
    __press: ms => {
      w.ctx.Bangle.emit("__noop");
      const t1 = (START + w.now) / 1000;
      w.advance(ms);
      watches.slice().forEach(x => { if (x.o.edge === "falling") x.fn({ time: (START + w.now) / 1000, lastTime: t1 }); });
    },
    __hrm: bpm => ctx.Bangle.emit("HRM", { bpm, confidence: 80 }),
  };
  ctx.global = ctx;
  vm.createContext(ctx);
  return ctx;
}

function levelTime(lib, m, upto) { let t = 0; for (let l = 1; l <= upto; l++) t += lib.TESTS[m][l - 1] * lib.shuttleTime(m, l); return t; }
function runFor(w, ms, bpm) { for (let e = 0; e < ms; e += 1000) { w.ctx.__hrm(bpm); w.advance(Math.min(1000, ms - e)); } }

// --- metadata ---
{
  for (const s of meta.storage) assert(fs.existsSync(path.join(dir, s.url)), s.url);
  assert(meta.storage.some(s => s.name === "bleeptest" && s.url === "lib.js"));
  console.log("ok metadata");
}

// --- tables ---
{
  const w = makeWatch(); w.boot(SRC.app);
  const lib = w.ctx.require("bleeptest");
  assert.strictEqual(lib.speed(1), 8); assert.strictEqual(lib.speed(2), 9); assert.strictEqual(lib.speed(21), 18.5);
  for (const m of [20, 15, 10]) {
    assert.strictEqual(lib.TESTS[m].length, 21);
    for (let l = 1; l <= 21; l++) {
      const d = lib.TESTS[m][l - 1] * lib.shuttleTime(m, l);
      assert(d > 58 && d <= 66.5, `${m}m level ${l} lasts ${d}s`);
    }
  }
  assert.strictEqual(lib.shuttleTime(15, 1), 6.75); // FitnessGram 15m PACER
  assert.strictEqual(lib.shuttleTime(10, 1), 4.5);
  assert.strictEqual(JSON.stringify(lib.result(20, lib.position(20, 63.5))), JSON.stringify({ l: 1, s: 7 }));
  assert.strictEqual(lib.vo2max(20, 8, 5), 41.5);
  assert.strictEqual(lib.vo2max(15, 8, 5), 0);
  console.log("ok level tables (20m, 15m, 10m)");
}

// --- 20m test: start from ready screen, short press opens menu, stop at 8.5 ---
{
  const w = makeWatch(); w.boot(SRC.app);
  assert.strictEqual(w.ev("view"), "ready");
  w.press(200);
  assert.strictEqual(w.ev("view"), "menu");
  assert(w.menu()["Start test"] && w.menu()["Course"]);
  w.menu()["Start test"]();
  w.advance(5000);
  assert.strictEqual(w.ev("view"), "run");
  const lib = w.ctx.require("bleeptest");
  const t = levelTime(lib, 20, 7) + 5 * lib.shuttleTime(20, 8);
  runFor(w, t * 1000 + 1500, 150);
  w.press(200);                       // drop out: opens menu, time recorded
  w.advance(10000);                   // fumbling with the menu (more than a shuttle)
  w.menu()["Stop & save"]();
  assert.strictEqual(w.ev("view"), "result");
  const hist = JSON.parse(w.files["bleep.json"]);
  assert.deepStrictEqual([hist[0].m, hist[0].l, hist[0].s], [20, 8, 5]);
  assert.strictEqual(hist[0].d, (7 + 8 + 8 + 9 + 9 + 10 + 10 + 5) * 20);
  assert.strictEqual(w.files["bleeptest.state.json"], undefined);
  const log = BleepLog.parse(w.files[hist[0].f]);
  const tcx = BleepLog.toTCX(log);
  fs.mkdirSync(path.join(__dirname, "out"), { recursive: true });
  fs.writeFileSync(path.join(__dirname, "out", "sample-20m.tcx"), tcx);
  fs.writeFileSync(path.join(__dirname, "out", "sample-20m.csv"), BleepLog.toCSV(log));
  assert.strictEqual((tcx.match(/<Lap /g) || []).length, 8);
  const laps = [...tcx.matchAll(/<TotalTimeSeconds>([\d.]+)<\/TotalTimeSeconds>\s*<DistanceMeters>([\d.]+)/g)].map(m => [+m[1], +m[2]]);
  for (let l = 1; l <= 7; l++) {
    assert.strictEqual(laps[l - 1][1], lib.TESTS[20][l - 1] * 20, `lap ${l} distance`);
    assert(Math.abs(laps[l - 1][0] - lib.TESTS[20][l - 1] * lib.shuttleTime(20, l)) < 0.15, `lap ${l} time`);
  }
  assert(tcx.includes("<Id>2026-11-23T18:00:05Z</Id>"));
  assert(tcx.includes("20m bleep test result 8.5"));
  console.log("ok 20m run stopped at 8.5 via short press menu,", hist[0].d + "m");
}

// --- 15m test, long press to the clock mid-test, widget, back into the app ---
{
  const w = makeWatch(); w.boot(SRC.app);
  w.press(200);
  const course = w.menu()["Course"];
  course.onchange(1); // 15m
  assert.strictEqual(JSON.parse(w.files["bleeptest.settings.json"]).m, 15);
  w.menu()["Start test"]();
  w.advance(5000);
  const lib = w.ctx.require("bleeptest");
  runFor(w, 90000, 140);              // into level 2
  // long press: firmware loads the clock
  w.boot(null);
  assert(w.ev("WIDGETS.bleeptest.width") > 0, "widget visible on clock");
  assert(w.ev("bleepTest.active()"));
  let cues = 0;
  w.ctx.Bangle.on("bleeptest", e => { if (e.type == "shuttle" || e.type == "level") cues++; });
  runFor(w, 120000, 160);             // two more minutes at the clock
  assert(cues >= 20, "cues continue at the clock face: " + cues);
  const st = w.ev("bleepTest.status()");
  assert.strictEqual(st.m, 15); assert.strictEqual(st.level, 4);
  // reopen the app: goes straight to the running screen
  w.boot(SRC.app);
  assert.strictEqual(w.ev("view"), "run");
  runFor(w, 30000, 170);
  w.press(200);
  w.menu()["Stop & save"]();
  const hist = JSON.parse(w.files["bleep.json"]);
  assert.strictEqual(hist[0].m, 15);
  const tt = 240;
  const p = lib.position(15, tt);
  assert.deepStrictEqual([hist[0].l, hist[0].s], [lib.result(15, p).l, lib.result(15, p).s]);
  assert.strictEqual(hist[0].vo2, 0);
  assert(hist[0].avg > 140 && hist[0].max === 170, "HR stats survive app changes: " + hist[0].avg + "/" + hist[0].max);
  // widget hides once stopped
  w.boot(null);
  assert.strictEqual(w.ev("WIDGETS.bleeptest.width"), 0);
  const log = BleepLog.parse(w.files[hist[0].f]);
  assert.strictEqual(log.course, 15);
  const ts = log.samples.map(s => s.t);
  for (let i = 1; i < ts.length; i++) assert(ts[i] >= ts[i - 1] - 0.01, "samples in order");
  assert(Math.abs(ts.length - 240) < 12, "one sample a second across app changes: " + ts.length);
  const tcx = BleepLog.toTCX(log);
  fs.writeFileSync(path.join(__dirname, "out", "sample-15m.tcx"), tcx);
  assert(tcx.includes("15m bleep test result"));
  console.log("ok 15m run with clock widget mid-test, result", hist[0].l + "." + hist[0].s);
}

// --- 10m test runs to the end without drift ---
{
  const w = makeWatch(); w.boot(SRC.app);
  w.press(200); w.menu()["Course"].onchange(2); w.menu()["Start test"](); w.advance(5000);
  const lib = w.ctx.require("bleeptest");
  const total = levelTime(lib, 10, 21);
  assert(Math.abs(total - levelTime(lib, 20, 21)) < 1e-6, "10m levels take as long as 20m levels");
  w.advance(total * 1000 + 100);
  assert.strictEqual(w.ev("view"), "result");
  const hist = JSON.parse(w.files["bleep.json"]);
  assert.deepStrictEqual([hist[0].m, hist[0].l, hist[0].s, hist[0].d], [10, 21, 32, lib.TESTS[10].reduce((a, b) => a + b) * 10]);
  console.log("ok 10m full test completes in", (total / 60).toFixed(1), "min");
}

// --- countdown can be cancelled, test resumes if the clock was set while away ---
{
  const w = makeWatch(); w.boot(SRC.app);
  w.ev("startCountdown()"); w.advance(2000); w.press(200);
  assert.strictEqual(w.ev("view"), "ready");
  assert(!w.ev("bleepTest.active()"));
  console.log("ok countdown cancel");
}
// --- cancel during a test: no result, log deleted, widget hidden ---
(async () => {
  const w = makeWatch(); w.boot(SRC.app);
  w.press(200); w.menu()["Start test"](); w.advance(5000);
  runFor(w, 30000, 120);
  const file = w.ev("bleepTest.status()") && Object.keys(w.files).find(f => /^bleep\.\d+\.csv$/.test(f));
  w.press(200);
  assert(w.menu()["Back to test"] && w.menu()["Stop & save"] && !w.menu()["Course"], "running menu differs");
  w.menu()["Cancel test"]();
  await new Promise(r => setImmediate(r));
  assert.strictEqual(w.ev("view"), "ready");
  assert(!w.ev("bleepTest.active()"));
  assert.strictEqual(w.files[file], undefined, "log deleted");
  assert.strictEqual(w.files["bleep.json"], undefined, "no result saved");
  assert.strictEqual(w.files["bleeptest.state.json"], undefined);
  assert.strictEqual(w.ev("WIDGETS.bleeptest.width"), 0);
  console.log("ok cancel during test");
  console.log("all tests passed");
})();
