//! The port of `lib.js`'s tables (`src/tables.rs`) gives what `lib.js`
//! gives: `tests/fixtures/tables.json` at the repo root is `position` and
//! `result` at the start of every shuttle (and past the end) of each course
//! and speed progression, as node worked them out from `lib.js` itself
//! (`tests/fixtures/beeplog/make.js`, which GitHub checks is up to date).
//! An engine from before the Speeds setting has only the standard speeds,
//! so then only progression 0 is there; `src/tables.rs`'s own tests have
//! the others.

use beep_android::tables;
use std::path::Path;

#[test]
fn position_and_result_are_lib_jss_for_every_shuttle() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/tables.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    // A table's line, then a line a row. The numbers are read with
    // str::parse, which rounds correctly (serde_json's f64 can be a bit
    // off, which moves a boundary), so each is exactly lib.js's.
    let mut seen: Vec<(u32, u32, usize)> = Vec::new();
    for line in text.lines() {
        if line.starts_with("{") {
            let g: serde_json::Value = serde_json::from_str(&format!("{line}]}}")).unwrap();
            let m = g["m"].as_u64().unwrap() as u32;
            let p = g["p"].as_u64().unwrap() as u32;
            assert!(
                tables::is_progression(p),
                "progression {p}: the phone hasn't got it"
            );
            seen.push((m, p, 0));
            continue;
        }
        let Some(row) = line.strip_prefix('[').map(|l| l.trim_end_matches(',')) else {
            continue;
        };
        let Some(row) = row.strip_suffix(']') else {
            continue;
        };
        let r: Vec<f64> = row.split(',').map(|v| v.parse().unwrap()).collect();
        assert_eq!(r.len(), 9, "{line}");
        let (m, p, n) = seen.last_mut().expect("a table first");
        *n += 1;
        let (m, p) = (*m, *p);
        let t = r[0];
        let pos = tables::position(m, t, p);
        let score = tables::result(m, &pos, p);
        let got = [
            f64::from(pos.level),
            f64::from(pos.shuttle),
            f64::from(pos.total),
            pos.shuttle_start,
            pos.next_at,
            f64::from(u8::from(pos.done)),
            f64::from(score.level),
            f64::from(score.shuttle),
        ];
        assert!(
            got[..] == r[1..],
            "{m} m, progression {p}, at {t} s: the port gives {got:?}, lib.js {:?}. If \
             lib.js's tables changed, run node tests/fixtures/beeplog/make.js and port the \
             change to src/tables.rs.",
            &r[1..]
        );
    }
    for (m, p, n) in &seen {
        assert!(*n > 100, "{m} m, {p}: every shuttle ({n})");
    }
    let seen: Vec<(u32, u32)> = seen.iter().map(|(m, p, _)| (*m, *p)).collect();
    assert!(
        seen.contains(&(20, 0)) && seen.contains(&(15, 0)),
        "{seen:?}"
    );
}
