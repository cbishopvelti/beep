/* Install tests: metadata.json for the App Loader, and the Web IDE
   installer (npm run bundle). Run: node tests/run.js tests/install.test.js */
const fs = require("fs");
const path = require("path");
const { test, assert } = require("./harness");
const installer = require("../tools/install");
const appFiles = require("../tools/app-files");

const ROOT = path.join(__dirname, "..");
const meta = JSON.parse(fs.readFileSync(path.join(ROOT, "metadata.json")));

test("metadata.json: fields the App Loader needs, files, version", async () => {
  for (const k of ["id", "name", "shortName", "version", "description", "icon", "type", "tags", "supports", "readme", "author", "storage", "interface"])
    assert(meta[k] !== undefined, `metadata.json has ${k}`);
  assert.eq(meta.supports, ["BANGLEJS2"]);
  for (const s of meta.storage) {
    assert(fs.existsSync(path.join(ROOT, s.url)), `${s.url} exists`);
    assert(s.name == meta.id || s.name.startsWith(meta.id + "."), `${s.name} starts with the app id`);
  }
  const listed = meta.storage.map(s => s.url);
  for (const f of ["app.js", "app-icon.js", "lib.js", "boot.js", "widget.js"])
    assert(listed.includes(f), `${f} is installed`);
  for (const d of meta.data) assert((d.name || d.wildcard).startsWith(meta.id + "."), `${d.name || d.wildcard} starts with the app id`);
  for (const s of meta.screenshots) assert(fs.existsSync(path.join(ROOT, s.url)), `${s.url} exists`);
  // the App Loader shows the last ChangeLog entry as the version's notes
  const log = fs.readFileSync(path.join(ROOT, "ChangeLog"), "utf8").trim().split("\n");
  assert(log[log.length - 1].startsWith(meta.version + ":"), "ChangeLog ends with " + meta.version);
  const png = fs.readFileSync(path.join(ROOT, meta.icon));
  assert.eq(png.toString("latin1", 1, 4), "PNG");
  assert.eq([png.readUInt32BE(16), png.readUInt32BE(20)], [48, 48], "icon size");
  // the export page and the script it loads are copied with the app
  const files = appFiles.list(ROOT);
  assert(files.includes("interface.html") && files.includes("beeplog.js"), "export page files: " + files);
});

test("installed like the App Loader: launcher entry, icon, files", async h => {
  const info = h.eval('require("Storage").readJSON("beeptest.info",1)');
  assert.eq([info.id, info.name, info.src, info.icon], ["beeptest", "Beep Test", "beeptest.app.js", "beeptest.img"]);
  for (const f of info.files.split(","))
    assert(h.eval(`require("Storage").read(${JSON.stringify(f)})!==undefined`), `${f} on the watch`);
  assert.eq(h.eval('g.imageMetrics(require("Storage").read("beeptest.img"))').width, 48, "icon is a 48 px image");
});

test("the Web IDE installer replaces an old install and opens cleanly", async h => {
  h.tx('require("Storage").write("beeptest.app.js","OLD");require("Storage").erase("beeptest.info")');
  const code = installer.bundle();
  assert(!/[^\x00-\x7f]/.test(code), "ASCII only");
  assert(code.split("\n").every(l => l.length < 1200), "short lines for the Web IDE");
  for (const l of code.split("\n")) if (l && !l.startsWith("//")) h.tx(l);
  await h.wait(1500); // it reloads after 500 ms
  const f = installer.appFiles().find(f => f.name == "beeptest.app.js");
  assert.eq(h.eval('require("Storage").read("beeptest.app.js")'), f.content, "new app.js");
  assert(h.eval('require("Storage").readJSON("beeptest.info",1).version') == meta.version, "launcher entry");
  await h.load("beeptest.app.js");
  assert.eq(h.eval("view"), "ready", "app runs");
});
