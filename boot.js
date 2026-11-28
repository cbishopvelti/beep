// Resume a beep test that is in progress whenever an app (e.g. the clock) loads
if (require("Storage").read("beeptest.state.json") !== undefined) {
  global.beepTest = require("beeptest");
  global.beepTest.resume();
}
// The Beep Test phone app's messages come through Gadgetbridge as GB() calls.
// Take those whose t starts with "beeptest-" before Android Integration sees
// them (boot files run in name order, so "android" comes first): it
// console.logs a type it has no handler for, and Gadgetbridge then pops up
// "Malformed JSON". Everything else goes on to it. The sync module
// (sync.js) is loaded only when it is needed.
(function () {
  var gb = global.GB;
  global.GB = function (e) {
    if (e && typeof e.t == "string" && e.t.indexOf("beeptest-") == 0) return require("beeptest.sync").msg(e);
    if (gb) return gb(e);
  };
  // after Android Integration's own hello (2 s)
  NRF.on("connect", function () {
    setTimeout(function () { require("beeptest.sync").state(); }, 3000);
  });
})();
