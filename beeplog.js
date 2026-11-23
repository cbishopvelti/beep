// Parses beeptest.<time>.csv logs written by the Beep Test watch app and
// converts them to TCX (Garmin Training Center XML) or plain CSV.
// Used by interface.html; also loadable from Node for testing.
(function (root) {
  "use strict";

  function parse(text) {
    var out = { startMs: 0, tzOffset: 0, course: 20, samples: [], end: null };
    text.split(/\r?\n/).forEach(function (line) {
      if (!line) return;
      var p = line.split(",");
      if (p[0] == "start") {
        out.startMs = +p[1];
        out.tzOffset = +p[2] || 0;
        out.course = +p[3] || 20;
      } else if (p[0] == "end") {
        out.end = { t: +p[1], level: +p[2], shuttle: +p[3], dist: +p[4], maxHr: +p[5], avgHr: +p[6] };
      } else if (p.length >= 6 && !isNaN(+p[0])) {
        out.samples.push({ t: +p[0], bpm: +p[1], conf: +p[2], level: +p[3], shuttle: +p[4], dist: +p[5],
          gpsSpeed: num(p[6]), lat: num(p[7]), lon: num(p[8]) });
      }
    });
    // The watch keeps logging while its menu is open, then writes the last
    // sample at the moment the button was pressed: leave out what came after
    // it, and keep the samples in time order.
    if (out.end) out.samples = out.samples.filter(function (s) { return s.t <= out.end.t; });
    out.samples = out.samples.map(function (s, i) { return [s, i]; })
      .sort(function (a, b) { return a[0].t - b[0].t || a[1] - b[1]; })
      .map(function (a) { return a[0]; });
    return out;
  }

  function num(v) { return v === undefined || v === "" ? NaN : +v; }

  function iso(ms) { return new Date(Math.round(ms)).toISOString().replace(/\.\d{3}Z$/, "Z"); }

  function esc(s) { return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;"); }

  function resultText(log) {
    var name = log.course + "m beep test";
    if (!log.end) return name + " (incomplete log)";
    return name + " result " + log.end.level + "." + log.end.shuttle + ", " + log.end.dist + " m";
  }

  // opts.minConfidence: drop HR samples below this confidence (0-100)
  function toTCX(log, opts) {
    opts = opts || {};
    var minConf = opts.minConfidence || 0;
    var samples = log.samples;
    if (!samples.length) throw new Error("Log has no samples");
    var start = log.startMs;
    var id = iso(start);

    // one lap per level
    var laps = [];
    samples.forEach(function (s) {
      var lap = laps[laps.length - 1];
      if (!lap || lap.level != s.level) {
        lap = { level: s.level, pts: [] };
        laps.push(lap);
      }
      lap.pts.push(s);
    });

    var x = [];
    x.push('<?xml version="1.0" encoding="UTF-8"?>');
    x.push('<TrainingCenterDatabase xmlns="http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:ns3="http://www.garmin.com/xmlschemas/ActivityExtension/v2" xsi:schemaLocation="http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2 http://www.garmin.com/xmlschemas/TrainingCenterDatabasev2.xsd">');
    x.push(' <Activities>');
    x.push('  <Activity Sport="Running">');
    x.push('   <Id>' + id + '</Id>');
    laps.forEach(function (lap, i) {
      // Lap i runs from its first sample (logged exactly on the level beep)
      // to the first sample of the next lap, or the last sample for the final lap.
      var first = i ? lap.pts[0] : { t: 0, dist: 0 };
      var next = laps[i + 1] ? laps[i + 1].pts[0] : lap.pts[lap.pts.length - 1];
      var lapStartT = first.t;
      var lapEndT = next.t;
      var lapDist = next.dist - first.dist;
      var hrs = lap.pts.filter(function (p) { return p.bpm > 0 && p.conf >= minConf; }).map(function (p) { return p.bpm; });
      // pace is constant within a level
      var maxSpeed = lapEndT > lapStartT ? lapDist / (lapEndT - lapStartT) : 0;
      x.push('   <Lap StartTime="' + iso(start + lapStartT * 1000) + '">');
      x.push('    <TotalTimeSeconds>' + Math.max(0, lapEndT - lapStartT).toFixed(1) + '</TotalTimeSeconds>');
      x.push('    <DistanceMeters>' + Math.max(0, lapDist).toFixed(1) + '</DistanceMeters>');
      x.push('    <MaximumSpeed>' + maxSpeed.toFixed(2) + '</MaximumSpeed>');
      x.push('    <Calories>0</Calories>');
      if (hrs.length) {
        var avg = hrs.reduce(function (a, b) { return a + b; }, 0) / hrs.length;
        x.push('    <AverageHeartRateBpm><Value>' + Math.round(avg) + '</Value></AverageHeartRateBpm>');
        x.push('    <MaximumHeartRateBpm><Value>' + Math.max.apply(null, hrs) + '</Value></MaximumHeartRateBpm>');
      }
      x.push('    <Intensity>Active</Intensity>');
      x.push('    <TriggerMethod>Distance</TriggerMethod>');
      x.push('    <Track>');
      lap.pts.forEach(function (p) {
        x.push('     <Trackpoint>');
        x.push('      <Time>' + iso(start + p.t * 1000) + '</Time>');
        if (!isNaN(p.lat) && !isNaN(p.lon))
          x.push('      <Position><LatitudeDegrees>' + p.lat + '</LatitudeDegrees><LongitudeDegrees>' + p.lon + '</LongitudeDegrees></Position>');
        x.push('      <DistanceMeters>' + p.dist.toFixed(1) + '</DistanceMeters>');
        if (p.bpm > 0 && p.conf >= minConf)
          x.push('      <HeartRateBpm><Value>' + Math.round(p.bpm) + '</Value></HeartRateBpm>');
        if (!isNaN(p.gpsSpeed)) // GPS speed, m/s
          x.push('      <Extensions><ns3:TPX><ns3:Speed>' + (p.gpsSpeed / 3.6).toFixed(2) + '</ns3:Speed></ns3:TPX></Extensions>');
        x.push('     </Trackpoint>');
      });
      x.push('    </Track>');
      x.push('    <Notes>Level ' + lap.level + '</Notes>');
      x.push('   </Lap>');
    });
    x.push('   <Notes>' + esc(resultText(log)) + '</Notes>');
    x.push('   <Creator xsi:type="Device_t"><Name>Bangle.js Beep Test</Name><UnitId>0</UnitId><ProductID>0</ProductID>' +
      '<Version><VersionMajor>0</VersionMajor><VersionMinor>1</VersionMinor></Version></Creator>');
    x.push('  </Activity>');
    x.push(' </Activities>');
    x.push('</TrainingCenterDatabase>');
    return x.join("\n") + "\n";
  }

  function toCSV(log) {
    var rows = ["time,elapsed_s,heart_rate_bpm,hr_confidence,level,shuttle,distance_m,gps_speed_kmh,latitude,longitude"];
    log.samples.forEach(function (s) {
      rows.push([iso(log.startMs + s.t * 1000), s.t, s.bpm, s.conf, s.level, s.shuttle, s.dist,
        isNaN(s.gpsSpeed) ? "" : s.gpsSpeed, isNaN(s.lat) ? "" : s.lat, isNaN(s.lon) ? "" : s.lon].join(","));
    });
    return rows.join("\n") + "\n";
  }

  function fileBase(log) {
    var d = new Date(log.startMs - log.tzOffset * 60000); // local time for the name
    return "beeptest-" + log.course + "m-" + d.toISOString().substr(0, 16).replace(/[:T]/g, "-");
  }

  var api = { parse: parse, toTCX: toTCX, toCSV: toCSV, resultText: resultText, fileBase: fileBase };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else root.BeepLog = api;
})(this);
