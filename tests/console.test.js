/* Gadgetbridge reads every line the watch prints to its console as a possible
   message, and a line that starts with "{" but isn't JSON pops up "Malformed
   JSON from Bangle.js" on the phone. Android Integration prints anything GB()
   gets that it has no handler for ("GB Unknown", the object pretty-printed over
   many lines, some of them starting with "{"), so Beep Test's own messages
   must not reach it, and the sync must print nothing itself. Other messages
   still go to Android Integration. Run: node tests/run.js tests/console.test.js */
const { test, assert } = require("./harness");
const { ours, untilEnd, makeLog } = require("./helpers");

const F = "beeptest.1781344800.csv";
const MESSAGES = [
  { t: "beeptest-sync", del: ["beeptest.1700000000.csv"] },
  { t: "beeptest-get", f: "beeptest.json" },
  { t: "beeptest-get", f: F, at: 1500 },
  { t: "beeptest-get", f: "beeptest.1700000000.csv" },
  { t: "beeptest-set", m: 15, gps: true },
  { t: "beeptest-something-newer" }
];

/* Every message from the phone, letting the transfers finish. What was
   printed is in h.log, from the first message on. */
async function exchange(h) {
  h.tx(`require("Storage").writeJSON("beeptest.json",[${JSON.stringify(makeLog(h, 1781344800, 20, 200))}])`);
  await h.connect();
  // what Android Integration's GB() is given, and its "GB" events
  h.tx('global.GBS=[];Bangle.on("GB",function(e){GBS.push(e.t)})');
  h.log.length = 0;
  for (const m of MESSAGES) {
    const from = ours(h).length;
    await h.gb(m);
    if (m.t == "beeptest-get") await untilEnd(h, m.f, from);
  }
  await h.wait(500);
}

/* What Gadgetbridge would try to parse as JSON: console lines starting
   with "{" (the harness's own h.eval answers are between \x01 and \x02) */
const jsonish = h => h.log.join("\n").replace(/\x01[^]*?\x02/g, "").split("\n").filter(l => l.startsWith("{"));

test("the phone's messages print nothing on the watch's console, and none reach Android Integration", async h => {
  await exchange(h);
  assert.eq(h.eval("GBS"), [], "Android Integration's GB() saw none of them");
  assert(!h.log.some(l => /GB Unknown/.test(l)), "no 'GB Unknown': " + JSON.stringify(h.log));
  assert.eq(jsonish(h), []);
  assert.eq(h.errors, []);
  const events = ours(h).map(e => e.event);
  for (const e of ["state", "log", "logend"]) assert(events.includes(e), "answered with " + e + ": " + events);
});

test("the same inside the app", async h => {
  await h.load("beeptest.app.js");
  await exchange(h);
  assert.eq(h.eval("GBS"), []);
  assert.eq(jsonish(h), []);
  assert.eq(h.eval("view"), "ready");
});

test("other GB messages still reach Android Integration", async h => {
  await h.connect();
  h.tx('global.GBS=[];Bangle.on("GB",function(e){GBS.push(e.t)})');
  h.log.length = 0;
  await h.gb({ t: "notify", id: 7, src: "Gadgetbridge", title: "Hello", body: "there" });
  assert.eq(h.eval("GBS.length"), 1, "Android Integration's GB() got it");
  assert.eq(h.eval("global.MESSAGES[0].title"), "Hello", "the notification arrived");
  // a type nobody handles still goes to Android Integration, which says so
  await h.gb({ t: "nobody-handles-this" });
  assert(h.log.some(l => /GB Unknown/.test(l)), "Android Integration still gets the rest: " + JSON.stringify(h.log));
  // and a message without t goes on to it too
  h.tx("GB({})");
  await h.wait(50);
  assert.eq(h.eval("GBS.length"), 3);
  assert.eq(ours(h), []);
});

test("connecting prints nothing either (state is sent as a message)", async h => {
  h.log.length = 0;
  await h.connect();
  await h.wait(4000);
  assert.eq(ours(h).map(e => e.event), ["state"]);
  assert.eq(jsonish(h), []);
  assert(!h.log.some(l => /GB Unknown/.test(l)));
});
