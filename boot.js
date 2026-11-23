// Resume a beep test that is in progress whenever an app (e.g. the clock) loads
if (require("Storage").read("beeptest.state.json") !== undefined) {
  global.beepTest = require("beeptest");
  global.beepTest.resume();
}
