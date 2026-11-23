// Resume a bleep test that is in progress whenever an app (e.g. the clock) loads
if (require("Storage").read("bleeptest.state.json") !== undefined) {
  global.bleepTest = require("bleeptest");
  bleepTest.resume();
}
