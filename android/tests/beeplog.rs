//! The port of `beeplog.js` gives what `beeplog.js` gives, byte for byte:
//! for each log in `tests/fixtures/beeplog/` at the repo root (`<name>.log`),
//! the TCX with and without low-confidence readings, the CSV, the result
//! text and the file name are compared with what node made of it there
//! (`make.js`, which also says how to make them again).

use beep_android::beeplog;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/beeplog")
}

fn read(name: &str) -> String {
    let path = dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// What beeplog.js made: the text of the file named, or the error it threw.
fn want(expected: &serde_json::Value) -> Result<String, String> {
    match expected {
        serde_json::Value::String(file) => Ok(read(file)),
        other => Err(other["error"]
            .as_str()
            .expect("a file name or an error")
            .to_string()),
    }
}

/// Fails at the first line that differs, so a mismatch is readable.
fn same(name: &str, what: &str, got: Result<String, String>, want: Result<String, String>) {
    match (&got, &want) {
        (Ok(got), Ok(want)) if got != want => {
            let (g, w): (Vec<_>, Vec<_>) = (got.split('\n').collect(), want.split('\n').collect());
            let i = (0..g.len().max(w.len()))
                .find(|&i| g.get(i) != w.get(i))
                .unwrap_or(0);
            panic!(
                "{name} {what}: line {} differs from beeplog.js's\n  port:       {:?}\n  beeplog.js: {:?}\n\
                 (if beeplog.js changed, run node tests/fixtures/beeplog/make.js and port the change)",
                i + 1,
                g.get(i),
                w.get(i)
            );
        }
        _ => assert_eq!(got, want, "{name} {what}"),
    }
}

#[test]
fn the_port_gives_what_beeplog_js_gives() {
    let expected: serde_json::Value = serde_json::from_str(&read("expected.json")).unwrap();
    let logs = expected.as_object().unwrap();
    assert!(logs.len() >= 10, "the fixtures are there");
    for (name, e) in logs {
        let log = beeplog::parse(&read(&format!("{name}.log")));
        assert_eq!(
            beeplog::result_text(&log),
            e["resultText"].as_str().unwrap(),
            "{name} resultText"
        );
        let base = match &e["fileBase"] {
            serde_json::Value::String(s) => Ok(s.clone()),
            other => Err(other["error"].as_str().unwrap().to_string()),
        };
        assert_eq!(beeplog::file_base(&log), base, "{name} fileBase");
        same(name, "toTCX", beeplog::to_tcx(&log, 0.0), want(&e["tcx"]));
        same(
            name,
            "toTCX minConfidence 50",
            beeplog::to_tcx(&log, 50.0),
            want(&e["tcx50"]),
        );
        same(name, "toCSV", beeplog::to_csv(&log), want(&e["csv"]));
    }
}

/// Every log in the directory is in expected.json, so none is left out
/// of the comparison by mistake.
#[test]
fn every_log_is_compared() {
    let expected: serde_json::Value = serde_json::from_str(&read("expected.json")).unwrap();
    for entry in std::fs::read_dir(dir()).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if let Some(base) = name.strip_suffix(".log") {
            assert!(
                expected.get(base).is_some(),
                "{name} isn't in expected.json: run node tests/fixtures/beeplog/make.js"
            );
        }
    }
}

/// What the result page draws from the same logs: a row per lap of the TCX,
/// and the heart rate the TCX keeps.
#[test]
fn the_rows_are_the_tcxs_laps() {
    for name in ["nogps", "gps", "clock", "lowconf"] {
        let log = beeplog::parse(&read(&format!("{name}.log")));
        let tcx = beeplog::to_tcx(&log, 50.0).unwrap();
        let rows = beeplog::level_rows(&log, 50.0);
        assert_eq!(rows.len(), tcx.matches("<Lap ").count(), "{name}");
        let levels: Vec<u32> = tcx
            .split("<Notes>Level ")
            .skip(1)
            .map(|s| s.split('<').next().unwrap().parse().unwrap())
            .collect();
        assert_eq!(
            rows.iter().map(|r| r.level).collect::<Vec<_>>(),
            levels,
            "{name}"
        );
        for row in &rows {
            assert!(row.end_s >= row.start_s, "{name}: {row:?}");
        }
        let avgs: Vec<u32> = tcx
            .split("<AverageHeartRateBpm><Value>")
            .skip(1)
            .map(|s| s.split('<').next().unwrap().parse().unwrap())
            .collect();
        let rows_avg: Vec<u32> = rows.iter().filter_map(|r| r.avg_hr).collect();
        assert_eq!(rows_avg, avgs, "{name}");
        assert_eq!(
            beeplog::hr_series(&log, 50.0).len(),
            tcx.matches("<HeartRateBpm>").count(),
            "{name}"
        );
        if name == "gps" {
            assert!(rows.iter().all(|r| r.gps_kmh.is_some()));
        } else {
            assert!(rows.iter().all(|r| r.gps_kmh.is_none()));
        }
    }
}
