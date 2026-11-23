// Bleep Test - 20m multistage shuttle run for Bangle.js
// Timings follow the standard UK/Leger 20m MSFT table:
// level 1 at 8.0 km/h, level 2 at 9.0 km/h, then +0.5 km/h per level.
// Heart rate is logged once a second to bleep.<unixtime>.csv (StorageFile),
// which the App Loader page (interface.html) converts to TCX.

/*LEVELS_START*/
var SHUTTLE_M = 20;
var SHUTTLES = [7,8,8,9,9,10,10,11,11,11,12,12,13,13,13,14,14,15,15,16,16];
function levelSpeed(l) { return l == 1 ? 8 : 8 + 0.5 * l; } // km/h
function shuttleTime(l) { return SHUTTLE_M * 3.6 / levelSpeed(l); } // seconds
// Result is the last fully completed shuttle: {l:level, s:shuttle}
function resultFor(level, shuttle) {
  if (shuttle > 0) return { l: level, s: shuttle };
  if (level > 1) return { l: level - 1, s: SHUTTLES[level - 2] };
  return { l: 0, s: 0 };
}
// Estimated VO2max (ml/kg/min), Ramsbottom table / Flouris et al. 2005
function vo2max(l, s) {
  if (!l) return 0;
  return Math.round((3.46 * (l + s / (0.4325 * l + 7.0048)) + 12.2) * 10) / 10;
}
/*LEVELS_END*/

var storage = require("Storage");
var HISTORY = "bleep.json";
var COUNTDOWN = 5;

var state = "menu";
var run; // current test
var hr = { bpm: 0, conf: 0 };
var timers = {};
var prevOpts;

function clearTimers() {
  for (var k in timers) { clearTimeout(timers[k]); clearInterval(timers[k]); }
  timers = {};
}

// ---------- cues ----------
var HAS_BEEPER = process.env.HWVERSION == 1;
function wait(ms) { return new Promise(function (r) { setTimeout(r, ms); }); }
function cue(n, len) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) {
    p = p.then(function () {
      if (HAS_BEEPER) Bangle.beep(len, 2500);
      return Bangle.buzz(len, 1);
    });
    if (i < n - 1) p = p.then(function () { return wait(120); });
  }
  return p;
}
function cueShuttle() { cue(1, 250); }
function cueLevel() { cue(3, 180); }

// ---------- HRM ----------
function onHRM(h) { hr.bpm = h.bpm; hr.conf = h.confidence; }
function hrmOn() { Bangle.on("HRM", onHRM); Bangle.setHRMPower(1, "bleeptest"); }
function hrmOff() { Bangle.removeListener("HRM", onHRM); Bangle.setHRMPower(0, "bleeptest"); }

// ---------- screen options ----------
function keepAwake() {
  prevOpts = Bangle.getOptions ? Bangle.getOptions() : undefined;
  Bangle.setOptions({ lockTimeout: 0, backlightTimeout: 0 });
  Bangle.setLocked(false);
  Bangle.setLCDPower(1);
}
function restoreAwake() {
  if (prevOpts) Bangle.setOptions({ lockTimeout: prevOpts.lockTimeout, backlightTimeout: prevOpts.backlightTimeout });
  prevOpts = undefined;
}

function fmtTime(s) {
  s = Math.floor(s);
  var m = Math.floor(s / 60);
  s = s % 60;
  return m + ":" + (s < 10 ? "0" : "") + s;
}

// ---------- test flow ----------
function startCountdown() {
  E.showMenu();
  state = "countdown";
  keepAwake();
  hrmOn();
  Bangle.setUI({ mode: "custom", btn: function () { abort(); } });
  var n = COUNTDOWN;
  function tick() {
    var R = Bangle.appRect;
    g.reset().clearRect(R);
    g.setFontAlign(0, 0).setFont("6x8", 2).drawString("Get ready", R.x + R.w / 2, R.y + 20);
    g.setFont("Vector", 60).drawString(n, R.x + R.w / 2, R.y + R.h / 2 + 5);
    g.setFont("6x8").drawString("BTN: cancel", R.x + R.w / 2, R.y2 - 8);
    if (n <= 0) { startTest(); return; }
    Bangle.buzz(80, 0.6);
    n--;
    timers.cd = setTimeout(tick, 1000);
  }
  tick();
}

function abort() {
  clearTimers();
  hrmOff();
  restoreAwake();
  state = "menu";
  showMenu();
}

function startTest() {
  clearTimers();
  state = "run";
  var now = Date.now();
  run = {
    start: getTime(),
    startMs: now,
    level: 1,
    shuttle: 0,      // completed shuttles in current level
    total: 0,        // completed shuttles overall
    shuttleStart: 0, // seconds since start
    nextAt: shuttleTime(1),
    hrSum: 0, hrN: 0, hrMax: 0,
    file: "bleep." + Math.floor(now / 1000) + ".csv"
  };
  run.log = storage.open(run.file, "w");
  run.log.write("start," + now + "," + (new Date().getTimezoneOffset()) + "\n");
  Bangle.setUI({ mode: "custom", btn: function () { finish(false); } });
  cueLevel();
  scheduleBeep();
  timers.draw = setInterval(drawRun, 250);
  timers.log = setInterval(function () { logSample(); }, 1000);
  logSample();
  drawRun();
}

function scheduleBeep() {
  var delay = (run.start + run.nextAt - getTime()) * 1000;
  timers.beep = setTimeout(onBeep, delay > 0 ? delay : 0);
}

function onBeep() {
  run.shuttle++;
  run.total++;
  run.shuttleStart = run.nextAt;
  var newLevel = false;
  if (run.shuttle >= SHUTTLES[run.level - 1]) {
    if (run.level >= SHUTTLES.length) { finish(true); return; }
    run.level++;
    run.shuttle = 0;
    newLevel = true;
    cueLevel();
  } else {
    cueShuttle();
  }
  run.nextAt += shuttleTime(run.level);
  scheduleBeep();
  // extra sample exactly on the level boundary so exported laps line up
  if (newLevel) logSample(true);
}

function elapsed() { return getTime() - run.start; }

function distanceAt(t) {
  // completed shuttles plus fraction of the current one
  var dur = run.nextAt - run.shuttleStart;
  var f = (t - run.shuttleStart) / dur;
  if (f < 0) f = 0;
  if (f > 1) f = 1;
  return (run.total + f) * SHUTTLE_M;
}

function logSample(boundary) {
  var t = boundary ? run.shuttleStart : elapsed();
  if (!boundary && hr.bpm > 0) {
    run.hrSum += hr.bpm; run.hrN++;
    if (hr.bpm > run.hrMax) run.hrMax = hr.bpm;
  }
  run.log.write(t.toFixed(1) + "," + Math.round(hr.bpm) + "," + Math.round(hr.conf) + "," +
    run.level + "," + run.shuttle + "," + distanceAt(t).toFixed(1) + "\n");
}

function drawRun() {
  if (state != "run") return;
  var R = Bangle.appRect, cx = R.x + R.w / 2;
  var t = elapsed();
  var dur = run.nextAt - run.shuttleStart;
  var left = run.start + run.nextAt - getTime();
  var f = 1 - left / dur;
  if (f < 0) f = 0;
  if (f > 1) f = 1;
  g.reset().clearRect(R);
  g.setFontAlign(0, -1).setFont("6x8").drawString("LEVEL", R.x + R.w / 4, R.y + 2).drawString("SHUTTLE", R.x + 3 * R.w / 4, R.y + 2);
  g.setFont("Vector", 44).drawString(run.level, R.x + R.w / 4, R.y + 12);
  g.setFont("Vector", 30).drawString((run.shuttle + 1) + "/" + SHUTTLES[run.level - 1], R.x + 3 * R.w / 4, R.y + 18);
  // progress bar for the current shuttle
  var by = R.y + 62;
  g.drawRect(R.x + 4, by, R.x2 - 4, by + 14);
  g.fillRect(R.x + 6, by + 2, R.x + 6 + (R.w - 12) * f, by + 12);
  g.setFont("6x8").setFontAlign(0, -1).drawString(levelSpeed(run.level).toFixed(1) + " km/h  " + Math.max(0, left).toFixed(1) + "s", cx, by + 18);
  g.setFont("Vector", 26).setFontAlign(-1, -1);
  g.setColor(g.theme.dark ? "#f44" : "#c00").drawString("♥", R.x + 4, by + 32);
  g.setColor(g.theme.fg).drawString(hr.bpm > 0 ? Math.round(hr.bpm) : "--", R.x + 28, by + 32);
  g.setFontAlign(1, -1).drawString(run.total * SHUTTLE_M + "m", R.x2 - 4, by + 32);
  g.setFont("6x8").setFontAlign(-1, 1).drawString(fmtTime(t), R.x + 4, R.y2 - 2);
  g.setFontAlign(1, 1).drawString("BTN: stop", R.x2 - 4, R.y2 - 2);
}

function saveResult(completedAll) {
  clearTimers();
  logSample();
  var t = elapsed();
  var r = completedAll ? { l: SHUTTLES.length, s: SHUTTLES[SHUTTLES.length - 1] } : resultFor(run.level, run.shuttle);
  var res = {
    t: run.startMs,
    l: r.l, s: r.s,
    d: run.total * SHUTTLE_M,
    dur: Math.round(t),
    vo2: vo2max(r.l, r.s),
    max: run.hrMax,
    avg: run.hrN ? Math.round(run.hrSum / run.hrN) : 0,
    f: run.file
  };
  run.log.write("end," + t.toFixed(1) + "," + res.l + "," + res.s + "," + res.d + "," + res.vo2 + "," + res.max + "," + res.avg + "\n");
  run.log = undefined;
  var hist = storage.readJSON(HISTORY, 1) || [];
  hist.unshift(res);
  while (hist.length > 30) hist.pop();
  storage.writeJSON(HISTORY, hist);
  state = "done";
  hrmOff();
  restoreAwake();
  return res;
}

function finish(completedAll) {
  if (state != "run") return;
  var res = saveResult(completedAll);
  cue(2, 500);
  showResult(res, function () { showMenu(); });
}

// Close off a test in progress if the app is left (e.g. long button hold)
E.on("kill", function () {
  if (state == "run" && run && run.log) {
    try { saveResult(false); } catch (e) {}
  }
});

// ---------- results ----------
function showResult(res, back) {
  state = "result";
  var R = Bangle.appRect, cx = R.x + R.w / 2;
  g.reset().clearRect(R);
  g.setFontAlign(0, -1).setFont("6x8").drawString(new Date(res.t).toString().substr(0, 21), cx, R.y + 2);
  g.setFont("Vector", 40).drawString(res.l + "." + res.s, cx, R.y + 14);
  g.setFont("6x8", 1).drawString("level . shuttle", cx, R.y + 56);
  g.setFont("6x8", 2);
  g.drawString("VO2max ~" + res.vo2, cx, R.y + 70);
  g.drawString(res.d + "m  " + fmtTime(res.dur), cx, R.y + 90);
  g.drawString("HR " + (res.avg || "--") + " / " + (res.max || "--"), cx, R.y + 110);
  g.setFont("6x8").drawString("avg / max bpm", cx, R.y + 128);
  Bangle.setUI({ mode: "custom", btn: back, back: back, touch: back });
}

function showHistory() {
  var hist = storage.readJSON(HISTORY, 1) || [];
  var m = { "": { title: "Results", back: showMenu } };
  if (!hist.length) m["No results yet"] = function () {};
  hist.forEach(function (res, i) {
    var d = new Date(res.t);
    var label = d.getDate() + "/" + (d.getMonth() + 1) + " " + fmtTime(d.getHours() * 60 + d.getMinutes()) + "  " + res.l + "." + res.s;
    m[label] = function () { E.showMenu(); showEntry(i); };
  });
  E.showMenu(m);
}

function showEntry(i) {
  var hist = storage.readJSON(HISTORY, 1) || [];
  var res = hist[i];
  if (!res) return showHistory();
  showResult(res, function () {
    E.showMenu({
      "": { title: res.l + "." + res.s, back: showHistory },
      "Show again": function () { E.showMenu(); showEntry(i); },
      "Delete": function () {
        E.showPrompt("Delete result and HR log?").then(function (ok) {
          if (ok) {
            hist.splice(i, 1);
            storage.writeJSON(HISTORY, hist);
            if (res.f) storage.open(res.f, "r").erase();
          }
          showHistory();
        });
      }
    });
  });
}

function showMenu() {
  state = "menu";
  E.showMenu({
    "": { title: "Bleep Test", back: load },
    "Start test": startCountdown,
    "Results": showHistory,
    "Exit": function () { load(); }
  });
}

g.clear();
Bangle.loadWidgets();
Bangle.drawWidgets();
showMenu();
