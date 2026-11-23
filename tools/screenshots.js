/* Takes the app store screenshots in the headless emulator (see
   tests/README.md for its setup) and writes them to screenshots/.

     node tools/screenshots.js          (or: npm run screenshots) */
const fs = require("fs");
const path = require("path");
const zlib = require("zlib");
const { create } = require("../tests/harness");
const { BT, timeAt, startAgo, spyMenu, captureButton, press } = require("../tests/helpers");

const OUT = path.join(__dirname, "..", "screenshots");

/* A plain RGBA PNG (no dependencies) */
const CRC = new Int32Array(256).map((_, n) => {
  for (let k = 0; k < 8; k++) n = n & 1 ? 0xedb88320 ^ (n >>> 1) : n >>> 1;
  return n;
});
function crc32(buf) {
  let c = -1;
  for (const b of buf) c = CRC[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4), crc = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, "latin1"), data]);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}
function png(rgba, w, hgt) {
  const raw = Buffer.alloc((w * 4 + 1) * hgt);
  for (let y = 0; y < hgt; y++)
    Buffer.from(rgba.buffer, y * w * 4, w * 4).copy(raw, y * (w * 4 + 1) + 1);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(hgt, 4);
  ihdr[8] = 8; ihdr[9] = 6; // 8 bit RGBA
  return Buffer.concat([Buffer.from("\x89PNG\r\n\x1a\n", "latin1"), chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(raw, { level: 9 })), chunk("IEND", Buffer.alloc(0))]);
}

async function main() {
  const h = await create();
  const shot = name => {
    const px = h.emu.getScreenshot();
    for (let i = 0; i < px.length; i++) px[i] |= 0xff000000; // opaque
    fs.writeFileSync(path.join(OUT, name), png(new Uint8Array(px.buffer), 176, 176));
    console.log("screenshots/" + name);
  };
  const hrm = bpm => h.tx(`Bangle.emit("HRM",{bpm:${bpm},confidence:90})`);
  fs.mkdirSync(OUT, { recursive: true });

  // The start screen, with a previous result
  await h.reset();
  startAgo(h, 20, timeAt(h, 20, 9, 4) + 1);
  h.tx(`${BT}.stop(false)`);
  await h.load("bleeptest.app.js");
  await h.wait(500);
  shot("1-start.png");

  // The menu (short press)
  spyMenu(h);
  captureButton(h);
  h.tx("showReady()");
  press(h, 150);
  await h.wait(500);
  shot("2-menu.png");

  // Running, level 7
  await h.reset();
  startAgo(h, 20, timeAt(h, 20, 7, 7) + 3.5);
  await h.load("bleeptest.app.js");
  hrm(164);
  await h.wait(600);
  shot("3-running.png");

  // The menu during a test
  spyMenu(h);
  captureButton(h);
  h.tx("showRun()");
  press(h, 150);
  await h.wait(500);
  shot("4-test-menu.png");

  // The clock with the widget
  await h.load();
  hrm(166);
  await h.wait(1100);
  shot("5-clock-widget.png");

  // The result
  await h.load("bleeptest.app.js");
  for (const bpm of [150, 160, 171]) { hrm(bpm); await h.wait(1000); }
  h.tx(`showResult(${BT}.stop(false),showReady)`);
  await h.wait(500);
  shot("6-result.png");

  if (h.errors.length) throw new Error(h.errors.join(" | "));
  h.close();
  process.exit(0);
}

main().catch(e => { console.error(e); process.exit(1); });
