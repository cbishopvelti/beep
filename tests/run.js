/* Runs the emulator tests: every tests/*.test.js, or the files given.
   node tests/run.js [file.test.js ...] [-v] [--grep=part of a test name] */
const fs = require("fs");
const path = require("path");
const { run } = require("./harness");

let files = process.argv.slice(2).filter(a => a.endsWith(".js"));
if (!files.length)
  files = fs.readdirSync(__dirname).filter(f => f.endsWith(".test.js")).sort().map(f => path.join(__dirname, f));
for (const f of files) require(path.resolve(f));
run();
