/* The sync with the Beep Test phone app (sync.js and the boot file's GB()),
   message by message, as docs/DEVELOPMENT.md "The watch link" lists them:
   beeptest-sync, beeptest-get and beeptest-set from the phone, and state,
   log, logend and busy back. Run: node tests/run.js tests/sync.test.js -v */
const zlib = require("zlib");
const { test, assert } = require("./harness");
const { BT, startAgo, spyBuzz, spyMenu, spyDraw, readFile, ours, untilEnd, makeLog } = require("./helpers");

const meta = require("../metadata.json");
const crc = s => String(zlib.crc32(Buffer.from(s, "latin1")));
const A = "beeptest.1781344800.csv", B = "beeptest.1781258400.csv", C = "beeptest.1781172000.csv";

/* Three past tests on the watch, newest first, with their logs */
function threeResults(h) {
  const hist = [makeLog(h, 1781344800, 20, 400), makeLog(h, 1781258400, 15, 150), makeLog(h, 1781172000, 20, 30)];
  h.tx(`require("Storage").writeJSON("beeptest.json",${JSON.stringify(hist)})`);
  return hist;
}
const exists = (h, f) => h.eval(`require("Storage").list(/\\.csv\\x01$/).indexOf(${JSON.stringify(f + "\x01")})>=0`);
const history = h => h.eval('require("Storage").readJSON("beeptest.json",1)');
const last = h => { const o = ours(h); return o[o.length - 1]; };

/* The pieces of f sent since `from`, checked: in order from `at`, each
   at most 1000 bytes with the right CRC, then logend. Returns the text. */
function joined(got, at) {
  const pieces = got.filter(e => e.event == "log");
  let pos = at || 0, text = "";
  for (const p of pieces) {
    assert.eq(p.at, String(pos), "pieces follow on");
    assert(p.d.length > 0 && p.d.length <= 1000, "a piece is 1 to 1000 bytes: " + p.d.length);
    assert.eq(p.crc, crc(p.d), "piece CRC at " + p.at);
    for (const k of ["f", "at", "crc", "d"]) assert.eq(typeof p[k], "string", k + " is a string");
    pos += p.d.length;
    text += p.d;
  }
  const end = got[got.length - 1];
  assert.eq([end.event, end.len], ["logend", String(pos)], "logend with the file's length");
  assert.eq(got.filter(e => e.event == "logend").length, 1, "one logend");
  return text;
}

// ---------- state ----------

test("sync with nothing on the watch: state with every extra, as strings", async h => {
  await h.connect();
  await h.gb({ t: "beeptest-sync" });
  const s = last(h);
  assert.eq(s, { event: "state", v: meta.version, m: "20", gps: "false", run: "", hist: "0:0", del: "" });
  const m = h.sent().filter(m => m.t == "intent").pop();
  assert.eq([m.target, m.package, m.action], ["broadcastreceiver", "com.cbishopvelti.beep", "com.cbishopvelti.beep.WATCH"]);
});

/* The history as the phone gets it: JSON (Storage.writeJSON leaves the
   quotes off keys, which serde_json refuses) */
const histJSON = h => h.eval('JSON.stringify(require("Storage").readJSON("beeptest.json"))');

test("state's hist is the history's length and CRC32, the same as zlib's", async h => {
  threeResults(h);
  await h.connect();
  await h.gb({ t: "beeptest-sync" });
  const text = histJSON(h);
  assert.eq(JSON.parse(text).length, 3, "strict JSON");
  assert.eq(last(h).hist, text.length + ":" + crc(text));
  // E.CRC32 is unsigned and equal to zlib's for a few bytes too
  for (const s of ["", "a", "hello world", "\n\t~{"])
    assert.eq(h.eval(`require("beeptest.sync").crc(${JSON.stringify(s)})`), crc(s), JSON.stringify(s));
  assert(+crc("a") > 2147483647, "a CRC over 2^31 stays positive");
});

test("state says which test is running and the settings", async h => {
  h.tx('require("Storage").writeJSON("beeptest.settings.json",{m:15,gps:true})');
  startAgo(h, 15, 30);
  await h.connect();
  await h.gb({ t: "beeptest-sync" });
  const run = h.eval(`require("Storage").readJSON("beeptest.state.json").file`);
  assert.eq([last(h).m, last(h).gps, last(h).run], ["15", "true", run]);
});

test("state is sent when a phone connects, a moment later", async h => {
  await h.connect();
  assert.eq(ours(h), [], "not straight away");
  await h.wait(3000);
  assert.eq(ours(h).map(e => e.event), ["state"]);
});

test("state is sent when a test starts, ends and is cancelled", async h => {
  await h.connect();
  startAgo(h, 20, 10);
  const run = h.eval(`require("Storage").readJSON("beeptest.state.json").file`);
  assert.eq([last(h).event, last(h).run], ["state", run], "on start");
  h.tx(`${BT}.stop(false)`);
  assert.eq([last(h).run, last(h).hist.split(":")[0] > 0], ["", true], "on stop, with the new result");
  startAgo(h, 20, 10);
  const n = ours(h).length;
  h.tx(`${BT}.cancel()`);
  assert.eq(ours(h).length, n + 1);
  assert.eq(last(h).run, "", "on cancel");
});

test("a test that ends by itself sends state too", async h => {
  await h.connect();
  startAgo(h, 20, 1311);
  await h.wait(30000);
  assert(!h.eval(`${BT}.active()`), "finished");
  assert.eq(last(h).run, "");
  assert.eq(history(h)[0].l, 21);
});

test("nothing is sent with no phone connected", async h => {
  threeResults(h);
  await h.gb({ t: "beeptest-sync", del: [C] });
  await h.gb({ t: "beeptest-set", m: 15 });
  await h.gb({ t: "beeptest-get", f: A });
  await h.wait(2000);
  startAgo(h, 20, 10);
  h.tx(`${BT}.stop(false)`);
  await h.load("beeptest.app.js");
  spyMenu(h);
  h.tx('showMenu();MENU.Course.onchange(1)');
  assert.eq(h.sent(), []);
  assert(!exists(h, C), "the messages were still handled");
});

test("a phone that disconnects mid-transfer stops it", async h => {
  threeResults(h);
  await h.connect();
  await h.gb({ t: "beeptest-get", f: A });
  await h.wait(200);
  await h.disconnect();
  const n = ours(h).length;
  await h.wait(2000);
  assert.eq(ours(h).length, n);
  assert.eq(h.eval("global.SENT.length"), 0);
});

// ---------- beeptest-sync ----------

test("sync: deletes the results the phone deleted, history and log, then state", async h => {
  threeResults(h);
  await h.connect();
  await h.gb({ t: "beeptest-sync", del: [B, "beeptest.1700000000.csv"] });
  assert.eq(history(h).map(r => r.f), [A, C]);
  assert(!exists(h, B), "log deleted");
  assert(exists(h, A) && exists(h, C), "the others kept");
  const text = histJSON(h);
  assert.eq([last(h).event, last(h).hist, last(h).del], ["state", text.length + ":" + crc(text), ""],
    "a delete the phone asked for isn't reported back");
  assert.eq(h.eval('require("Storage").readJSON("beeptest.sync.json",1)'), null);
});

test("sync: never deletes the running test, nor anything but a log", async h => {
  threeResults(h);
  startAgo(h, 20, 30);
  const run = h.eval(`require("Storage").readJSON("beeptest.state.json").file`);
  await h.connect();
  await h.gb({ t: "beeptest-sync", del: [run, "beeptest.json", "beeptest.settings.json", "beeptest.state.json", "../" + A, 5, null] });
  assert(h.eval(`${BT}.active()`), "still running");
  assert(exists(h, run), "its log kept");
  assert.eq(history(h).length, 3);
  assert(h.eval('require("Storage").read("beeptest.state.json")!==undefined'));
  assert.eq(last(h).run, run);
});

test("sync: a deleted log the watch is sending ends its transfer with len -1", async h => {
  threeResults(h);
  await h.connect();
  await h.gb({ t: "beeptest-get", f: A });
  await h.wait(120);
  await h.gb({ t: "beeptest-sync", del: [A] });
  const got = await untilEnd(h, A);
  assert.eq(got[got.length - 1], { event: "logend", f: A, len: "-1" });
  const n = ours(h).length;
  await h.wait(500);
  assert.eq(ours(h).slice(n).filter(e => e.f == A), [], "nothing more of it");
});

// ---------- beeptest-get ----------

test("get: a long log arrives whole and byte-identical, in paced pieces", async h => {
  threeResults(h);
  const file = readFile(h, A);
  assert(file.length > 8000, "a long log: " + file.length);
  await h.connect();
  await h.gb({ t: "beeptest-get", f: A });
  // one piece a tick (50 ms), not all at once
  const early = ours(h).filter(e => e.event == "log").length;
  assert(early >= 1 && early <= 2, "pieces in the first 50 ms: " + early);
  await h.wait(200);
  const soon = ours(h).filter(e => e.event == "log").length;
  assert(soon >= 4 && soon <= 6, "pieces after 250 ms: " + soon);
  const got = await untilEnd(h, A);
  assert.eq(joined(got), file);
  assert.eq(got.filter(e => e.event == "log").length, Math.ceil(file.length / 1000));
});

test("get: from an offset, the rest of the log", async h => {
  threeResults(h);
  const file = readFile(h, A);
  await h.connect();
  for (const at of [2500, 1000, 7, file.length - 3]) {
    const from = ours(h).length;
    await h.gb({ t: "beeptest-get", f: A, at });
    const got = await untilEnd(h, A, from);
    assert.eq(got[0].at, String(at), "starts at " + at);
    assert.eq(joined(got, at), file.slice(at), "from " + at);
  }
});

test("get: from the end or past it, just logend with the length", async h => {
  threeResults(h);
  const len = readFile(h, B).length;
  await h.connect();
  for (const at of [len, len + 5000]) {
    const from = ours(h).length;
    await h.gb({ t: "beeptest-get", f: B, at });
    assert.eq(await untilEnd(h, B, from), [{ event: "logend", f: B, len: String(len) }], "at " + at);
  }
});

test("get: the history, from 0 and from an offset, matching state's hist", async h => {
  threeResults(h);
  await h.connect();
  await h.gb({ t: "beeptest-sync" });
  const [len, sum] = last(h).hist.split(":");
  let from = ours(h).length;
  await h.gb({ t: "beeptest-get", f: "beeptest.json" });
  const text = joined(await untilEnd(h, "beeptest.json", from));
  assert.eq([String(text.length), crc(text)], [len, sum]);
  assert.eq(JSON.parse(text).map(r => r.f), [A, B, C]);
  from = ours(h).length;
  await h.gb({ t: "beeptest-get", f: "beeptest.json", at: 100 });
  assert.eq(joined(await untilEnd(h, "beeptest.json", from), 100), text.slice(100));
});

test("get: a file that isn't there answers logend with len -1", async h => {
  await h.connect();
  for (const f of ["beeptest.1700000000.csv", "beeptest.json", "beeptest.settings.json", "beeptest.state.json", "../x", "setting.json"]) {
    const from = ours(h).length;
    await h.gb({ t: "beeptest-get", f });
    assert.eq(await untilEnd(h, f, from), [{ event: "logend", f, len: "-1" }], f);
  }
});

test("get: a new get replaces one in progress", async h => {
  threeResults(h);
  await h.connect();
  await h.gb({ t: "beeptest-get", f: A });
  await h.wait(100);
  const from = ours(h).length;
  assert(ours(h).filter(e => e.f == A).length >= 2, "A had started");
  await h.gb({ t: "beeptest-get", f: B });
  const got = await untilEnd(h, B, from);
  assert.eq(joined(got), readFile(h, B));
  await h.wait(1000);
  assert(!ours(h).slice(from).some(e => e.f == A), "nothing more of A, not even logend");
});

test("get: while a test runs answers busy, even for another file", async h => {
  threeResults(h);
  startAgo(h, 20, 30);
  const run = h.eval(`require("Storage").readJSON("beeptest.state.json").file`);
  await h.connect();
  const from = ours(h).length;
  await h.gb({ t: "beeptest-get", f: A });
  await h.gb({ t: "beeptest-get", f: run });
  await h.gb({ t: "beeptest-get", f: "beeptest.json" });
  await h.wait(1000);
  assert.eq(ours(h).slice(from), [{ event: "busy", f: A }, { event: "busy", f: run }, { event: "busy", f: "beeptest.json" }]);
});

test("get: a test started mid-transfer stops it (busy), and its cues are on time", async h => {
  threeResults(h);
  await h.connect();
  spyBuzz(h);
  await h.gb({ t: "beeptest-get", f: A });
  await h.wait(100);
  const t0 = h.eval("getTime()");
  h.tx(`${BT}.start(20)`);
  const got = await untilEnd(h, A);
  assert.eq(got[got.length - 1], { event: "busy", f: A });
  const pieces = got.filter(e => e.event == "log").length;
  await h.wait(30000);
  assert.eq(ours(h).filter(e => e.event == "log").length, pieces, "no more pieces during the test");
  // the cues: two at the start, then one per 9 s shuttle (20 m at 8 km/h)
  const buzz = h.eval("BUZZ").map(b => +(b[0] - t0).toFixed(2));
  assert.eq(buzz.slice(2, 5), [9, 18, 27], "shuttle cues at 9, 18, 27 s: " + buzz);
});

test("pacing: the watch's timers run between pieces", async h => {
  threeResults(h);
  await h.connect();
  await h.gb({ t: "beeptest-get", f: A });
  h.tx("global.FIRED=[];setTimeout(function(){FIRED.push(getTime())},120);setInterval(function(){FIRED.push(getTime())},100)");
  const t0 = h.eval("getTime()");
  await h.wait(400);
  const fired = h.eval("FIRED").map(t => Math.round((t - t0) * 1000));
  const sentBy400 = ours(h).filter(e => e.event == "log").length;
  assert(sentBy400 < 10, "the transfer isn't over yet: " + sentBy400);
  [100, 120, 200, 300].forEach((want, i) => assert.near(fired[i], want, 15, `timer ${i} (${fired})`));
});

// ---------- beeptest-set ----------

test("set: saves the course and GPS, answers state", async h => {
  await h.connect();
  await h.gb({ t: "beeptest-set", m: 15, gps: true });
  assert.eq(h.eval('require("Storage").readJSON("beeptest.settings.json")'), { m: 15, gps: true });
  assert.eq([last(h).event, last(h).m, last(h).gps], ["state", "15", "true"]);
  await h.gb({ t: "beeptest-set", gps: false });
  assert.eq(h.eval('require("Storage").readJSON("beeptest.settings.json")'), { m: 15, gps: false }, "m left alone");
  await h.gb({ t: "beeptest-set", m: 17 });
  assert.eq(h.eval('require("Storage").readJSON("beeptest.settings.json")'), { m: 15, gps: false }, "a bad m is ignored");
  assert.eq([last(h).m, last(h).gps], ["15", "false"]);
  await h.gb({ t: "beeptest-set", m: 20 });
  assert.eq(last(h).m, "20");
});

test("set: the open app shows the new course on its start screen, and starts with it", async h => {
  await h.connect();
  await h.load("beeptest.app.js");
  spyDraw(h);
  await h.gb({ t: "beeptest-set", m: 15, gps: true });
  assert(h.eval("DRAWN").includes("15m"), "redrawn: " + h.eval("DRAWN"));
  assert.eq(h.eval("[settings.m,settings.gps]"), [15, true]);
  h.tx("startCountdown()");
  await h.wait(3100);
  assert.eq(h.eval(`[${BT}.status().m,${BT}.status().gps]`), [15, true]);
});

test("set: the app's menu keeps its place, and the next start uses the new course", async h => {
  await h.connect();
  await h.load("beeptest.app.js");
  spyMenu(h);
  h.tx("showMenu()");
  spyDraw(h);
  await h.gb({ t: "beeptest-set", m: 15 });
  assert.eq(h.eval("view"), "menu", "not taken back to the start screen");
  assert(!h.eval("DRAWN").includes("Tap to start"));
  h.tx('MENU["Start test"]()');
  await h.wait(3100);
  assert.eq(h.eval(`${BT}.status().m`), 15);
});

test("set: emits a beeptest settings event", async h => {
  await h.connect();
  h.tx('global.EV=[];Bangle.on("beeptest",function(e){EV.push(e.type)})');
  await h.gb({ t: "beeptest-set", gps: true });
  assert.eq(h.eval("EV"), ["settings"]);
});

// ---------- on the watch ----------

test("changing Course or GPS speed in the app sends state", async h => {
  await h.connect();
  await h.load("beeptest.app.js");
  spyMenu(h);
  h.tx("showMenu()");
  h.tx("MENU.Course.onchange(1)");
  assert.eq([last(h).event, last(h).m], ["state", "15"]);
  h.tx('MENU["GPS speed"].onchange(true)');
  assert.eq(last(h).gps, "true");
});

/* Delete the i-th result from the app's Results menu, answering yes */
async function deleteInApp(h, i) {
  await h.load("beeptest.app.js");
  spyMenu(h);
  // past the result screen straight to its menu
  h.tx("showResult=function(r,back){back()};E.showPrompt=function(){return Promise.resolve(true)}");
  h.tx(`showEntry(${i})`);
  h.tx("MENU.Delete()");
  await h.wait(50);
}

test("deleting a result in the app records it and sends state", async h => {
  threeResults(h);
  await h.connect();
  await deleteInApp(h, 1);
  assert.eq(history(h).map(r => r.f), [A, C]);
  assert(!exists(h, B));
  assert.eq(h.eval('require("Storage").readJSON("beeptest.sync.json")'), { del: [B] });
  assert.eq([last(h).event, last(h).del], ["state", B]);
  await deleteInApp(h, 0);
  assert.eq(last(h).del, B + "," + A, "newest last");
});

test("deletions on the watch: the last 30 are kept, newest last, each once", async h => {
  await h.connect();
  const names = [];
  for (let i = 0; i < 33; i++) names.push(`beeptest.${1781000000 + i}.csv`);
  h.tx(`(function(){var s=require("beeptest.sync");${JSON.stringify(names)}.forEach(function(f){s.deleted(f)});s.deleted(${JSON.stringify(names[10])})})()`);
  const del = h.eval('require("Storage").readJSON("beeptest.sync.json").del');
  const want = names.slice(3).filter(f => f != names[10]).concat([names[10]]);
  assert.eq(del, want);
  await h.gb({ t: "beeptest-sync" });
  assert.eq(last(h).del, want.join(","));
});

test("the App Loader's Delete records the deletion too", async h => {
  threeResults(h);
  // Run interface.html's own script, with the App Loader's Util and Puck
  // talking to the emulator
  const html = require("fs").readFileSync(require("path").join(__dirname, "..", "interface.html"), "utf8");
  const script = [...html.matchAll(/<script>([^]*?)<\/script>/g)].map(m => m[1]).join("\n");
  const page = {
    document: { getElementById: () => ({ addEventListener() {}, innerHTML: "", checked: false }) },
    confirm: () => true,
    Util: {
      showModal() {}, hideModal() {},
      eraseStorageFile: (f, cb) => { h.tx(`require("Storage").open(${JSON.stringify(f)},"r").erase()`); cb(); }
    },
    Puck: { eval: (js, cb) => cb(h.eval(js)) },
    BeepLog: {}
  };
  const run = new Function(...Object.keys(page), script + "\nreturn erase;");
  const erase = run(...Object.values(page));
  erase(B);
  assert.eq(history(h).map(r => r.f), [A, C]);
  assert(!exists(h, B));
  assert.eq(h.eval('require("Storage").readJSON("beeptest.sync.json")'), { del: [B] });
});
