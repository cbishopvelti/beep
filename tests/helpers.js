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

module.exports = { BT, timeAt, startAgo, spyBuzz, spyMenu, menuItems, spyDraw, captureButton, press, tap, readFile };
