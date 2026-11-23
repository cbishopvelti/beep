// Beep Test - multistage shuttle run (20m or 15m) for Bangle.js 2
// The test itself runs in the "beeptest" module (lib.js), which the boot file
// resumes on every app load, so it carries on if you go back to the clock.
//
// Controls: short button press opens the menu, long press goes to the clock
// (the test keeps running and the widget shows it), tap the screen to start.

var storage = require("Storage");
var bt = global.beepTest = require("beeptest");
var SETTINGS = "beeptest.settings.json";
var HISTORY = "beeptest.json";
var COUNTDOWN = 3;
var COURSES = [20, 15];

var settings = storage.readJSON(SETTINGS, 1) || {};
if (COURSES.indexOf(settings.m) < 0) settings.m = 20;

var view;      // "ready", "countdown", "run", "result", "menu"
var timers = {};

function clearTimers() {
  for (var k in timers) { clearTimeout(timers[k]); clearInterval(timers[k]); }
  timers = {};
}

function fmtTime(s) {
  s = Math.floor(s);
  var m = Math.floor(s / 60);
  s = s % 60;
  return m + ":" + (s < 10 ? "0" : "") + s;
}

function heart(x, y, r) {
  g.setColor(g.theme.dark ? "#f44" : "#c00");
  g.fillCircle(x - r / 2, y, r / 2).fillCircle(x + r / 2, y, r / 2);
  g.fillPoly([x - r, y + 1, x + r, y + 1, x, y + r * 1.3]);
  g.setColor(g.theme.fg);
}


// A custom screen: short button press opens the menu. A long press is left to
// the firmware, which goes back to the clock.
function setScreen(name, opts) {
  opts = opts || {};
  E.showMenu();
  clearTimers();
  view = name;
  var down;
  var w = setWatch(function (e) {
    if (e.state) { down = e.time; return; }
    // released: only a short press counts (a long hold goes to the clock)
    if (down !== undefined && e.time - down < 1) (opts.btn || showMenu)();
    down = undefined;
  }, BTN1, { edge: "both", repeat: true, debounce: 25 });
  Bangle.setUI({
    mode: "custom",
    touch: opts.touch,
    remove: function () { clearWatch(w); clearTimers(); if (opts.remove) opts.remove(); }
  });
  g.reset().clearRect(Bangle.appRect);
}

// ---------- ready ----------
function showReady() {
  if (bt.active()) return showRun();
  Bangle.setHRMPower(0, "beeptest.app"); // in case a countdown was cancelled
  setScreen("ready", { touch: startCountdown });
  var R = Bangle.appRect, cx = R.x + R.w / 2;
  g.setFontAlign(0, -1).setFont("6x8", 2).drawString("Beep Test", cx, R.y + 4);
  g.setFont("Vector", 40).drawString(settings.m + "m", cx, R.y + 26);
  g.setFont("6x8", 2).drawString("Tap to start", cx, R.y + 76);
  var last = (storage.readJSON(HISTORY, 1) || [])[0];
  g.setFont("6x8");
  if (last) g.drawString("Last: " + last.l + "." + last.s + " (" + (last.m || 20) + "m)", cx, R.y + 104);
}

// ---------- countdown ----------
function startCountdown() {
  setScreen("countdown", { btn: showReady });
  // warm the HRM up so there is a reading at the start
  Bangle.setHRMPower(1, "beeptest.app");
  var n = COUNTDOWN;
  function tick() {
    var R = Bangle.appRect, cx = R.x + R.w / 2;
    g.reset().clearRect(R);
    g.setFontAlign(0, 0).setFont("6x8", 2).drawString("Get ready " + settings.m + "m", cx, R.y + 20);
    g.setFont("Vector", 60).drawString(n, cx, R.y + R.h / 2 + 5);
    g.setFont("6x8").drawString("BTN: cancel", cx, R.y2 - 8);
    if (n <= 0) {
      bt.start(settings.m, { gps: settings.gps });
      Bangle.setHRMPower(0, "beeptest.app");
      showRun();
      return;
    }
    Bangle.buzz(80, 0.6);
    n--;
    timers.cd = setTimeout(tick, 1000);
  }
  tick();
}

// ---------- running ----------
// The watch locks and dims as usual during a test. While it's locked the
// screen redraws once a second instead of four times, to save battery.
function showRun() {
  function rate() {
    if (timers.draw) clearInterval(timers.draw);
    timers.draw = setInterval(drawRun, Bangle.isLocked() ? 1000 : 250);
    drawRun();
  }
  setScreen("run", { remove: function () { Bangle.removeListener("lock", rate); } });
  Bangle.on("lock", rate);
  rate();
}

function drawRun() {
  var s = bt.status();
  if (!s) return;
  var R = Bangle.appRect, cx = R.x + R.w / 2;
  var f = 1 - s.left / s.dur;
  if (f < 0) f = 0;
  if (f > 1) f = 1;
  g.reset().clearRect(R);
  g.setFontAlign(0, -1).setFont("6x8").drawString("LEVEL", R.x + R.w / 4, R.y + 2).drawString("SHUTTLE", R.x + 3 * R.w / 4, R.y + 2);
  g.setFont("Vector", 44).drawString(s.level, R.x + R.w / 4, R.y + 12);
  g.setFont("Vector", s.shuttles > 9 ? 26 : 30).drawString((s.shuttle + 1) + "/" + s.shuttles, R.x + 3 * R.w / 4, R.y + 18);
  var by = R.y + 62;
  g.drawRect(R.x + 4, by, R.x2 - 4, by + 14);
  g.fillRect(R.x + 6, by + 2, R.x + 6 + (R.w - 12) * f, by + 12);
  g.setFont("6x8").setFontAlign(0, -1).drawString(s.m + "m  " + s.speed.toFixed(1) + " km/h  " + Math.max(0, s.left).toFixed(1) + "s", cx, by + 18);
  heart(R.x + 14, by + 42, 8);
  g.setFont("Vector", 26).setFontAlign(-1, -1).drawString(s.bpm > 0 ? Math.round(s.bpm) : "--", R.x + 28, by + 32);
  g.setFont("Vector", s.dist >= 1000 ? 20 : 26).setFontAlign(1, -1).drawString(s.dist + "m", R.x2 - 4, by + 34);
  g.setFont("6x8").setFontAlign(-1, 1).drawString(fmtTime(s.t), R.x + 4, R.y2 - 2);
  if (s.gps) g.setFontAlign(1, 1).drawString("GPS " + (isNaN(s.gpsSpeed) ? "--" : s.gpsSpeed.toFixed(1)) + " km/h", R.x2 - 4, R.y2 - 2);
}

Bangle.on("beeptest", function (e) {
  if (e.type == "done" && (view == "run" || view == "menu")) {
    showResult(e.data, showReady);
  }
});

// ---------- menu ----------
function showMenu() {
  view = "menu";
  var m;
  if (bt.active()) {
    // the moment the button was pressed counts as dropping out
    var stopAt = bt.status().t;
    m = {
      "": { title: "Test running", back: showRun },
      "Back to test": showRun,
      "Stop & save": function () {
        var res = bt.stop(false, stopAt);
        showResult(res, showReady);
      },
      "Cancel test": function () {
        E.showPrompt("Cancel without saving?").then(function (ok) {
          if (ok) { bt.cancel(); showReady(); }
          else showMenu();
        });
      }
    };
  } else {
    m = {
      "": { title: "Beep Test", back: showReady },
      "Start test": startCountdown,
      "Course": {
        value: COURSES.indexOf(settings.m),
        min: 0, max: COURSES.length - 1,
        format: function (v) { return COURSES[v] + "m"; },
        onchange: function (v) { settings.m = COURSES[v]; storage.writeJSON(SETTINGS, settings); }
      },
      "GPS speed": {
        value: !!settings.gps,
        onchange: function (v) { settings.gps = v; storage.writeJSON(SETTINGS, settings); }
      },
      "Results": showHistory,
      "Exit": function () { load(); }
    };
  }
  E.showMenu(m);
}

// ---------- results ----------
function showResult(res, back) {
  setScreen("result", { btn: back, touch: back });
  var R = Bangle.appRect, cx = R.x + R.w / 2;
  g.setFontAlign(0, -1).setFont("6x8").drawString(new Date(res.t).toString().substr(0, 21), cx, R.y + 2);
  g.setFont("Vector", 40).drawString(res.l + "." + res.s, cx, R.y + 14);
  g.setFont("6x8").drawString("level . shuttle  (" + (res.m || 20) + "m)", cx, R.y + 56);
  g.setFont("6x8", 2);
  g.drawString(res.d + "m  " + fmtTime(res.dur), cx, R.y + 76);
  g.drawString("HR " + (res.avg || "--") + " / " + (res.max || "--"), cx, R.y + 100);
  g.setFont("6x8").drawString("avg / max bpm", cx, R.y + 118);
}

function showHistory() {
  view = "menu";
  var hist = storage.readJSON(HISTORY, 1) || [];
  var m = { "": { title: "Results", back: showMenu } };
  if (!hist.length) m["No results yet"] = function () {};
  hist.forEach(function (res, i) {
    var d = new Date(res.t);
    var label = d.getDate() + "/" + (d.getMonth() + 1) + " " + (res.m || 20) + "m " + res.l + "." + res.s;
    m[label] = function () { showEntry(i); };
  });
  E.showMenu(m);
}

function showEntry(i) {
  var hist = storage.readJSON(HISTORY, 1) || [];
  var res = hist[i];
  if (!res) return showHistory();
  showResult(res, function () {
    view = "menu";
    E.showMenu({
      "": { title: res.l + "." + res.s, back: showHistory },
      "Show again": function () { showEntry(i); },
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

g.clear();
Bangle.loadWidgets();
Bangle.drawWidgets();
showReady();
