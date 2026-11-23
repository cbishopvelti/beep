// Beep Test widget: shows level.shuttle and heart rate while a test is running
(function () {
  var timer;
  function active() { return global.beepTest && global.beepTest.active(); }
  function draw() {
    if (!active()) return;
    var s = global.beepTest.status(), x = this.x, y = this.y;
    g.reset().clearRect(x, y, x + 29, y + 23);
    g.setFont("6x8").setFontAlign(0, -1);
    g.drawString(s.level + "." + (s.shuttle + 1), x + 15, y + 3);
    // little heart
    g.setColor(g.theme.dark ? "#f44" : "#c00");
    g.fillCircle(x + 3, y + 15, 2).fillCircle(x + 7, y + 15, 2).fillPoly([x + 1, y + 16, x + 9, y + 16, x + 5, y + 20]);
    g.setColor(g.theme.fg).setFontAlign(-1, -1).drawString(s.bpm > 0 ? Math.round(s.bpm) : "--", x + 12, y + 13);
  }
  function update() {
    var on = active();
    var w = on ? 30 : 0;
    if (on && !timer) timer = setInterval(function () { WIDGETS.beeptest.draw(WIDGETS.beeptest); }, 1000);
    if (!on && timer) { clearInterval(timer); timer = undefined; }
    if (WIDGETS.beeptest.width != w) {
      WIDGETS.beeptest.width = w;
      Bangle.drawWidgets();
    }
  }
  WIDGETS.beeptest = { area: "tl", width: 0, draw: draw, update: update };
  if (active()) {
    WIDGETS.beeptest.width = 30;
    timer = setInterval(function () { WIDGETS.beeptest.draw(WIDGETS.beeptest); }, 1000);
  }
})();
