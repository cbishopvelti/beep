/* The App Loader export (bleeplog.js): a log the watch wrote, as TCX and CSV.
   Run: node tests/run.js tests/export.test.js -v */
const fs = require("fs");
const path = require("path");
const { test, assert } = require("./harness");
const { BT, timeAt, startAgo, readFile } = require("./helpers");
const BleepLog = require("../bleeplog.js");

/* Tags balance and nest (enough to catch a broken TCX without an XML library) */
function wellFormed(xml) {
  const stack = [];
  for (const [, close, name, self] of xml.matchAll(/<(\/?)([A-Za-z][\w:]*)[^>]*?(\/?)>/g)) {
    if (self) continue;
    if (close) { if (stack.pop() !== name) return false; } else stack.push(name);
  }
  return stack.length === 0 && /^<\?xml /.test(xml);
}

/* Run a test from the start of level 1 to partway through shuttle 6 of level 3 */
async function runTest(h, m, opts) {
  h.tx(`${BT}.start(${m},${JSON.stringify(opts || {})})`);
  const end = timeAt(h, m, 3, 5) + 2;
  for (let t = 0; t < end; t += 5) {
    h.tx(`Bangle.emit("HRM",{bpm:${100 + t / 3},confidence:${t < 10 ? 20 : 90}})`);
    if (opts && opts.gps) h.tx(`Bangle.emit("GPS",{fix:1,speed:9.1,lat:51.5,lon:-0.12})`);
    await h.wait(5000);
  }
  const res = h.eval(`${BT}.stop(false)`);
  return { res, log: BleepLog.parse(readFile(h, res.f)) };
}

test("TCX: one lap per level, exact lap times and distances, HR", async h => {
  const { res, log } = await runTest(h, 20);
  assert.eq([log.course, log.end.level, log.end.shuttle], [20, 3, 5]);
  const tcx = BleepLog.toTCX(log);
  assert(wellFormed(tcx), "well-formed XML");
  fs.mkdirSync(path.join(__dirname, "..", "dist"), { recursive: true });
  fs.writeFileSync(path.join(__dirname, "..", "dist", "sample.tcx"), tcx);
  const laps = [...tcx.matchAll(/<TotalTimeSeconds>([\d.]+)<\/TotalTimeSeconds>\s*<DistanceMeters>([\d.]+)/g)].map(m => [+m[1], +m[2]]);
  assert.eq(laps.length, 3);
  assert.eq(laps.slice(0, 2), [[63, 140], [64, 160]], "levels 1 and 2");
  assert(laps[2][1] > 100 && laps[2][1] < 120, "level 3 so far: " + laps[2][1]);
  assert(tcx.includes(`<Id>${new Date(res.t).toISOString().replace(/\.\d+Z/, "Z")}</Id>`), "starts at the test's start");
  assert(tcx.includes("<Notes>20m bleep test result 3.5, 400 m</Notes>"));
  assert(/<HeartRateBpm><Value>1\d\d<\/Value><\/HeartRateBpm>/.test(tcx), "heart rate");
  assert(!/VO2/i.test(tcx));
  // low confidence readings can be left out
  const all = (tcx.match(/<HeartRateBpm>/g) || []).length;
  const sure = (BleepLog.toTCX(log, { minConfidence: 50 }).match(/<HeartRateBpm>/g) || []).length;
  assert(sure < all && sure > 0, `${sure} of ${all} readings kept`);
  assert(!tcx.includes("<Position>") && !tcx.includes("ns3:Speed>"), "no GPS when off");
});

test("TCX and CSV with GPS: position and speed", async h => {
  const { log } = await runTest(h, 15, { gps: true });
  const tcx = BleepLog.toTCX(log);
  assert(wellFormed(tcx));
  assert(tcx.includes("<Position><LatitudeDegrees>51.5</LatitudeDegrees><LongitudeDegrees>-0.12</LongitudeDegrees></Position>"));
  assert(tcx.includes("<ns3:Speed>2.53</ns3:Speed>"), "9.1 km/h in m/s");
  const csv = BleepLog.toCSV(log).trim().split("\n");
  assert.eq(csv[0], "time,elapsed_s,heart_rate_bpm,hr_confidence,level,shuttle,distance_m,gps_speed_kmh,latitude,longitude");
  assert(csv.slice(2).every(r => r.split(",").length == 10));
  assert(/^bleeptest-15m-\d{4}-\d\d-\d\d-\d\d-\d\d$/.test(BleepLog.fileBase(log)), BleepLog.fileBase(log));
});
