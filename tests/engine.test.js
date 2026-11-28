/* The test engine (lib.js, module "beeptest"): timings, cues, results,
   resuming after an app change, cancel, HR and GPS logging.
   Run: node tests/run.js tests/engine.test.js -v */
const { test, assert } = require("./harness");
const { BT, timeAt, startAgo, spyBuzz, readFile } = require("./helpers");
const BeepLog = require("../beeplog.js");

const hist = h => h.eval('require("Storage").readJSON("beeptest.json",1)||[]');

test("tables: 8.0 then 9.0 km/h, +0.5 a level, about a minute each", async h => {
  assert.eq(h.eval(`[${BT}.speed(1),${BT}.speed(2),${BT}.speed(3),${BT}.speed(21)]`), [8, 9, 9.5, 18.5]);
  assert.eq(h.eval(`${BT}.TESTS[20]`), [7, 8, 8, 9, 9, 10, 10, 11, 11, 11, 12, 12, 13, 13, 13, 14, 14, 15, 15, 16, 16]);
  assert.eq(h.eval(`${BT}.TESTS[15]`), [9, 10, 11, 12, 12, 13, 13, 14, 14, 15, 15, 16, 17, 17, 18, 18, 19, 19, 20, 20, 21]);
  assert.eq(h.eval(`[${BT}.shuttleTime(20,1),${BT}.shuttleTime(15,1)]`), [9, 6.75]);
  for (const m of [20, 15])
    for (let l = 1; l <= 21; l++) {
      const d = timeAt(h, m, l + 1, 0) - timeAt(h, m, l, 0);
      assert(d > 58 && d < 66.5, `${m}m level ${l} lasts ${d}s`);
    }
});

test("other speed progressions: 8.0, 8.5, 9.0... and 8.5, 9.5, 10.0..., levels over a minute", async h => {
  const sp = p => h.eval(`[1,2,3,21].map(function(l){return ${BT}.speed(l,${p})})`);
  assert.eq(sp(1), [8, 8.5, 9, 18]);
  assert.eq(sp(2), [8.5, 9.5, 10, 19]);
  assert.eq(h.eval(`${BT}.shuttles(20,0)`), h.eval(`${BT}.TESTS[20]`), "standard table kept");
  for (const p of [1, 2])
    for (const m of [20, 15]) {
      assert.eq(h.eval(`${BT}.shuttles(${m},${p}).length`), 21);
      for (let l = 1; l <= 21; l++) {
        const d = timeAt(h, m, l + 1, 0, p) - timeAt(h, m, l, 0, p);
        assert(d > 60 && d < 68, `${m}m progression ${p} level ${l} lasts ${d}s`);
      }
    }
});

test("a test keeps its progression: status, resume and the saved result", async h => {
  startAgo(h, 20, timeAt(h, 20, 3, 2, 1) + 1, { p: 1 });
  let s = h.eval(`${BT}.status()`);
  assert.eq([s.level, s.shuttle, s.p, s.speed], [3, 2, 1, 9]);
  await h.load(); // the clock: the boot file resumes it
  s = h.eval(`${BT}.status()`);
  assert.eq([s.level, s.shuttle, s.p], [3, 2, 1]);
  const res = h.eval(`${BT}.stop(false)`);
  assert.eq([res.p, res.l, res.s], [1, 3, 2]);
});

test("start: state saved, HR log started, HRM on, 3 buzzes", async h => {
  spyBuzz(h);
  h.tx(`${BT}.start(20)`);
  await h.wait(1000);
  const st = h.eval('require("Storage").readJSON("beeptest.state.json",1)');
  assert.eq(st.m, 20);
  assert(/^beeptest\.\d+\.csv$/.test(st.file), st.file);
  assert.eq(h.eval("HRM_USERS"), { beeptest: 1 });
  assert.eq(h.eval("GPS_USERS"), {}, "no GPS unless asked");
  assert.eq(h.eval("BUZZ.length"), 3);
  const log = readFile(h, st.file).split("\n");
  assert(/^start,\d+,-?\d+,20,0$/.test(log[0]), log[0]);
  assert(log.length >= 3, "samples once a second");
});

test("cues: 1 buzz a shuttle, 3 at a new level, on the table's times", async h => {
  h.tx(`${BT}.start(20)`);
  spyBuzz(h);
  const t0 = h.eval(`getTime()`);
  await h.wait(64000);
  const buzz = h.eval("BUZZ").map(([t, l]) => [Math.round((t - t0) * 100) / 100, l]);
  // 6 shuttles at 9 s, then 3 buzzes when level 2 starts at 63 s
  const shuttles = buzz.filter(b => b[1] == 300).map(b => b[0]);
  assert.eq(shuttles.length, 6, JSON.stringify(buzz));
  shuttles.forEach((t, i) => assert.near(t, 9 * (i + 1), 0.05, `shuttle ${i + 1}`));
  const level = buzz.filter(b => b[1] == 250 && b[0] > 1); // not the start's
  assert.eq(level.length, 3, "three buzzes for level 2");
  assert.near(level[0][0], 63, 0.05, "level 2 at 63 s");
  assert.eq(h.eval(`${BT}.status().level`), 2);
});

test("stop: result is the last completed shuttle, saved with HR stats", async h => {
  startAgo(h, 20, timeAt(h, 20, 8, 5) + 2); // 2 s into shuttle 6 of level 8
  for (const bpm of [150, 160, 170]) { h.tx(`Bangle.emit("HRM",{bpm:${bpm},confidence:90})`); await h.wait(1000); }
  const res = h.eval(`${BT}.stop(false)`);
  assert.eq([res.m, res.l, res.s, res.d], [20, 8, 5, (7 + 8 + 8 + 9 + 9 + 10 + 10 + 5) * 20]);
  assert.eq([res.avg, res.max], [160, 170]);
  assert(!("vo2" in res), "no VO2max");
  assert.eq(hist(h)[0].f, res.f);
  assert.eq(h.eval('require("Storage").read("beeptest.state.json")===undefined'), true);
  assert.eq(h.eval("HRM_USERS"), {}, "HRM off");
  assert(/\nend,[\d.]+,8,5,1320,170,160\n$/.test(readFile(h, res.f)), "log ends with the result");
});

test("stop at a given time (when the button was pressed)", async h => {
  startAgo(h, 15, timeAt(h, 15, 3, 2) + 1);
  const at = h.eval(`${BT}.status().t`);
  await h.wait(30000); // the runner takes a while to choose Stop
  const res = h.eval(`${BT}.stop(false,${at})`);
  assert.eq([res.m, res.l, res.s, res.d], [15, 3, 2, (9 + 10 + 2) * 15]);
});

test("no shuttle completed: result 0.0; level done: previous level's last shuttle", async h => {
  h.tx(`${BT}.start(20)`);
  await h.wait(3000);
  assert.eq(h.eval(`(function(r){return [r.l,r.s]})(${BT}.stop(false))`), [0, 0]);
  startAgo(h, 20, timeAt(h, 20, 2, 0) + 1);
  assert.eq(h.eval(`(function(r){return [r.l,r.s]})(${BT}.stop(false))`), [1, 7]);
});

test("finishing level 21 ends the test with 21.16", async h => {
  startAgo(h, 20, timeAt(h, 20, 22, 0) - 3);
  await h.wait(4000);
  assert(!h.eval(`${BT}.active()`), "ended");
  assert.eq([hist(h)[0].l, hist(h)[0].s], [21, 16]);
});

test("keeps running after load(): cues, HR stats and log carry on", async h => {
  h.tx(`${BT}.start(20)`);
  h.tx(`Bangle.emit("HRM",{bpm:120,confidence:90})`);
  await h.wait(20500);
  await h.load(); // the clock, as after a long press
  assert(h.eval(`global.beepTest&&beepTest.active()`), "resumed by the boot file");
  assert.eq(h.eval("HRM_USERS"), { beeptest: 1 });
  assert(!/native/.test(h.eval("setTime.toString()")), "watching setTime again");
  spyBuzz(h);
  h.tx(`Bangle.emit("HRM",{bpm:140,confidence:90})`);
  await h.wait(20000);
  assert(h.eval("BUZZ.length") >= 2, "cues at the clock");
  await h.load("beeptest.app.js");
  const res = h.eval(`${BT}.stop(false)`);
  assert.eq(res.max, 140);
  assert(res.avg > 120 && res.avg < 140, "HR stats from before and after: " + res.avg);
  const lines = readFile(h, res.f).split("\n").filter(l => /^\d/.test(l));
  assert(lines.length >= 38 && lines.length <= 46, "a sample a second: " + lines.length);
  const ts = lines.map(l => +l.split(",")[0]);
  for (let i = 1; i < ts.length; i++) assert(ts[i] >= ts[i - 1], "in time order");
});

/* Pretend the watch was off (or crashed) for `secs`: no "kill" saved the
   state, and the saved start moves back so that much time has passed */
function switchedOff(h, secs) {
  h.tx(`E.removeAllListeners("kill");(function(s){var st=s.readJSON("beeptest.state.json");st.start-=${secs};s.writeJSON("beeptest.state.json",st)})(require("Storage"))`);
}

test("watch off mid-test: closed where the log stops, not as a full 21.16", async h => {
  startAgo(h, 20, timeAt(h, 20, 2, 3) + 1); // shuttle 4 of level 2
  await h.wait(1500);
  switchedOff(h, 2000); // long enough for the whole test to have ended
  await h.load();
  assert(!h.eval(`global.beepTest&&beepTest.active()`));
  assert.eq([hist(h)[0].l, hist(h)[0].s], [2, 3]);
});

test("watch off for a minute mid-test: closed, no buzzing on from a later level", async h => {
  startAgo(h, 20, 30);
  await h.wait(1500);
  switchedOff(h, 60);
  spyBuzz(h);
  await h.load();
  assert(!h.eval(`global.beepTest&&beepTest.active()`), "not resumed");
  assert.eq([hist(h)[0].l, hist(h)[0].s], [1, 3]);
});

test("a quick restart mid-test (no kill) carries on", async h => {
  startAgo(h, 20, 30);
  await h.wait(1500);
  switchedOff(h, 5);
  await h.load();
  assert(h.eval(`global.beepTest&&beepTest.active()`), "resumed");
  assert.eq(hist(h), []);
});

test("app changes late in a test carry on (no log reading)", async h => {
  startAgo(h, 20, 300);
  await h.load(); // saves the time on "kill"
  h.tx('(function(S,o){S.open=function(n,m){if(m=="r")global.READ=n;return o.apply(S,arguments)}})(require("Storage"),require("Storage").open)');
  await h.load();
  assert(h.eval(`global.beepTest&&beepTest.active()`));
  assert.eq(h.eval("global.READ"), null, "didn't read the log");
});

test("a full storage doesn't stop the cues or leave the test stuck", async h => {
  // every write to the log fails from the start
  h.tx('(function(S,o){S.open=function(n,m){var f=o.apply(S,arguments);if(m!="r")f.write=function(){throw new Error("full")};return f}})(require("Storage"),require("Storage").open)');
  h.tx(`${BT}.start(20)`);
  spyBuzz(h);
  await h.wait(70000);
  assert.eq(h.eval(`${BT}.status().level`), 2);
  assert(h.eval("BUZZ.length") >= 8, "cues: " + h.eval("BUZZ.length"));
  const res = h.eval(`${BT}.stop(false)`);
  assert.eq([res.l, res.s], [1, 7]);
  assert(!h.eval(`${BT}.active()`));
  assert.eq(h.eval('require("Storage").read("beeptest.state.json")===undefined'), true);
  h.errors.length = 0; // the logged "full" errors are expected
});

test("cancel: no result, log deleted, sensors off", async h => {
  h.tx(`${BT}.start(20,{gps:true})`);
  await h.wait(5000);
  const file = h.eval('require("Storage").readJSON("beeptest.state.json").file');
  h.tx(`${BT}.cancel()`);
  assert(!h.eval(`${BT}.active()`));
  assert.eq(hist(h), []);
  assert.eq(h.eval(`require("Storage").list(/^beeptest\\.\\d+\\.csv/)`), [], "log erased: " + file);
  assert.eq(h.eval('require("Storage").read("beeptest.state.json")===undefined'), true);
  assert.eq([h.eval("HRM_USERS"), h.eval("GPS_USERS")], [{}, {}]);
});

test("level boundary: an exact sample when each level starts", async h => {
  startAgo(h, 20, 60);
  await h.wait(5000);
  const f = h.eval(`${BT}.stop(false)`).f;
  const row = readFile(h, f).split("\n").find(l => l.startsWith("63.0,"));
  assert(row, "a sample at 63.0 s");
  assert.eq(row.split(",").slice(3, 6), ["2", "0", "140.0"]);
});

test("the phone setting the clock mid-test doesn't move the test", async h => {
  // The emulator's clock can't be set, so stand in for the firmware: getTime()
  // gains OFF, and setTime() changes OFF. Timers carry on unaffected, as on the watch.
  h.tx('global.OFF=0;(function(g){global.getTime=function(){return g()+OFF}})(getTime);global.setTime=function(t){OFF+=t-getTime()}');
  const fake = h.eval("setTime.toString()");
  h.tx(`${BT}.start(20)`);
  assert(h.eval("setTime.toString()") != fake, "setTime watched during the test");
  spyBuzz(h);
  await h.wait(20000);
  h.tx("setTime(getTime()+3600)"); // the watch was an hour slow
  assert.near(h.eval(`${BT}.status().t`), 20, 0.1, "elapsed time after the clock moved on");
  await h.wait(20000);
  h.tx("setTime(getTime()-1.5)");
  await h.wait(24000);
  assert.near(h.eval(`${BT}.status().t`), 64, 0.1, "elapsed time");
  assert.eq(h.eval(`${BT}.status().level`), 2);
  assert.eq(h.eval("BUZZ.filter(function(b){return b[1]==300}).length"), 6, "every shuttle cued once");
  const st = h.eval('require("Storage").readJSON("beeptest.state.json")');
  const res = h.eval(`${BT}.stop(false)`);
  assert.eq([res.l, res.s], [1, 7]);
  assert.eq(h.eval("setTime.toString()"), fake, "setTime put back");
  const log = BeepLog.parse(readFile(h, res.f));
  assert.near(log.startMs, res.t, 1, "export uses the corrected start");
  assert.near(log.startMs / 1000, st.start, 0.01);
  const ts = log.samples.map(s => s.t);
  for (let i = 1; i < ts.length; i++) assert(ts[i] - ts[i - 1] < 1.5 && ts[i] >= ts[i - 1], "no jump in the log at " + ts[i]);
});

test("GPS: logs speed and position, off again after", async h => {
  h.tx(`${BT}.start(20,{gps:true})`);
  assert.eq(h.eval("GPS_USERS"), { beeptest: 1 });
  await h.wait(2000);
  h.tx(`Bangle.emit("GPS",{fix:1,speed:8.3,lat:51.5,lon:-0.12})`);
  await h.wait(1000);
  assert.eq(h.eval(`${BT}.status().gpsSpeed`), 8.3);
  h.tx(`Bangle.emit("GPS",{fix:0,speed:NaN,lat:NaN,lon:NaN})`);
  await h.wait(1000);
  const f = h.eval(`${BT}.stop(false)`).f;
  assert.eq(h.eval("GPS_USERS"), {});
  const rows = readFile(h, f).split("\n").filter(l => /^\d/.test(l)).map(l => l.split(","));
  assert(rows.every(r => r.length == 9), "GPS columns on every row");
  assert(rows.some(r => r[6] == "8.3" && r[7] == "51.500000" && r[8] == "-0.120000"), "a fix");
  assert(rows.some(r => r[6] == "" && r[7] == ""), "no fix leaves them empty");
});
