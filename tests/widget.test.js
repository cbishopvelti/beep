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
