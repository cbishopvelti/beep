// Bleep Test engine, stored on the watch as module "bleeptest".
// The schedule is a pure function of time since the start, so a test keeps
// running across app changes: the boot file resumes it from bleeptest.state.json
// whenever any app (e.g. the clock) loads.

var S = require("Storage");
var STATE = "bleeptest.state.json";
var HISTORY = "bleep.json";

/*TABLES_START*/
// Speeds are the same for every course length: 8.0, 9.0, then +0.5 km/h per level.
// Each level lasts about a minute, so shorter courses need more shuttles per level.
var TESTS = {
  // Standard 20m multistage fitness test (Leger / UK bleep test)
  20: [7,8,8,9,9,10,10,11,11,11,12,12,13,13,13,14,14,15,15,16,16],
  // 15m version, FitnessGram 15m PACER lap counts
  15: [9,10,11,12,12,13,13,14,14,15,15,16,17,17,18,18,19,19,20,20,21],
  // 10m version: 20m counts doubled, so every level takes exactly as long as in the 20m test
  10: [14,16,16,18,18,20,20,22,22,22,24,24,26,26,26,28,28,30,30,32,32]
};
function speed(l) { return l == 1 ? 8 : 8 + 0.5 * l; } // km/h
function shuttleTime(m, l) { return m * 3.6 / speed(l); } // seconds
function shuttles(m) { return TESTS[m] || TESTS[20]; }

// Where the test is t seconds after the start.
// level/shuttle: current level and shuttles completed in it; total: shuttles completed;
// shuttleStart/nextAt: start and end (s) of the shuttle being run now.
function position(m, t) {
  var tab = shuttles(m), at = 0, total = 0;
  for (var l = 1; l <= tab.length; l++) {
    var d = shuttleTime(m, l);
    for (var s = 0; s < tab[l - 1]; s++) {
      if (at + d > t) return { level: l, shuttle: s, total: total, shuttleStart: at, nextAt: at + d, done: false };
      at += d;
      total++;
    }
  }
  return { level: tab.length, shuttle: tab[tab.length - 1], total: total, shuttleStart: at, nextAt: at, done: true };
}

// The result is the last fully completed shuttle
function result(m, p) {
  var tab = shuttles(m);
  if (p.done) return { l: tab.length, s: tab[tab.length - 1] };
  if (p.shuttle > 0) return { l: p.level, s: p.shuttle };
  if (p.level > 1) return { l: p.level - 1, s: tab[p.level - 2] };
  return { l: 0, s: 0 };
}

// Estimated VO2max (ml/kg/min) for the 20m test only (Flouris et al. 2005)
function vo2max(m, l, s) {
  if (m != 20 || !l) return 0;
  return Math.round((3.46 * (l + s / (0.4325 * l + 7.0048)) + 12.2) * 10) / 10;
}
/*TABLES_END*/

var st;          // persisted state while a test runs
var log;         // StorageFile
var hr = { bpm: 0, conf: 0 };
var tBeep, tLog;

var HAS_BEEPER = process.env.HWVERSION == 1;
function cue(n, len) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) {
    p = p.then(function () {
      if (HAS_BEEPER) Bangle.beep(len, 2500);
      return Bangle.buzz(len, 1);
    });
    if (i < n - 1) p = p.then(function () { return new Promise(function (r) { setTimeout(r, 120); }); });
  }
  return p;
}

function onHRM(h) { hr.bpm = h.bpm; hr.conf = h.confidence; }

function now() { return getTime() - st.start; }

function distanceAt(p, t) {
  var f = (t - p.shuttleStart) / (p.nextAt - p.shuttleStart);
  if (!(f > 0)) f = 0;
  if (f > 1) f = 1;
  return (p.total + f) * st.m;
}

function writeSample(t, p) {
  log.write(t.toFixed(1) + "," + Math.round(hr.bpm) + "," + Math.round(hr.conf) + "," +
    p.level + "," + p.shuttle + "," + distanceAt(p, t).toFixed(1) + "\n");
}

function logTick() {
  var t = now();
  if (hr.bpm > 0) {
    st.hrSum += hr.bpm; st.hrN++;
    if (hr.bpm > st.hrMax) st.hrMax = hr.bpm;
  }
  writeSample(t, position(st.m, t));
}

function emit(type, data) { Bangle.emit("bleeptest", { type: type, data: data }); }

function updateWidget() {
  if (global.WIDGETS && WIDGETS.bleeptest) WIDGETS.bleeptest.update();
}

// Schedule the next beep. Called on start, on resume and after each beep.
function schedule() {
  var p = position(st.m, now() + 0.05);
  if (p.done) { stop(true); return; }
  tBeep = setTimeout(function () {
    tBeep = undefined;
    var q = position(st.m, p.nextAt + 0.001);
    if (q.done) { stop(true); return; }
    if (q.shuttle == 0) {
      // exact sample on the level boundary so exported laps line up
      writeSample(p.nextAt, q);
      cue(3, 180);
      emit("level", q);
    } else {
      cue(1, 250);
      emit("shuttle", q);
    }
    schedule();
  }, (st.start + p.nextAt - getTime()) * 1000);
}

function run() {
  Bangle.on("HRM", onHRM);
  Bangle.setHRMPower(1, "bleeptest");
  log = S.open(st.file, "a");
  schedule();
  tLog = setInterval(logTick, 1000);
  updateWidget();
}

function halt() {
  if (tBeep) clearTimeout(tBeep);
  if (tLog) clearInterval(tLog);
  tBeep = tLog = undefined;
  Bangle.removeListener("HRM", onHRM);
  Bangle.setHRMPower(0, "bleeptest");
}

// Start a test on an m metre course now
exports.start = function (m) {
  if (st) return;
  m = TESTS[m] ? m : 20;
  var start = getTime();
  st = { start: start, m: m, file: "bleep." + Math.floor(start) + ".csv", hrSum: 0, hrN: 0, hrMax: 0 };
  S.writeJSON(STATE, st);
  log = S.open(st.file, "w");
  log.write("start," + Math.round(start * 1000) + "," + new Date().getTimezoneOffset() + "," + m + "\n");
  cue(3, 180);
  run();
  logTick();
};

// Resume a test after an app change (called from the boot file)
exports.resume = function () {
  if (st) return true;
  st = S.readJSON(STATE, 1);
  if (!st || !st.start) { st = undefined; return false; }
  // if the test would have ended while nothing was running, close it off
  if (position(st.m, now()).done) {
    log = S.open(st.file, "a");
    stop(true);
    return false;
  }
  run();
  return true;
};

// Stop the test. at: seconds since start when the runner dropped out (default now).
exports.stop = stop;
function stop(completed, at) {
  if (!st) return;
  halt();
  var t = at !== undefined ? at : now();
  var p = position(st.m, t);
  if (completed) { p = position(st.m, 1e9); t = p.nextAt; }
  writeSample(t, p);
  var r = result(st.m, p);
  var res = {
    t: Math.round(st.start * 1000), m: st.m, l: r.l, s: r.s,
    d: p.total * st.m, dur: Math.round(t), vo2: vo2max(st.m, r.l, r.s),
    max: st.hrMax, avg: st.hrN ? Math.round(st.hrSum / st.hrN) : 0, f: st.file
  };
  log.write("end," + t.toFixed(1) + "," + res.l + "," + res.s + "," + res.d + "," + res.vo2 + "," + res.max + "," + res.avg + "\n");
  log = undefined;
  var hist = S.readJSON(HISTORY, 1) || [];
  hist.unshift(res);
  while (hist.length > 30) hist.pop();
  S.writeJSON(HISTORY, hist);
  S.erase(STATE);
  st = undefined;
  cue(2, 500);
  updateWidget();
  emit("done", res);
  return res;
}

exports.active = function () { return !!st; };

// Live status for the app and widget
exports.status = function () {
  if (!st) return;
  var t = now(), p = position(st.m, t);
  return {
    m: st.m, t: t, level: p.level, shuttle: p.shuttle, shuttles: shuttles(st.m)[p.level - 1],
    total: p.total, dist: p.total * st.m, speed: speed(p.level),
    left: p.nextAt - t, dur: p.nextAt - p.shuttleStart, bpm: hr.bpm
  };
};

exports.TESTS = TESTS;
exports.position = position;
exports.result = result;
exports.vo2max = vo2max;
exports.speed = speed;
exports.shuttleTime = shuttleTime;

// keep the heart rate stats when the running app is replaced
E.on("kill", function () {
  if (st) S.writeJSON(STATE, st);
});
