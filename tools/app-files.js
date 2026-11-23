/* The files that make up the app in a BangleApps checkout (apps/beeptest/):
   metadata.json, ChangeLog, the icon, README, screenshots, every storage
   file, and the App Loader page (interface.html) with the script it loads. Tests, tools and docs stay in this repository.

     node tools/app-files.js <dest dir> [src dir]   copies them into dest
     require("./app-files").list(src)                the relative paths */
const fs = require("fs");
const path = require("path");

function list(src) {
  const m = JSON.parse(fs.readFileSync(path.join(src, "metadata.json"), "utf8"));
  const files = new Set(["metadata.json", "ChangeLog", m.icon, m.readme]);
  for (const s of m.screenshots || []) files.add(s.url);
  for (const s of m.storage) if (s.url) files.add(s.url);
  if (m.interface) {
    files.add(m.interface);
    // local scripts the page loads, e.g. <script src="beeplog.js">
    const html = fs.readFileSync(path.join(src, m.interface), "utf8");
    for (const [, f] of html.matchAll(/<script src="([^":/]+)"/g)) files.add(f);
  }
  return [...files].filter(f => f && fs.existsSync(path.join(src, f)));
}

function copy(src, dst) {
  for (const f of list(src)) {
    fs.mkdirSync(path.dirname(path.join(dst, f)), { recursive: true });
    fs.copyFileSync(path.join(src, f), path.join(dst, f));
  }
}

module.exports = { list, copy };

if (require.main === module) {
  const [dst, src] = process.argv.slice(2);
  if (!dst) { console.error("usage: node tools/app-files.js <dest dir> [src dir]"); process.exit(2); }
  copy(path.resolve(src || path.join(__dirname, "..")), path.resolve(dst));
}
