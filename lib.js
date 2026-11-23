// Beep Test engine, stored on the watch as module "beeptest".
// The schedule is a pure function of time since the start, so a test keeps
// running across app changes: the boot file resumes it from beeptest.state.json
// whenever any app (e.g. the clock) loads.

var S = require("Storage");
var STATE = "beeptest.state.json";
var HISTORY = "beeptest.json";

/*TABLES_START*/
// Speeds are the same for every course length: 8.0, 9.0, then +0.5 km/h per level.
// Each level lasts about a minute, so shorter courses need more shuttles per level.
var TESTS = {
  // Standard 20m multistage fitness test (Leger / UK beep test)
  20: [7,8,8,9,9,10,10,11,11,11,12,12,13,13,13,14,14,15,15,16,16],
  // 15m version, FitnessGram 15m PACER lap counts
  15: [9,10,11,12,12,13,13,14,14,15,15,16,17,17,18,18,19,19,20,20,21]
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

/*TABLES_END*/

var st;          // persisted state while a test runs
var log;         // StorageFile
var hr = { bpm: 0, conf: 0 };
var gps = { speed: NaN, lat: NaN, lon: NaN }; // speed km/h, NaN = no fix
var tBeep, tLog;

// Vibration cues: one short buzz per shuttle, two at each new level
var SHUTTLE_BUZZ = 180, LEVEL_BUZZ = 150; // ms
function cueShuttle() { cue(1, SHUTTLE_BUZZ); }
function cueLevel() { cue(2, LEVEL_BUZZ); }
function cue(n, len) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) {
    p = p.then(function () {
      return Bangle.buzz(len, 1);
    });
    if (i < n - 1) p = p.then(function () { return new Promise(function (r) { setTimeout(r, 120); }); });
  }
  return p;
}

function onHRM(h) { hr.bpm = h.bpm; hr.conf = h.confidence; }
function onGPS(f) {
  if (f.fix && !isNaN(f.speed)) { gps.speed = f.speed; gps.lat = f.lat; gps.lon = f.lon; }
  else gps.speed = gps.lat = gps.lon = NaN;
}
function fmtNum(v, dp) { return isNaN(v) ? "" : v.toFixed(dp); }

function now() { return getTime() - st.start; }

function distanceAt(p, t) {
  var f = (t - p.shuttleStart) / (p.nextAt - p.shuttleStart);
  if (!(f > 0)) f = 0;
  if (f > 1) f = 1;
  return (p.total + f) * st.m;
}

function writeSample(t, p) {
  log.write(t.toFixed(1) + "," + Math.round(hr.bpm) + "," + Math.round(hr.conf) + "," +
    p.level + "," + p.shuttle + "," + distanceAt(p, t).toFixed(1) +
    (st.gps ? "," + fmtNum(gps.speed, 1) + "," + fmtNum(gps.lat, 6) + "," + fmtNum(gps.lon, 6) : "") + "\n");
}

function logTick() {
  var t = now();
  if (hr.bpm > 0) {
    st.hrSum += hr.bpm; st.hrN++;
    if (hr.bpm > st.hrMax) st.hrMax = hr.bpm;
  }
  writeSample(t, position(st.m, t));
}

function emit(type, data) { Bangle.emit("beeptest", { type: type, data: data }); }

function updateWidget() {
  if (global.WIDGETS && WIDGETS.beeptest) WIDGETS.beeptest.update();
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
      cueLevel();
      emit("level", q);
    } else {
      cueShuttle();
      emit("shuttle", q);
    }
    schedule();
  }, (st.start + p.nextAt - getTime()) * 1000);
}

function run() {
  Bangle.on("HRM", onHRM);
  Bangle.setHRMPower(1, "beeptest");
  if (st.gps) {
    Bangle.on("GPS", onGPS);
    Bangle.setGPSPower(1, "beeptest");
  }
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
  Bangle.setHRMPower(0, "beeptest");
  if (st && st.gps) {
    Bangle.removeListener("GPS", onGPS);
    Bangle.setGPSPower(0, "beeptest");
  }
}

// Start a test on an m metre course. opts.gps: also record GPS speed/position.
// opts.start: start time (ms, as Date.now()), to begin partway through a test
// when testing on the watch, e.g. start(20, {start: Date.now() - 400000})
exports.start = function (m, opts) {
  if (st) return;
  opts = opts || {};
  m = TESTS[m] ? m : 20;
  var start = opts.start ? opts.start / 1000 : getTime();
  st = { start: start, m: m, file: "beeptest." + Math.floor(start) + ".csv", hrSum: 0, hrN: 0, hrMax: 0, gps: !!opts.gps };
  S.writeJSON(STATE, st);
  log = S.open(st.file, "w");
  log.write("start," + Math.round(start * 1000) + "," + new Date().getTimezoneOffset() + "," + m + "\n");
  cueLevel();
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
// (Espruino doesn't hoist functions in modules, so exports go after the definition.)
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
    d: p.total * st.m, dur: Math.round(t),
    max: st.hrMax, avg: st.hrN ? Math.round(st.hrSum / st.hrN) : 0, f: st.file
  };
  log.write("end," + t.toFixed(1) + "," + res.l + "," + res.s + "," + res.d + "," + res.max + "," + res.avg + "\n");
  log = undefined;
  var hist = S.readJSON(HISTORY, 1) || [];
  hist.unshift(res);
  while (hist.length > 30) hist.pop();
  S.writeJSON(HISTORY, hist);
  S.erase(STATE);
  st = undefined;
  cue(2, 350);
  updateWidget();
  emit("done", res);
  return res;
}
exports.stop = stop;

// Abandon the test: no result, and its HR log is deleted
exports.cancel = function () {
  if (!st) return;
  halt();
  log = undefined;
  S.open(st.file, "r").erase();
  S.erase(STATE);
  st = undefined;
  updateWidget();
  emit("cancel");
};

exports.active = function () { return !!st; };

// Live status for the app and widget
exports.status = function () {
  if (!st) return;
  var t = now(), p = position(st.m, t);
  return {
    m: st.m, t: t, level: p.level, shuttle: p.shuttle, shuttles: shuttles(st.m)[p.level - 1],
    total: p.total, dist: p.total * st.m, speed: speed(p.level),
    left: p.nextAt - t, dur: p.nextAt - p.shuttleStart, bpm: hr.bpm,
    gps: st.gps, gpsSpeed: gps.speed
  };
};

exports.TESTS = TESTS;
exports.position = position;
exports.result = result;
exports.speed = speed;
exports.shuttleTime = shuttleTime;

// keep the heart rate stats when the running app is replaced
E.on("kill", function () {
  if (st) S.writeJSON(STATE, st);
});
