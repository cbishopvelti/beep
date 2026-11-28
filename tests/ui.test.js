/* The app's screens and menus (app.js): tap to start, the short press
   menus before and during a test, stop and cancel, the result screen.
   Run: node tests/run.js tests/ui.test.js -v */
const { test, assert } = require("./harness");
const { BT, timeAt, startAgo, spyMenu, menuItems, spyDraw, captureButton, press, tap, swipe } = require("./helpers");

/* Open the app with the menu and button spies in place */
async function open(h) {
  await h.load("beeptest.app.js");
  spyMenu(h);
  captureButton(h);
  h.tx("showReady()"); // set the screen up again so its button watch is captured
}

test("start screen: course and Tap to start, no button hints", async h => {
  await h.load("beeptest.app.js");
  spyDraw(h);
  h.tx("showReady()");
  assert.eq(h.eval("view"), "ready");
  const drawn = h.eval("DRAWN");
  for (const s of ["Beep Test", "20m", "Tap to start"]) assert(drawn.includes(s), `drew ${s}: ${drawn}`);
  assert(!drawn.some(s => /BTN/.test(s)), "no BTN hint: " + drawn);
});

test("tap starts a 3 s countdown, then the test", async h => {
  await open(h);
  tap(h);
  assert.eq(h.eval("view"), "countdown");
  assert(!h.eval(`${BT}.active()`), "not started yet");
  await h.wait(2900);
  assert.eq(h.eval("view"), "countdown", "still counting at 2.9 s");
  await h.wait(200);
  assert.eq(h.eval("view"), "run");
  assert.eq(h.eval(`${BT}.status().m`), 20);
});

test("button on the start screen opens the menu and never starts a test", async h => {
  await open(h);
  press(h, 150);
  assert.eq(h.eval("view"), "menu");
  assert.eq(menuItems(h), ["Start test", "Course", "GPS speed", "Results", "Exit"]);
  assert(!h.eval(`${BT}.active()`));
  assert.eq(h.eval("MENU.Course.format(MENU.Course.value)"), "20m");
  assert.eq(h.eval("MENU.Course.max"), 1, "only 20m and 15m");
  assert.eq(h.eval('MENU["GPS speed"].value'), false, "GPS off by default");
});

test("a long hold doesn't open the menu (the firmware goes to the clock)", async h => {
  await open(h);
  press(h, 1500);
  assert.eq(h.eval("view"), "ready");
});

/* Wait (up to 2.5 s) for a load() to bring the clock up */
async function atClock(h) {
  for (let i = 0; i < 50 && h.eval("global.__FILE__") === "beeptest.app.js"; i++) await h.wait(50);
  return h.eval("global.__FILE__") !== "beeptest.app.js";
}

test("swipe right on the start screen goes to the clock, without starting", async h => {
  await h.load("beeptest.app.js");
  swipe(h, -1);
  await h.wait(300);
  assert.eq(h.eval("view"), "ready", "a left swipe does nothing");
  swipe(h, 1);
  assert(await atClock(h), "at the clock");
  assert(!h.eval(`${BT}.active()`), "no test started");
});

test("swipe right during a test goes to the clock, and the test keeps running", async h => {
  startAgo(h, 20, timeAt(h, 20, 3, 2));
  await h.load("beeptest.app.js");
  assert.eq(h.eval("view"), "run");
  swipe(h, -1);
  await h.wait(300);
  assert.eq(h.eval("view"), "run", "a left swipe does nothing");
  swipe(h, 1);
  assert(await atClock(h), "at the clock");
  assert(h.eval(`${BT}.active()`), "still running");
});

test("course and GPS choices are remembered and used", async h => {
  await open(h);
  press(h, 150);
  h.tx('MENU.Course.onchange(1);MENU["GPS speed"].onchange(true)');
  assert.eq(h.eval('require("Storage").readJSON("beeptest.settings.json")'), { m: 15, gps: true });
  await h.load("beeptest.app.js");
  spyMenu(h);
  h.tx("showMenu()");
  assert.eq(h.eval("MENU.Course.format(MENU.Course.value)"), "15m");
  h.tx('MENU["Start test"]()');
  await h.wait(3100);
  assert.eq(h.eval(`[${BT}.status().m,${BT}.status().gps]`), [15, true]);
  assert.eq(h.eval("GPS_USERS"), { beeptest: 1 });
});

test("button during the countdown cancels it", async h => {
  await open(h);
  tap(h);
  await h.wait(2000);
  press(h, 150);
  assert.eq(h.eval("view"), "ready");
  await h.wait(5000);
  assert(!h.eval(`${BT}.active()`));
  assert.eq(h.eval("HRM_USERS"), {}, "HRM warm-up off again");
});

test("during a test the button opens a different menu", async h => {
  startAgo(h, 20, 30);
  await open(h); // goes straight to the running screen
  assert.eq(h.eval("view"), "run");
  press(h, 150);
  assert.eq(menuItems(h), ["Back to test", "Stop & save", "Cancel test"]);
  h.tx('MENU["Back to test"]()');
  assert.eq(h.eval("view"), "run");
  assert(h.eval(`${BT}.active()`));
});

test("Stop & save counts from the button press, not the menu choice", async h => {
  startAgo(h, 20, timeAt(h, 20, 5, 3) + 1);
  await open(h);
  press(h, 150);
  await h.wait(20000); // more than a shuttle later
  h.tx('MENU["Stop & save"]()');
  assert.eq(h.eval("view"), "result");
  const r = h.eval('require("Storage").readJSON("beeptest.json")[0]');
  assert.eq([r.l, r.s], [5, 3]);
});

test("Cancel, then No: Stop & save still counts from the first press", async h => {
  startAgo(h, 20, timeAt(h, 20, 5, 3) + 1);
  await open(h);
  h.tx("global.PROMPT=null;E.showPrompt=function(q){PROMPT=q;return Promise.resolve(false)}");
  press(h, 150);
  await h.wait(12000);
  h.tx('MENU["Cancel test"]()');
  await h.wait(100);
  assert.eq(menuItems(h), ["Back to test", "Stop & save", "Cancel test"]);
  await h.wait(12000);
  h.tx('MENU["Stop & save"]()');
  const r = h.eval('require("Storage").readJSON("beeptest.json")[0]');
  assert.eq([r.l, r.s], [5, 3]);
});

test("Cancel test asks first, then discards the run", async h => {
  startAgo(h, 20, 40);
  await open(h);
  h.tx("global.PROMPT=null;E.showPrompt=function(q){PROMPT=q;return Promise.resolve(true)}");
  press(h, 150);
  h.tx('MENU["Cancel test"]()');
  await h.wait(100);
  assert(/Cancel/.test(h.eval("PROMPT")), "asked: " + h.eval("PROMPT"));
  assert.eq(h.eval("view"), "ready");
  assert(!h.eval(`${BT}.active()`));
  assert.eq(h.eval('require("Storage").readJSON("beeptest.json",1)||[]'), []);
});

test("running screen: level, shuttle, HR, km/h, distance; GPS only when on", async h => {
  startAgo(h, 20, timeAt(h, 20, 7, 4) + 2);
  await h.load("beeptest.app.js");
  h.tx('Bangle.emit("HRM",{bpm:164,confidence:90})');
  spyDraw(h);
  await h.wait(300);
  let drawn = h.eval("DRAWN");
  for (const s of ["7", "5/10", "164", "11.5", "km/h", "1100m"]) assert(drawn.includes(s), `drew ${s}: ${drawn}`);
  assert(!drawn.some(s => /GPS|BTN/.test(s)), "no GPS row or hint: " + drawn);
  h.tx(`${BT}.cancel()`);
  h.tx(`${BT}.start(20,{gps:true})`);
  h.tx('Bangle.emit("GPS",{fix:1,speed:8.4,lat:51.5,lon:-0.1})');
  await h.load("beeptest.app.js");
  h.tx('Bangle.emit("GPS",{fix:1,speed:8.4,lat:51.5,lon:-0.1})');
  spyDraw(h);
  await h.wait(300);
  drawn = h.eval("DRAWN");
  assert(drawn.includes("GPS 8.4 km/h"), "GPS speed: " + drawn);
});

test("result screen after a test, tap goes back to the start screen", async h => {
  startAgo(h, 15, timeAt(h, 15, 4, 6) + 1);
  await open(h);
  spyDraw(h);
  press(h, 150);
  h.tx('MENU["Stop & save"]()');
  const drawn = h.eval("DRAWN");
  assert(drawn.includes("4.6"), "result: " + drawn);
  assert(drawn.some(s => /level \. shuttle +\(15m\)/.test(s)), "course: " + drawn);
  assert(!drawn.some(s => /VO2/.test(s)), "no VO2max: " + drawn);
  tap(h);
  assert.eq(h.eval("view"), "ready");
});

test("Results lists past tests", async h => {
  startAgo(h, 20, timeAt(h, 20, 3, 1) + 1);
  h.tx(`${BT}.stop(false)`);
  await open(h);
  press(h, 150);
  h.tx("MENU.Results()");
  const items = menuItems(h);
  assert.eq(items.length, 1);
  assert(/ 20m 3\.1$/.test(items[0]), items[0]);
});

test("Results: two tests the same day with the same result are both listed", async h => {
  for (const ago of [600, 300]) {
    startAgo(h, 20, ago);
    h.tx(`${BT}.stop(false,2)`); // 0.0 both times
  }
  await open(h);
  press(h, 150);
  h.tx("MENU.Results()");
  assert.eq(menuItems(h).length, 2, JSON.stringify(menuItems(h)));
});

test("the countdown has no button hint", async h => {
  await open(h);
  spyDraw(h);
  tap(h);
  await h.wait(1100);
  const drawn = h.eval("DRAWN");
  assert(drawn.includes("Get ready 20m"), "countdown: " + drawn);
  assert(!drawn.some(s => /BTN/.test(s)), "no BTN hint: " + drawn);
});

test("the watch locks as usual during a test, and redraws less when locked", async h => {
  const before = h.eval("Bangle.getOptions().lockTimeout");
  startAgo(h, 20, 30);
  await h.load("beeptest.app.js");
  assert.eq(h.eval("view"), "run");
  assert.eq(h.eval("Bangle.getOptions().lockTimeout"), before, "lock timeout left alone");
  h.tx("global.N=0;(function(o){global.drawRun=function(){N++;return o()}})(drawRun)");
  const draws = async () => {
    h.tx("showRun();N=0");
    await h.wait(4000);
    return h.eval("N");
  };
  h.tx("Bangle.setLocked(false)");
  const unlocked = await draws();
  h.tx("Bangle.setLocked(true)");
  const locked = await draws();
  assert(unlocked >= 14 && locked <= 5, `draws in 4 s: ${unlocked} unlocked, ${locked} locked`);
});
