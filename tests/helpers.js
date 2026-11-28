/* Helpers shared by the emulator tests */

const BT = 'require("beeptest")';

/* Seconds from the start of a test on an m metre course to the end of
   shuttle `s` of level `l` (s = 0 is the start of the level) */
function timeAt(h, m, l, s) {
  return h.eval(`(function(){var b=${BT},t=0;for(var i=1;i<${l};i++)t+=b.TESTS[${m}][i-1]*b.shuttleTime(${m},i);return t+${s}*b.shuttleTime(${m},${l});})()`);
}

/* Start a test that began `secs` seconds ago (on the watch's clock) */
function startAgo(h, m, secs, opts) {
  const gps = !!(opts && opts.gps);
  h.tx(`${BT}.start(${m},{gps:${gps},start:Date.now()-${Math.round(secs * 1000)}})`);
}

/* Record Bangle.buzz calls in BUZZ as [getTime(), length] */
function spyBuzz(h) {
  h.tx(`global.BUZZ=[];Bangle.buzz=function(l){BUZZ.push([getTime(),l]);return Promise.resolve()}`);
}

/* Record E.showMenu calls; the last menu object is MENU (null when closed) */
function spyMenu(h) {
  h.tx(`global.MENU=null;(function(o){E.showMenu=function(m){MENU=m||null;return o.apply(E,arguments)}})(E.showMenu)`);
}
function menuItems(h) {
  return h.eval(`MENU&&Object.keys(MENU).filter(function(k){return k!==""})`);
}

/* Record strings drawn to the screen in DRAWN */
function spyDraw(h) {
  h.tx(`global.DRAWN=[];(function(o){g.drawString=function(s){DRAWN.push(""+s);return o.apply(g,arguments)}})(g.drawString)`);
}

/* Capture setWatch callbacks so a test can press the button. Call before
   the screen whose button watch it needs is set up. */
function captureButton(h) {
  h.tx(`global.WATCHES=[];(function(o){global.setWatch=function(cb,pin,opt){WATCHES.push(cb);return o.apply(this,arguments)}})(setWatch)`);
}
/* Press the button for ms milliseconds (as the firmware reports it) */
function press(h, ms) {
  h.tx(`(function(w,t){w({state:true,time:t});w({state:false,time:t+${(ms || 100) / 1000},lastTime:t})})(WATCHES[WATCHES.length-1],getTime())`);
}

/* Tap the touchscreen */
function tap(h) {
  h.tx(`Bangle.emit("touch",1,{x:88,y:100,type:0})`);
}

/* A Storage file's contents, StorageFile or not */
function readFile(h, name) {
  return h.eval(`(function(){var f=require("Storage").open(${JSON.stringify(name)},"r"),s="",l;while((l=f.read(2000))!==undefined)s+=l;return s})()`);
}

/* The phone app's link (sync.js) */
const PACKAGE = "com.cbishopvelti.beep";

/* The extras of every message the watch sent the Beep Test phone app so far
   (Android Integration's own messages left out) */
function ours(h) {
  return h.sent().filter(m => m.t == "intent" && m.package == PACKAGE).map(m => m.extra);
}

/* Let time pass on the watch until a "logend" or "busy" for file f has been
   sent (at most ms), then return everything sent for f since `from`
   (an index into ours(h)). */
async function untilEnd(h, f, from, ms) {
  for (let t = 0; t < (ms || 30000); t += 100) {
    const got = ours(h).slice(from || 0).filter(e => e.f == f);
    if (got.some(e => e.event == "logend" || e.event == "busy")) return got;
    await h.wait(100);
  }
  throw new Error(`no logend for ${f} after ${ms || 30000} ms`);
}

/* Write a test's log and return its history entry, as lib.js would for a
   test on an m metre course that started at `start` (s since 1970) and
   ran `dur` seconds, with a made-up heart rate. It doesn't depend on the
   watch's clock, so it's the same in every run. With a speed progression
   `prog`, the log and entry are as an engine with the Speeds setting
   writes them (a fifth field on the start line, and p in the entry); the
   levels in them are the standard speeds' either way, so they don't depend
   on the installed engine (only the format matters to the tests). */
function makeLog(h, start, m, dur, prog) {
  const has = prog !== undefined;
  return h.eval(`(function(){var b=${BT},f=require("Storage").open("beeptest.${start}.csv","w"),` +
    `s="start,${start * 1000},-60,${m}${has ? "," + prog : ""}\\n",p,t;` +
    `for(t=0;t<=${dur};t++){p=b.position(${m},t);` +
    `s+=t.toFixed(1)+","+(100+t%40)+","+(t<5?20:95)+","+p.level+","+p.shuttle+","+(p.total*${m}).toFixed(1)+"\\n";` +
    `if(s.length>900){f.write(s);s="";}}` +
    `p=b.position(${m},${dur});var r=b.result(${m},p);` +
    `s+="end,${dur}.0,"+r.l+","+r.s+","+p.total*${m}+",139,120\\n";f.write(s);` +
    `return {t:${start * 1000},m:${m},${has ? "p:" + prog + "," : ""}l:r.l,s:r.s,d:p.total*${m},dur:${dur},max:139,avg:120,f:"beeptest.${start}.csv"}})()`);
}

module.exports = { BT, timeAt, startAgo, spyBuzz, spyMenu, menuItems, spyDraw, captureButton, press, tap, readFile,
  PACKAGE, ours, untilEnd, makeLog };
