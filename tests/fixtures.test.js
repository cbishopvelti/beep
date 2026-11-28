/* The files shared with the phone app's tests (android/), as blink's
   phone-line.txt: the Rust tests read the same files.

   tests/fixtures/watch-lines.txt is what the watch prints for the scripted
   exchange below, one message a line (each is printed after an empty line,
   left out here; Android Integration's own messages are left out too). The
   phone app's tests feed it to its sync engine and check what ends up
   stored. This test fails when the watch no longer prints exactly that;
   `BEEP_BLESS=1 node tests/run.js tests/fixtures.test.js` writes it again
   (then run the phone app's tests too). The version in its state lines is
   metadata.json's, so a new version needs that as well.

   tests/fixtures/phone-lines.txt is lines exactly as the phone app builds
   them, written by the phone app's tests: the watch must read each one.

   Run: node tests/run.js tests/fixtures.test.js */
const fs = require("fs");
const path = require("path");
const { test, skip, assert } = require("./harness");
const { PACKAGE, ours, untilEnd, makeLog } = require("./helpers");

const WATCH_LINES = path.join(__dirname, "fixtures", "watch-lines.txt");
const PHONE_LINES = path.join(__dirname, "fixtures", "phone-lines.txt");
const A = "beeptest.1781344800.csv", B = "beeptest.1781258400.csv", C = "beeptest.1781172000.csv";
const D = "beeptest.1781085600.csv";
/* The n of the phone's sync in the exchange: the phone's tests
   (android/tests/fixtures.rs N) run the exchange's lines through its sync
   engine at a set time, and this is the n it gives the sync it sends in
   answer to the first state. */
const N = "1781400000050";

/* The watch for both: three past tests, newest first (A, a 20 m test of
   400 s; B, 15 m, 150 s, on speed progression 1, as an engine with the
   Speeds setting writes it; C, 20 m, 30 s), and D deleted on the watch
   earlier. A and C are as an engine from before the Speeds setting writes
   them (no p), so the phone reads both. Nothing in it depends on the
   watch's clock. */
function setUp(h) {
  const hist = [makeLog(h, 1781344800, 20, 400), makeLog(h, 1781258400, 15, 150, 1), makeLog(h, 1781172000, 20, 30)];
  h.tx(`require("Storage").writeJSON("beeptest.json",${JSON.stringify(hist)});require("beeptest.sync").deleted(${JSON.stringify(D)})`);
}

/* The exchange: the phone connects (state, unasked); a sync deleting C,
   with the n the phone gives it (state, echoing it); the history fetched; A fetched whole; B from 0, then, after two
   pieces, from 2000 (as after the link dropped: the new get replaces the
   old); the course, GPS and speeds set (state: with an engine that has no
   speed progressions, as before the Speeds setting, the watch ignores p
   and its states have none). */
async function exchange(h) {
  setUp(h);
  await h.connect();
  await h.wait(3500);
  await h.gb({ t: "beeptest-sync", del: [C], n: N });
  let from = ours(h).length;
  await h.gb({ t: "beeptest-get", f: "beeptest.json", at: 0 });
  await untilEnd(h, "beeptest.json", from);
  from = ours(h).length;
  await h.gb({ t: "beeptest-get", f: A, at: 0 });
  await untilEnd(h, A, from);
  from = ours(h).length;
  await h.gb({ t: "beeptest-get", f: B, at: 0 });
  while (ours(h).slice(from).length < 2) await h.wait(10);
  from = ours(h).length;
  await h.gb({ t: "beeptest-get", f: B, at: 2000 });
  await untilEnd(h, B, from);
  await h.gb({ t: "beeptest-set", m: 15, gps: true, p: 1 });
  return h.lines().filter(l => JSON.parse(l).package == PACKAGE);
}

test("tests/fixtures/watch-lines.txt is what the watch prints for the scripted exchange", async h => {
  const lines = await exchange(h);
  const ev = lines.map(l => JSON.parse(l).extra);
  // what the exchange is meant to show, so a change to it is noticed
  assert.eq(ev.filter(e => e.event == "state").length, 3, "three states");
  assert.eq(ev[1].del, D, "the deletion on the watch");
  assert(!ev[1].hist.startsWith("0:"), "a history");
  assert.eq(ev.filter(e => e.event == "logend").map(e => e.f), ["beeptest.json", A, B]);
  assert.eq(ev.filter(e => e.f == B && e.event == "log").map(e => e.at).slice(0, 3), ["0", "1000", "2000"]);
  assert.eq([ev[ev.length - 1].m, ev[ev.length - 1].gps], ["15", "true"]);
  const states = ev.filter(e => e.event == "state");
  if (h.eval('!!require("beeptest").PROGS'))
    assert.eq(states.map(e => e.p), ["0", "0", "1"], "the speeds, and the one set");
  else assert(states.every(e => !("p" in e)), "no speeds without progressions");
  assert.eq(states.map(e => e.n), ["", N, ""], "only the sync's answer echoes its n");
  assert(lines.every(l => /^[\x20-\x7e]*$/.test(l)), "ASCII, one message a line");

  const text = lines.join("\n") + "\n";
  if (process.env.BEEP_BLESS) {
    fs.mkdirSync(path.dirname(WATCH_LINES), { recursive: true });
    fs.writeFileSync(WATCH_LINES, text);
    console.log(`       wrote ${path.relative(process.cwd(), WATCH_LINES)} (${lines.length} lines)`);
    return;
  }
  const want = fs.existsSync(WATCH_LINES) ? fs.readFileSync(WATCH_LINES, "latin1") : "";
  if (text != want) {
    const old = want.split("\n");
    const i = lines.findIndex((l, n) => l != old[n]);
    throw new Error(`tests/fixtures/watch-lines.txt is out of date (line ${i + 1} differs: ` +
      `now ${JSON.stringify((lines[i] || "").slice(0, 120))}, file has ${JSON.stringify((old[i] || "").slice(0, 120))}). ` +
      "If the change is meant, run BEEP_BLESS=1 node tests/run.js tests/fixtures.test.js, then the phone app's tests (they read it).");
  }
});

test("tests/fixtures/phone-lines.txt: the watch reads every line the phone app builds", async h => {
  if (!fs.existsSync(PHONE_LINES)) skip("no tests/fixtures/phone-lines.txt yet (the phone app's tests write it)");
  const lines = fs.readFileSync(PHONE_LINES, "latin1").split("\n").filter(l => l);
  assert(lines.length > 0, "some lines");
  setUp(h);
  await h.connect();
  await h.wait(3500);
  h.log.length = 0;
  for (const line of lines) {
    assert(/^\x10GB\(.*\)$/.test(line), "a GB() line: " + JSON.stringify(line));
    assert(/^[\x20-\x7e]*$/.test(line.slice(1)), "ASCII only, as Gadgetbridge sends ISO-8859-1: " + JSON.stringify(line));
    const m = JSON.parse(line.slice(4, -1));
    assert(/^beeptest-/.test(m.t), "a Beep Test message: " + m.t);
    const from = ours(h).length;
    h.emu.tx(line + "\n");
    await h.wait(50);
    if (m.t == "beeptest-get") await untilEnd(h, String(m.f), from);
    const got = ours(h).slice(from).map(e => e.event);
    const want = m.t == "beeptest-get" ? ["log", "logend", "busy"] : ["state"];
    assert(got.length && got.every(e => want.includes(e)), `${m.t} answered with ${want}: ${got}`);
    if (m.t != "beeptest-get") assert.eq(ours(h).slice(from)[0].n, m.n || "", `${m.t}'s state echoes its n`);
  }
  assert(!h.log.some(l => /GB Unknown/.test(l)), "none reached Android Integration");
  assert.eq(h.errors, []);
});
