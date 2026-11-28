/* Beep Test's side of the sync with the Beep Test phone app, installed as
   module "beeptest.sync". The boot file hands it every GB() message from the
   phone whose t starts with "beeptest-", and lib.js and app.js ask it to
   tell the phone the watch's state when something the phone keeps changes:
   a test starting or ending, a result deleted, the course, speeds or GPS
   setting.

   Phone to watch: the Beep Test Android app broadcasts com.banglejs.uart.tx
   and Gadgetbridge passes the line on to the watch (or the phone writes it
   to the UART itself), where Android Integration calls GB(). Watch to phone:
   a Gadgetbridge intent message (needs "Allow Intents" in Gadgetbridge).
   The protocol is in docs/DEVELOPMENT.md, "The watch link". */
var S = require("Storage");
var HISTORY = "beeptest.json";
var SETTINGS = "beeptest.settings.json";
var STATE = "beeptest.state.json";
var SYNC = "beeptest.sync.json"; // {del: the logs of results deleted on the watch, newest last}
var PACKAGE = "com.cbishopvelti.beep";
var ACTION = PACKAGE + ".WATCH";
var LOG = /^beeptest\.\d+\.csv$/;
var PIECE = 1000; // bytes of a file in each "log" message
var TICK = 50;    // ms between pieces, so the watch stays responsive
var SKIP = 10;    // pieces' worth of a log skipped per tick to reach the offset asked for
var KEEP = 30;    // deletions remembered, as many as results

var xfer; // the file being sent: {f, at, pos, file (a log's StorageFile) or data (the history), t (its timer)}

function connected() {
  return !!NRF.getSecurityStatus().connected;
}

/* Sends a Gadgetbridge intent message to the Beep Test Android app, as
   blink's watch app does. Extras go as strings, which is how Gadgetbridge
   puts them on the intent. Nothing is sent with no phone connected. */
function send(extra) {
  if (!connected()) return;
  var m = { t: "intent", target: "broadcastreceiver", action: ACTION, extra: extra };
  m["package"] = PACKAGE;
  var android;
  try { android = require("android"); } catch (e) { }
  if (android && android.gbSend) android.gbSend(m);
  else {
    Bluetooth.println("");
    Bluetooth.println(JSON.stringify(m));
  }
}

// E.CRC32 as an unsigned decimal, the same number as zlib's crc32
function crc(s) {
  return String(E.CRC32(s) >>> 0);
}

// The running test's log file name, or ""
function running() {
  return (S.readJSON(STATE, 1) || {}).file || "";
}

/* The history as JSON. Storage.writeJSON leaves the quotes off keys
   ({t:1781344800000,...}), which a JSON parser on the phone refuses, so
   the phone gets (and state's hist describes) JSON.stringify of it. */
function history() {
  var h = S.readJSON(HISTORY, 1);
  return h === undefined ? undefined : JSON.stringify(h);
}

/* The engine's speed progressions (lib.js PROGS), or undefined when the
   installed engine has none (before the Speeds setting): then state has
   no p and beeptest-set ignores one. */
function progs() {
  var P = require("beeptest").PROGS;
  return Array.isArray(P) && P.length ? P : undefined;
}

function tombstones() {
  var r = S.readJSON(SYNC, 1);
  return r && Array.isArray(r.del) ? r.del : [];
}

/* Tells the phone how the watch is: the version installed, the settings
   (with the speed progression when the engine has them, as app.js reads
   it: 0 unless it's one of them), the running test, the history's length
   and CRC (so the phone fetches it only when it changed), what was
   deleted on the watch, and n: the n of the beeptest-sync this answers
   (digits; "" when it answers none), so the phone can tell its answer
   from a state sent unasked. */
function state(n) {
  if (!connected()) return;
  var s = S.readJSON(SETTINGS, 1) || {}, h = history();
  var info = S.readJSON("beeptest.info", 1) || {};
  var e = {
    event: "state",
    v: String(info.version || ""),
    m: s.m == 15 ? "15" : "20",
    gps: s.gps ? "true" : "false",
    run: running(),
    hist: h === undefined ? "0:0" : h.length + ":" + crc(h),
    del: tombstones().join(","),
    n: typeof n == "string" && n.length <= 20 && /^\d+$/.test(n) ? n : ""
  };
  var P = progs();
  if (P) e.p = String(P[s.p] ? s.p : 0);
  send(e);
}

function stop() {
  if (xfer && xfer.t) clearTimeout(xfer.t);
  xfer = undefined;
}

/* A load() (back to the clock, a message opening) ends a transfer without
   a word: it clears the timers and this module. So say it was cut off in
   KILLED, and the boot file sends state once the next app is up, which has
   the phone ask again from where it got to. */
var KILLED = "beeptest.xfer";
E.on("kill", function () {
  if (xfer) try { S.write(KILLED, xfer.f); } catch (e) { }
});

function end(len) {
  var f = xfer.f;
  stop();
  send({ event: "logend", f: f, len: String(len) });
}

// A file being sent is deleted: the phone hears it has no log
function gone(f) {
  if (xfer && xfer.f == f) end(-1);
}

/* Sends the next piece of the file being sent, one per tick. A log is a
   StorageFile, read from the start: up to SKIP pieces are passed over each
   tick until the offset asked for. The history is sent as it was when it
   was asked for (if it changes meanwhile, state says so, and the phone
   finds the CRC of what it got doesn't match and asks again). */
function next() {
  var x = xfer, s;
  x.t = undefined;
  if (!connected()) return stop();
  // a test started: reading storage must not delay its cues
  if (running()) {
    stop();
    return send({ event: "busy", f: x.f });
  }
  if (x.file) {
    for (var n = 0; x.pos < x.at && n < SKIP; n++) {
      s = x.file.read(Math.min(PIECE, x.at - x.pos));
      if (s === undefined) return end(x.pos);
      x.pos += s.length;
    }
    if (x.pos < x.at) {
      x.t = setTimeout(next, TICK);
      return;
    }
    s = x.file.read(PIECE);
    if (s === undefined) return end(x.pos);
  } else {
    if (x.pos >= x.data.length) return end(x.data.length);
    s = x.data.substr(x.pos, PIECE);
  }
  send({ event: "log", f: x.f, at: String(x.pos), crc: crc(s), d: s });
  x.pos += s.length;
  x.t = setTimeout(next, TICK);
}

/* beeptest-get: sends file f from byte `at`, replacing any transfer in
   progress. Only a log or the history, never while a test runs. */
function get(f, at) {
  stop();
  f = String(f);
  at = Math.floor(+at) || 0;
  if (at < 0) at = 0;
  if (running()) return send({ event: "busy", f: f });
  var x = { f: f, at: at, pos: 0 };
  if (LOG.test(f) && S.list(/^beeptest\.\d+\.csv\x01$/).indexOf(f + "\x01") >= 0) x.file = S.open(f, "r");
  else if (f == HISTORY && (x.data = history()) !== undefined) x.pos = at;
  else return send({ event: "logend", f: f, len: "-1" });
  xfer = x;
  x.t = setTimeout(next, 0);
}

/* beeptest-sync: deletes the results the phone deleted (their history
   entries and logs; never the running test's), then answers state,
   echoing the sync's n. */
function sync(del, n) {
  var run = running();
  var names = (Array.isArray(del) ? del : []).filter(function (f) {
    return typeof f == "string" && LOG.test(f) && f != run;
  });
  if (names.length) {
    var hist = S.readJSON(HISTORY, 1);
    if (Array.isArray(hist)) {
      var kept = hist.filter(function (r) { return !r || names.indexOf(r.f) < 0; });
      if (kept.length != hist.length) S.writeJSON(HISTORY, kept);
    }
    names.forEach(function (f) {
      gone(f);
      S.open(f, "r").erase();
    });
  }
  state(n);
}

/* beeptest-set: the course (20 or 15), GPS speed and speed progression
   (an index into the engine's PROGS, when it has them) chosen on the
   phone, shown by the app if it's open (a "beeptest" event "settings"). */
function set(e) {
  var s = S.readJSON(SETTINGS, 1) || {}, P = progs();
  if (e.m == 20 || e.m == 15) s.m = +e.m;
  if (typeof e.gps == "boolean") s.gps = e.gps;
  if (P && typeof e.p == "number" && P[e.p]) s.p = e.p;
  S.writeJSON(SETTINGS, s);
  Bangle.emit("beeptest", { type: "settings" });
  state();
}

/* A result deleted on the watch (its log file name): remembered in
   beeptest.sync.json, which state() reports, so the phone deletes it too.
   The caller deletes the files and sends state. */
function deleted(f) {
  var del = tombstones().filter(function (d) { return d != f; });
  del.push(f);
  while (del.length > KEEP) del.shift();
  S.writeJSON(SYNC, { del: del });
  gone(f);
}

// A message from the phone ({t:"beeptest-..."}, from the boot file's GB())
function msg(e) {
  if (e.t == "beeptest-sync") sync(e.del, e.n);
  else if (e.t == "beeptest-get") get(e.f, e.at);
  else if (e.t == "beeptest-set") set(e);
}

exports.state = state;
exports.msg = msg;
exports.deleted = deleted;
exports.crc = crc;
