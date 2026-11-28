/* Makes the beeplog.js fixtures the phone app's tests compare against
   (android/tests/beeplog.rs): for each log below, <name>.log is the log as
   the watch writes it, and <name>.tcx, <name>.min50.tcx (minConfidence 50)
   and <name>.csv are what beeplog.js makes of it. expected.json has each
   log's resultText and fileBase, and the error beeplog.js throws where it
   throws (then there's no file for it).

   Run: node tests/fixtures/beeplog/make.js
   Run it again after a change to beeplog.js, then the phone app's tests
   (cd android && cargo test --test beeplog): the port must keep up. */
const fs = require("fs");
const path = require("path");
const BeepLog = require("../../../beeplog.js");

const DIR = __dirname;
const lib = fs.readFileSync(path.join(DIR, "../../../lib.js"), "utf8");
const T = new Function(lib.slice(lib.indexOf("/*TABLES_START*/"), lib.indexOf("/*TABLES_END*/")) +
  "return { position: position, result: result };")();

// The same numbers every run (mulberry32).
function rng(seed) {
  return function () {
    seed |= 0; seed = seed + 0x6D2B79F5 | 0;
    var t = Math.imul(seed ^ seed >>> 15, 1 | seed);
    t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
    return ((t ^ t >>> 14) >>> 0) / 4294967296;
  };
}

function fmtNum(v, dp) { return isNaN(v) ? "" : v.toFixed(dp); }

/* A test's log as lib.js writes it: a sample a second for `dur` s on an
   m metre course, the heart rate climbing, then the end line. */
function test(o) {
  var r = rng(o.seed || 1), s = "start," + o.start + "," + (o.tz || 0) + "," + o.m + "\n";
  var hrMax = 0, hrSum = 0, hrN = 0, p, t;
  for (t = 0; t <= o.dur; t += 1) {
    if (o.clockAt && t == o.clockAt) s += "clock," + o.clock.toFixed(3) + "\n";
    p = T.position(o.m, t);
    var f = (t - p.shuttleStart) / (p.nextAt - p.shuttleStart);
    if (!(f > 0)) f = 0;
    if (f > 1) f = 1;
    var bpm = Math.round(90 + t * 0.4 + r() * 6), conf = t < 4 ? Math.round(r() * 40) : Math.round(60 + r() * 40);
    if (o.lowConf && t % 7 == 3) conf = Math.round(r() * 49);
    if (t % 11 == 5) bpm = 0;
    if (bpm > 0) { hrSum += bpm; hrN++; if (bpm > hrMax) hrMax = bpm; }
    s += t.toFixed(1) + "," + bpm + "," + conf + "," + p.level + "," + p.shuttle + "," + ((p.total + f) * o.m).toFixed(1);
    if (o.gps) {
      var fix = t % 9 != 4;
      s += "," + fmtNum(fix ? 9 + r() * 4 : NaN, 1) + "," + fmtNum(fix ? 51.5 + r() / 1000 : NaN, 6) + "," +
        fmtNum(fix ? -0.12 - r() / 1000 : NaN, 6);
    }
    s += "\n";
  }
  if (o.after) for (var a = 1; a <= o.after; a++) s += (o.dur + a).toFixed(1) + ",150,90,9,9,9999.0\n";
  if (!o.unfinished) {
    p = T.position(o.m, o.dur);
    var res = T.result(o.m, p);
    s += "end," + o.dur.toFixed(1) + "," + res.l + "," + res.s + "," + p.total * o.m + "," + hrMax + "," +
      (hrN ? Math.round(hrSum / hrN) : 0) + "\n";
  }
  return s;
}

/* Numbers of every kind in every field, written as String() writes them so
   they read back exactly: what the port's number formatting is held to. */
function numbers() {
  var r = rng(7), s = "start,1781344800000.25,-330,15\n", t = 0;
  var odd = [0.05, 0.25, 1.005, 1.45, 2.5, 0.125, 1e-7, 1.5e-7, 123456789.123456, 1e21, 2e21, 5e-324,
    0.1 + 0.2, 72.5, 73.5, -0.5, -1.25, 999999999999999900000, 4.35, 9.005, 0.045, 1 / 3, 2 / 3];
  function any() {
    var k = r();
    if (k < 0.25) return odd[Math.floor(r() * odd.length)];
    if (k < 0.5) return Math.round(r() * 2000) / 10;
    if (k < 0.75) return r() * Math.pow(10, Math.floor(r() * 12) - 4);
    return (r() - 0.3) * 400;
  }
  for (var i = 0; i < 300; i++) {
    t += Math.round(r() * 1500) / 1000;
    var level = 1 + Math.floor(i / 40);
    s += [t, any(), Math.round(r() * 100), level, i % 9, any(), r() < 0.2 ? "" : any(), r() < 0.2 ? "" : any(),
      r() < 0.2 ? "" : any()].join(",") + "\n";
  }
  return s;
}

const LOGS = {
  // A 20 m test, no GPS, in UTC+1.
  nogps: test({ start: 1781344800000, tz: -60, m: 20, dur: 200, seed: 1 }),
  // A 15 m test with GPS speed, some seconds without a fix.
  gps: test({ start: 1781258400000, tz: 0, m: 15, dur: 130, gps: true, seed: 2 }),
  // The watch's clock set an hour and a bit forward 40 s in (UTC-5).
  clock: test({ start: 1781172000000, tz: 300, m: 20, dur: 90, clockAt: 40, clock: 3600.25, seed: 3 }),
  // The watch went on logging while its menu was open: samples after the end.
  "after-end": test({ start: 1781085600000, tz: -60, m: 20, dur: 70, after: 5, seed: 4 }),
  // The watch stopped before it wrote the end (no result).
  unfinished: test({ start: 1781000000000, tz: -120, m: 15, dur: 45, unfinished: true, seed: 5 }),
  // Readings below 50% confidence, which min50 leaves out.
  lowconf: test({ start: 1780900000000, tz: 0, m: 20, dur: 80, lowConf: true, seed: 6 }),
  // Only the start: no samples, which TCX can't have.
  empty: "start,1781344800000,-60,20\n",
  // Fields as +s reads them, out of order samples, CRLF, junk lines, NaN
  // levels, a negative clock, and a half-hour time zone.
  odd: "start, 1781344800000 ,-330.5,0x0f\r\n" +
    "hello world\r\n" +
    "\r\n" +
    "2.0,0x48,1e2,1,0,1.25\r\n" +
    "1.0,72.5,50,1,0,0.25\r\n" +
    "1.0,73.5,49,1,0,0.35, 9 ,,\r\n" +
    "3,abc,90,abc,0,2.5\r\n" +
    "4,80,90,abc,0,3.45,12.6,51.5,\r\n" +
    "5,Infinity,90,2,1,-0.04,1e21,51.5,-0.1\r\n" +
    "clock,-1.5\r\n" +
    "6,  ,  ,2,1,\t\r\n" +
    "7,81,90,2,1,7.005,4.35,51.50000000000001,-0.12345678901234567,extra\r\n" +
    "7.000,82,90,3,0,8\r\n" +
    "x,1,2,3,4,5\r\n" +
    ",1,2,3,4,5\r\n" +
    "end,7,3,0,8\r\n",
  // A start a Date can't hold: beeplog.js throws.
  "invalid-start": "start,abc,0,20\n0.0,100,90,1,0,0.0\nend,0,0,0,0,100,100\n",
  // A time past a Date's range in the samples, after good ones.
  "too-late": "start,8639999999990000,0,20\n0.0,100,90,1,0,0.0\n5.0,101,90,1,0,10.0\n20.0,102,90,1,1,20.0\n",
  // No start line at all, and years past 9999.
  "no-start": "0,100,90,1,0,0\n1,100,90,1,0,1\n",
  "far-future": "start,253402300799000,-60,20\n0,100,90,1,0,0\n2,100,90,1,0,5\n",
  "before-1970": "start,-62198755200500,0,20\n0,100,90,1,0,0\n0.5,100,90,2,0,5\n",
  // An end at NaN leaves no samples.
  "nan-end": "start,1781344800000,0,20\n0,100,90,1,0,0\nend,x,1,2,3,4,5\n",
  numbers: numbers()
};

function attempt(f) {
  try { return { value: f() }; } catch (e) { return { error: e.message }; }
}

const expected = {};
for (const name of Object.keys(LOGS)) {
  const text = LOGS[name];
  fs.writeFileSync(path.join(DIR, name + ".log"), text);
  const log = BeepLog.parse(text);
  const e = { resultText: BeepLog.resultText(log) };
  const base = attempt(() => BeepLog.fileBase(log));
  e.fileBase = base.error === undefined ? base.value : { error: base.error };
  for (const [key, file, f] of [
    ["tcx", name + ".tcx", () => BeepLog.toTCX(log)],
    ["tcx50", name + ".min50.tcx", () => BeepLog.toTCX(log, { minConfidence: 50 })],
    ["csv", name + ".csv", () => BeepLog.toCSV(log)]
  ]) {
    const out = attempt(f);
    const p = path.join(DIR, file);
    if (out.error !== undefined) {
      e[key] = { error: out.error };
      if (fs.existsSync(p)) fs.unlinkSync(p);
    } else {
      e[key] = file;
      fs.writeFileSync(p, out.value);
    }
  }
  expected[name] = e;
}
fs.writeFileSync(path.join(DIR, "expected.json"), JSON.stringify(expected, null, 1) + "\n");
console.log(`wrote ${Object.keys(LOGS).length} logs and what beeplog.js makes of them in ${path.relative(process.cwd(), DIR) || "."}`);
