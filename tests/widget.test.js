/* The clock widget (widget.js). Run: node tests/run.js tests/widget.test.js -v */
const { test, assert } = require("./harness");
const { BT, timeAt, startAgo, spyDraw } = require("./helpers");

test("no widget when no test is running", async h => {
  await h.load();
  assert.eq(h.eval("WIDGETS.beeptest.width"), 0);
});

test("on the clock during a test: level.shuttle and heart rate", async h => {
  startAgo(h, 20, timeAt(h, 20, 6, 2) + 1);
  await h.load();
  assert.eq(h.eval("WIDGETS.beeptest.width"), 30);
  h.tx('Bangle.emit("HRM",{bpm:152,confidence:90})');
  spyDraw(h);
  await h.wait(1100); // redraws every second
  const drawn = h.eval("DRAWN");
  assert(drawn.includes("6.3") && drawn.includes("152"), "widget text: " + drawn);
});

/* Tap at (x,y), then wait long enough for a load() to start the app */
async function tapAt(h, x, y) {
  h.tx(`Bangle.emit("touch",1,{x:${x},y:${y},type:0})`);
  await h.wait(500);
}
function widgetCentre(h) {
  return h.eval("[WIDGETS.beeptest.x+15,WIDGETS.beeptest.y+12]");
}

test("tapping the widget on the clock goes back to the test", async h => {
  startAgo(h, 20, 30);
  await h.load();
  assert(h.eval("global.__FILE__") !== "beeptest.app.js", "starts on the clock");
  const [x, y] = widgetCentre(h);
  await tapAt(h, 120, 120); // the clock face, not the widget
  assert(h.eval("global.__FILE__") !== "beeptest.app.js", "a tap elsewhere doesn't open the app");
  await tapAt(h, x, y);
  for (let i = 0; i < 50 && h.eval("global.__FILE__") !== "beeptest.app.js"; i++) await h.wait(50);
  assert.eq(h.eval("global.__FILE__"), "beeptest.app.js");
  assert(h.eval("beepTest.active()"), "the test is still running");
});

test("tapping the widget does nothing when no test is running", async h => {
  await h.load();
  await tapAt(h, 12, 12);
  assert(h.eval("global.__FILE__") !== "beeptest.app.js", "no app load");
});

test("tapping the widget inside the app doesn't reload it", async h => {
  startAgo(h, 20, 30);
  await h.load("beeptest.app.js");
  h.tx("global.STILL_HERE=1");
  const [x, y] = widgetCentre(h);
  await tapAt(h, x, y);
  assert.eq(h.eval("global.STILL_HERE"), 1);
});

test("goes away when the test ends", async h => {
  startAgo(h, 20, 20);
  await h.load();
  assert.eq(h.eval("WIDGETS.beeptest.width"), 30);
  h.tx(`beepTest.stop(false)`);
  assert.eq(h.eval("WIDGETS.beeptest.width"), 0);
  await h.load();
  assert.eq(h.eval("WIDGETS.beeptest.width"), 0);
  void BT;
});
