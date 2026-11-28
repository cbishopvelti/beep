//! The sync engine against a fake watch: [`FakeWatch`] is the watch's side
//! of the link as `sync.js` and `lib.js` do it (the spec's tables: `sync`,
//! `get` and `set` in, `state`, `log`, `logend` and `busy` out, a piece a
//! tick, the 30 results and 30 deletions it remembers), and [`Link`] carries
//! the lines between it and the phone, dropping, duplicating and reordering
//! them when it's lossy.
//!
//! Each seeded run throws a while of chaos at both ends (tests run, are
//! cancelled and deleted on the watch, results deleted on the phone, the
//! settings changed, the phone app killed and started again, the watch
//! disconnecting, other apps sending junk), then lets the link settle and
//! checks that the phone ended up with what the watch has: every result
//! the watch lists and the phone didn't delete, with its log byte for byte,
//! nothing deleted on either side, and the results the watch forgot kept.
//! `BEEP_SEEDS=500 cargo test --test sync` runs more of them.

use beep_android::link::{self, HISTORY, WatchEvent};
use beep_android::store::{Entry, LogState, Store, Tombstone};
use beep_android::sync::{Engine, Input, Progress, TRIES};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

/// xorshift64*: the same numbers for a seed, every run.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn chance(&mut self, p: f64) -> bool {
        ((self.next() >> 11) as f64 / (1u64 << 53) as f64) < p
    }
}

const PIECE: usize = 1000;
const SKIP: usize = 10;
const KEEP: usize = 30;

/// A transfer on the watch (`sync.js` `xfer`).
struct Xfer {
    f: String,
    at: usize,
    pos: usize,
    /// The history as it was when asked for, or the log's file.
    data: Vec<u8>,
    log: bool,
}

/// The watch, as far as the phone can tell.
struct FakeWatch {
    files: BTreeMap<String, Vec<u8>>,
    /// `beeptest.json`, newest first; `None` before the first test.
    history: Option<Vec<Entry>>,
    /// `beeptest.sync.json`'s `del`.
    del: Vec<String>,
    m: u32,
    gps: bool,
    run: Option<String>,
    xfer: Option<Xfer>,
    connected: bool,
    /// The next test's start, Unix seconds.
    clock: u64,
    /// Lines the watch printed, for the phone.
    out: Vec<String>,
    /// For the checks: results the 30 pushed out of the history, and those
    /// deleted on the watch (its menu, or the phone's sync).
    dropped: BTreeSet<String>,
    deleted: BTreeSet<String>,
}

impl FakeWatch {
    fn new() -> FakeWatch {
        FakeWatch {
            files: BTreeMap::new(),
            history: None,
            del: Vec::new(),
            m: 20,
            gps: false,
            run: None,
            xfer: None,
            connected: true,
            clock: 1_781_000_000,
            out: Vec::new(),
            dropped: BTreeSet::new(),
            deleted: BTreeSet::new(),
        }
    }

    /// `JSON.stringify` of the history, keys in `lib.js` `stop`'s order.
    fn history_json(&self) -> Option<Vec<u8>> {
        let h = self.history.as_ref()?;
        let items: Vec<String> = h
            .iter()
            .map(|e| {
                format!(
                    "{{\"t\":{},\"m\":{},\"l\":{},\"s\":{},\"d\":{},\"dur\":{},\"max\":{},\"avg\":{},\"f\":\"{}\"}}",
                    e.t, e.m, e.l, e.s, e.d, e.dur, e.max, e.avg, e.f
                )
            })
            .collect();
        Some(format!("[{}]", items.join(",")).into_bytes())
    }

    fn send(&mut self, extra: &[(&str, String)]) {
        if !self.connected {
            return;
        }
        let extra: serde_json::Map<String, serde_json::Value> = extra
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone().into()))
            .collect();
        let m = serde_json::json!({
            "t": "intent", "target": "broadcastreceiver", "action": link::WATCH_ACTION,
            "extra": extra, "package": link::PACKAGE,
        });
        self.out.push(m.to_string());
    }

    fn state(&mut self) {
        let hist = match self.history_json() {
            Some(h) => format!("{}:{}", h.len(), link::crc32(&h)),
            None => "0:0".into(),
        };
        self.send(&[
            ("event", "state".into()),
            ("v", "0.05".into()),
            ("m", self.m.to_string()),
            ("gps", self.gps.to_string()),
            ("run", self.run.clone().unwrap_or_default()),
            ("hist", hist),
            ("del", self.del.join(",")),
        ]);
    }

    fn end(&mut self, len: i64) {
        if let Some(x) = self.xfer.take() {
            self.send(&[
                ("event", "logend".into()),
                ("f", x.f),
                ("len", len.to_string()),
            ]);
        }
    }

    /// A file being sent is deleted: the phone hears it has no log.
    fn gone(&mut self, f: &str) {
        if self.xfer.as_ref().is_some_and(|x| x.f == f) {
            self.end(-1);
        }
    }

    /// A line from the phone, as the console and `sync.js` `msg` take it.
    fn line(&mut self, line: &str) {
        let json = line
            .strip_prefix("\u{10}GB(")
            .and_then(|l| l.strip_suffix(")\n"))
            .unwrap_or_else(|| panic!("not a GB() line: {line:?}"));
        let e: serde_json::Value = serde_json::from_str(json).unwrap();
        match e["t"].as_str() {
            Some("beeptest-sync") => {
                let run = self.run.clone();
                let names: Vec<String> = e["del"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|f| f.as_str())
                    .filter(|f| link::is_log_name(f) && Some(*f) != run.as_deref())
                    .map(str::to_string)
                    .collect();
                if !names.is_empty() {
                    if let Some(h) = &mut self.history {
                        h.retain(|r| !names.contains(&r.f));
                    }
                    for f in &names {
                        self.gone(f);
                        if self.files.remove(f).is_some() {
                            self.deleted.insert(f.clone());
                        }
                    }
                }
                self.state();
            }
            Some("beeptest-get") => {
                self.xfer = None;
                let f = e["f"].as_str().unwrap().to_string();
                let at = e["at"].as_f64().unwrap_or(0.0).floor().max(0.0) as usize;
                if self.run.is_some() {
                    return self.send(&[("event", "busy".into()), ("f", f)]);
                }
                if link::is_log_name(&f) && self.files.contains_key(&f) {
                    self.xfer = Some(Xfer {
                        data: self.files[&f].clone(),
                        f,
                        at,
                        pos: 0,
                        log: true,
                    });
                } else if let (true, Some(data)) = (f == HISTORY, self.history_json()) {
                    self.xfer = Some(Xfer {
                        f,
                        at,
                        pos: at,
                        data,
                        log: false,
                    });
                } else {
                    self.send(&[("event", "logend".into()), ("f", f), ("len", "-1".into())]);
                }
            }
            Some("beeptest-set") => {
                if let Some(m) = e["m"].as_u64().filter(|m| *m == 20 || *m == 15) {
                    self.m = m as u32;
                }
                if let Some(gps) = e["gps"].as_bool() {
                    self.gps = gps;
                }
                self.state();
            }
            other => panic!("the phone sent {other:?}"),
        }
    }

    /// The transfer's timer: the next piece (`sync.js` `next`).
    fn tick(&mut self) {
        let Some(x) = self.xfer.as_mut() else {
            return;
        };
        if !self.connected {
            self.xfer = None;
            return;
        }
        if self.run.is_some() {
            let f = x.f.clone();
            self.xfer = None;
            return self.send(&[("event", "busy".into()), ("f", f)]);
        }
        if x.log {
            let mut n = 0;
            while x.pos < x.at && n < SKIP {
                if x.pos >= x.data.len() {
                    let pos = x.pos as i64;
                    return self.end(pos);
                }
                x.pos = (x.pos + PIECE.min(x.at - x.pos)).min(x.data.len());
                n += 1;
            }
            if x.pos < x.at {
                return;
            }
            if x.pos >= x.data.len() {
                let pos = x.pos as i64;
                return self.end(pos);
            }
        } else if x.pos >= x.data.len() {
            let len = x.data.len() as i64;
            return self.end(len);
        }
        let piece = x.data[x.pos..(x.pos + PIECE).min(x.data.len())].to_vec();
        let (f, at) = (x.f.clone(), x.pos);
        x.pos += piece.len();
        self.send(&[
            ("event", "log".into()),
            ("f", f),
            ("at", at.to_string()),
            ("crc", link::crc32(&piece).to_string()),
            ("d", piece.iter().map(|&b| char::from(b)).collect()),
        ]);
    }

    /// A made-up log: the lines a test writes, with now and then a byte
    /// from 0x80 up (the phone keeps bytes, whatever they are).
    fn log_text(rng: &mut Rng, start: u64) -> Vec<u8> {
        let mut s = format!("start,{},-60,20\n", start * 1000).into_bytes();
        let most = if rng.chance(0.1) { 400 } else { 120 };
        let lines = rng.below(most);
        for t in 0..lines {
            s.extend(format!("{t}.0,{},95,1,0,{}.0\n", 90 + rng.below(90), t * 2).bytes());
            if rng.chance(0.02) {
                s.push(0xe9);
            }
        }
        s
    }

    fn start_test(&mut self, rng: &mut Rng) {
        if self.run.is_some() {
            return;
        }
        self.clock += 100 + rng.below(1000);
        let f = format!("beeptest.{}.csv", self.clock);
        self.files.insert(f.clone(), b"start,".to_vec());
        self.run = Some(f);
        self.state();
    }

    fn stop_test(&mut self, rng: &mut Rng) {
        let Some(f) = self.run.take() else {
            return;
        };
        let start: u64 = f[9..f.len() - 4].parse().unwrap();
        self.files.insert(f.clone(), Self::log_text(rng, start));
        let e = Entry {
            f,
            t: start as i64 * 1000 + rng.below(1000) as i64,
            m: self.m,
            l: rng.below(12) as u32,
            s: rng.below(10) as u32,
            d: rng.below(3000) as u32,
            dur: rng.below(1200) as u32,
            max: rng.below(200) as u32,
            avg: rng.below(180) as u32,
        };
        let h = self.history.get_or_insert_with(Vec::new);
        h.insert(0, e);
        while h.len() > KEEP {
            let old = h.pop().unwrap();
            self.dropped.insert(old.f);
        }
        self.state();
    }

    fn cancel_test(&mut self) {
        if let Some(f) = self.run.take() {
            self.files.remove(&f);
            self.state();
        }
    }

    /// The Results menu's Delete.
    fn delete(&mut self, rng: &mut Rng) {
        let Some(h) = self.history.as_mut().filter(|h| !h.is_empty()) else {
            return;
        };
        let e = h.remove(rng.below(h.len() as u64) as usize);
        self.files.remove(&e.f);
        self.del.retain(|d| *d != e.f);
        self.del.push(e.f.clone());
        while self.del.len() > KEEP {
            self.del.remove(0);
        }
        self.gone(&e.f);
        self.deleted.insert(e.f);
        self.state();
    }

    fn settings(&mut self, rng: &mut Rng) {
        self.m = if rng.chance(0.5) { 20 } else { 15 };
        self.gps = rng.chance(0.5);
        self.state();
    }

    fn connect(&mut self, on: bool) {
        if on == self.connected {
            return;
        }
        self.connected = on;
        if on {
            // NRF.on("connect"): state, a moment later.
            self.state();
        } else {
            self.xfer = None;
        }
    }
}

/// The lines in flight each way.
#[derive(Default)]
struct Link {
    to_watch: VecDeque<String>,
    to_phone: VecDeque<String>,
    lossy: bool,
}

impl Link {
    /// The next line from `queue`: with a lossy link, sometimes a later one
    /// first, sometimes none (lost), sometimes again later (duplicated).
    fn take(queue: &mut VecDeque<String>, lossy: bool, rng: &mut Rng) -> Option<String> {
        if queue.is_empty() {
            return None;
        }
        let i = if lossy && rng.chance(0.15) {
            rng.below(queue.len().min(4) as u64) as usize
        } else {
            0
        };
        let line = queue.remove(i)?;
        if lossy && rng.chance(0.04) {
            return None;
        }
        if lossy && rng.chance(0.04) {
            let at = rng.below(queue.len() as u64 + 1) as usize;
            queue.insert(at, line.clone());
        }
        Some(line)
    }
}

/// The phone: its store's directory, the store and the engine.
struct Phone {
    root: tempfile::TempDir,
    store: Store,
    engine: Engine,
}

impl Phone {
    fn new() -> Phone {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(&root.path().join("files")).unwrap();
        Phone {
            root,
            store,
            engine: Engine::new(),
        }
    }

    fn dir(&self) -> PathBuf {
        self.root.path().join("files")
    }

    /// The app killed and started again: what's in memory goes.
    fn restart(&mut self) {
        self.engine = Engine::new();
        self.store = Store::open(&self.dir()).unwrap();
    }

    fn input(&mut self, input: Input, now: i64, link: &mut Link) {
        let out = self.engine.handle(&self.store, input, now);
        link.to_watch.extend(out.lines);
    }

    /// A line the watch printed: read and checked as the receiver does.
    fn line(&mut self, line: &str, now: i64, link: &mut Link) {
        let extras = link::extras_from_line(line).expect("the watch's line reads");
        let event = link::parse_event(&extras)
            .unwrap_or_else(|why| panic!("the watch's line was refused ({why}): {line:.100}"));
        self.input(Input::Watch(event), now, link);
    }
}

/// What another app on the phone might broadcast: none of it may be taken,
/// or panic.
fn junk(rng: &mut Rng) -> BTreeMap<String, String> {
    let values = [
        "../x",
        "../../files/results.json",
        "beeptest.1.csv/../../x",
        "/etc/passwd",
        "-1",
        "99999999999999999999999",
        "1e9",
        "",
        "beeptest..csv",
        "\u{1f600}",
        "NaN",
        "4294967296",
        "20 ",
    ];
    let events = ["state", "log", "logend", "busy", "pong", ""];
    let mut m = BTreeMap::new();
    let event = events[rng.below(events.len() as u64) as usize];
    m.insert("event".to_string(), event.to_string());
    // A field that's always wrong, then any of the rest.
    let wrong = match event {
        "state" => ("m", "17"),
        "log" => ("d", "\u{100}"),
        "logend" => ("len", "-2"),
        "busy" => ("f", "../busy"),
        _ => ("f", "x"),
    };
    for key in link::EXTRAS.iter().skip(1) {
        if rng.chance(0.6) {
            let v = values[rng.below(values.len() as u64) as usize];
            m.insert(key.to_string(), v.to_string());
        }
    }
    m.insert(wrong.0.into(), wrong.1.into());
    if rng.chance(0.2) {
        m.insert("d".into(), "x".repeat(5000));
    }
    m
}

/// What the test watched over the run, for the checks, and how much of
/// each kind of trouble there was (so a run that tests nothing fails).
#[derive(Default)]
struct Seen {
    /// Results the phone had with their logs complete at some point.
    complete: BTreeSet<String>,
    restarts: usize,
    phone_deletes: usize,
    forged: usize,
    busy: usize,
    gave_up: usize,
}

struct World {
    rng: Rng,
    watch: FakeWatch,
    phone: Phone,
    link: Link,
    now: i64,
    seen: Seen,
}

impl World {
    fn new(seed: u64) -> World {
        let mut rng = Rng::new(seed);
        let mut watch = FakeWatch::new();
        // Results from before the phone app, some past the 30 it keeps.
        watch.connected = false;
        for _ in 0..20 + rng.below(20) {
            watch.start_test(&mut rng);
            watch.stop_test(&mut rng);
        }
        watch.connected = true;
        World {
            rng,
            watch,
            phone: Phone::new(),
            link: Link {
                lossy: true,
                ..Link::default()
            },
            now: 1_781_400_000_000,
            seen: Seen::default(),
        }
    }

    /// A watch with `n` results and a phone with none, over a link that
    /// loses nothing.
    fn with(seed: u64, n: usize) -> World {
        let mut w = World::new(seed);
        w.link.lossy = false;
        w.watch = FakeWatch::new();
        w.watch.connected = false;
        for _ in 0..n {
            let r = &mut w.rng;
            w.watch.start_test(r);
            w.watch.stop_test(r);
        }
        w.watch.connected = true;
        w
    }

    /// Moves what the watch printed onto the link.
    fn flush_watch(&mut self) {
        let out = std::mem::take(&mut self.watch.out);
        self.link.to_phone.extend(out);
    }

    fn deliver_to_watch(&mut self) -> bool {
        let Some(line) = Link::take(&mut self.link.to_watch, self.link.lossy, &mut self.rng) else {
            return false;
        };
        // Gadgetbridge drops what it can't deliver.
        if self.watch.connected {
            self.watch.line(&line);
            self.flush_watch();
        }
        true
    }

    fn deliver_to_phone(&mut self) -> bool {
        let Some(line) = Link::take(&mut self.link.to_phone, self.link.lossy, &mut self.rng) else {
            return false;
        };
        self.phone.line(&line, self.now, &mut self.link);
        true
    }

    fn phone(&mut self, input: Input) {
        self.phone.input(input, self.now, &mut self.link);
    }

    /// One step of chaos: 50 ms go by.
    fn step(&mut self, i: usize) {
        self.now += 50;
        self.watch.tick();
        self.flush_watch();
        for _ in 0..2 {
            self.deliver_to_watch();
        }
        for _ in 0..3 {
            self.deliver_to_phone();
        }
        if i % 20 == 0 {
            self.phone(Input::Tick);
        }
        let r = &mut self.rng;
        match r.below(1000) {
            0..=14 => self.watch.start_test(r),
            15..=39 => self.watch.stop_test(r),
            40 => self.watch.cancel_test(),
            41..=43 => self.watch.delete(r),
            44..=46 => self.watch.settings(r),
            47..=48 => {
                let on = !self.watch.connected;
                self.watch.connect(on);
            }
            49..=58 => self.phone(Input::SyncNow),
            59..=61 => {
                let results = self.phone.store.results();
                if !results.is_empty() {
                    let f = results[self.rng.below(results.len() as u64) as usize]
                        .entry
                        .f
                        .clone();
                    self.seen.phone_deletes += 1;
                    self.phone(Input::Delete(f));
                }
            }
            62..=64 => {
                let m = Some(if r.chance(0.5) { 20 } else { 15 });
                let gps = Some(r.chance(0.5));
                self.phone(Input::SetWatch { m, gps });
            }
            65..=68 => {
                self.seen.restarts += 1;
                self.phone.restart();
            }
            69..=78 => {
                let extras = junk(r);
                assert!(
                    link::parse_event(&extras).is_err(),
                    "junk was taken: {extras:?}"
                );
            }
            79..=86 => {
                // Forged, but well formed: a piece of whatever is being
                // fetched with a bad CRC, or in the wrong place, or a
                // logend that says there's more.
                if let Progress::Logs { f, got, .. } = self.phone.engine.progress().clone() {
                    self.seen.forged += 1;
                    let n = self.rng.below(3);
                    let event = match n {
                        0 => WatchEvent::Log(link::Piece {
                            f,
                            at: got,
                            crc: 0,
                            d: b"forged".to_vec(),
                        }),
                        1 => WatchEvent::Log(link::Piece {
                            f,
                            at: got + 1 + self.rng.below(u64::MAX / 2),
                            crc: link::crc32(b"forged"),
                            d: b"forged".to_vec(),
                        }),
                        _ => WatchEvent::LogEnd {
                            f,
                            len: Some(got + 1 + self.rng.below(link::MAX_LOG * 2)),
                        },
                    };
                    self.phone(Input::Watch(event));
                }
            }
            _ => {}
        }
        self.flush_watch();
        match self.phone.engine.progress() {
            Progress::Busy => self.seen.busy += 1,
            Progress::Done { failed, .. } if !failed.is_empty() => self.seen.gave_up += 1,
            _ => {}
        }
        if i % 25 == 0 {
            for r in self.phone.store.results() {
                if r.log == LogState::Complete {
                    self.seen.complete.insert(r.entry.f);
                }
            }
        }
    }

    /// The link comes good, the watch is connected and no test runs: the
    /// phone syncs and everything is delivered, in order, until nothing is
    /// left in flight. Returns the lines the phone sent.
    fn settle(&mut self) -> Vec<String> {
        self.link.lossy = false;
        self.watch.connect(true);
        let mut rng = Rng::new(self.now as u64);
        self.watch.stop_test(&mut rng);
        self.flush_watch();
        // Whatever was in flight arrives first.
        while self.deliver_to_watch() || self.deliver_to_phone() {
            self.flush_watch();
        }
        let mut sent = Vec::new();
        let before = self.link.to_watch.len();
        self.phone(Input::SyncNow);
        sent.extend(self.link.to_watch.iter().skip(before).cloned());
        for _ in 0..1_000_000 {
            if let Some(line) = self.link.to_watch.pop_front() {
                self.watch.line(&line);
                self.flush_watch();
            } else if let Some(line) = self.link.to_phone.pop_front() {
                let before = self.link.to_watch.len();
                self.phone.line(&line, self.now, &mut self.link);
                sent.extend(self.link.to_watch.iter().skip(before).cloned());
            } else if self.watch.xfer.is_some() {
                self.now += 50;
                self.watch.tick();
                self.flush_watch();
            } else {
                return sent;
            }
        }
        panic!("never settled");
    }

    /// The checks: the phone has what the watch has.
    fn check(&self, seed: u64) {
        let store = &self.phone.store;
        let results: BTreeMap<String, _> = store
            .results()
            .into_iter()
            .map(|r| (r.entry.f.clone(), r))
            .collect();
        let tombstones = store.tombstones();
        let listed: Vec<Entry> = self.watch.history.clone().unwrap_or_default();
        let ctx = |what: &str| format!("seed {seed}: {what}");

        // Deleted on the phone: the watch has been asked and is seen
        // without it, and it isn't back.
        assert_eq!(
            store.pending_deletes(),
            Vec::<String>::new(),
            "{}",
            ctx("pending")
        );
        for f in tombstones.keys() {
            assert!(!results.contains_key(f), "{}", ctx(&format!("{f} is back")));
            assert!(
                !listed.iter().any(|e| &e.f == f),
                "{}",
                ctx(&format!("{f} deleted, but on the watch"))
            );
        }
        // Everything the watch lists is here, the same, with its log byte
        // for byte.
        for e in &listed {
            let r = results
                .get(&e.f)
                .unwrap_or_else(|| panic!("{}", ctx(&format!("{} isn't on the phone", e.f))));
            assert_eq!(&r.entry, e, "{}", ctx("the same entry"));
            assert_eq!(
                r.log,
                LogState::Complete,
                "{}",
                ctx(&format!("{}'s log", e.f))
            );
        }
        for (f, r) in &results {
            if r.log == LogState::Complete
                && let Some(bytes) = self.watch.files.get(f)
            {
                let here = std::fs::read(store.log_path(f)).unwrap();
                assert!(here == *bytes, "{}", ctx(&format!("{f}'s log differs")));
            }
            // Nothing here the watch never had, or deleted and still says so.
            let known = listed.iter().any(|e| &e.f == f)
                || self.watch.dropped.contains(f)
                || (self.watch.deleted.contains(f) && !self.watch.del.contains(f));
            assert!(known, "{}", ctx(&format!("{f} shouldn't be here")));
        }
        for f in &self.watch.del {
            assert!(
                !results.contains_key(f),
                "{}",
                ctx(&format!("{f} deleted on the watch"))
            );
        }
        // What the watch forgot (its 30) is kept, with its log.
        for f in &self.seen.complete {
            if !tombstones.contains_key(f) && !self.watch.deleted.contains(f) {
                assert_eq!(
                    results.get(f).map(|r| r.log),
                    Some(LogState::Complete),
                    "{}",
                    ctx(&format!("{f} was kept"))
                );
            }
        }
        assert!(
            matches!(self.phone.engine.progress(), Progress::Done { failed, .. } if failed.is_empty()),
            "{}",
            ctx(&format!("{:?}", self.phone.engine.progress()))
        );
        only_the_stores_files(self.phone.root.path(), &self.phone.dir());
    }
}

/// Nothing was written outside the store's directory, and inside it only
/// the store's own files.
fn only_the_stores_files(root: &Path, dir: &Path) {
    let outside: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(outside, vec![dir.to_path_buf()]);
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(
            ["results.json", "watch.json", "logs", "health", "exports"].contains(&name.as_str()),
            "{name}"
        );
    }
    for entry in std::fs::read_dir(dir.join("logs")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let f = name.strip_suffix(".part").unwrap_or(&name);
        assert!(link::is_log_name(f), "logs/{name}");
    }
    for entry in std::fs::read_dir(dir.join("health")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(
            ["watch", "synced"].contains(&name.as_str()),
            "health/{name}"
        );
    }
}

fn seeds() -> u64 {
    std::env::var("BEEP_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40)
}

#[test]
fn the_phone_converges_to_the_watch_over_a_bad_link() {
    let mut totals = [0usize; 9];
    for seed in 0..seeds() {
        let mut w = World::new(seed);
        w.phone(Input::Opened);
        for i in 0..1500 {
            w.step(i);
        }
        w.settle();
        w.check(seed);
        // Settled: another sync changes nothing and fetches nothing.
        let before = w.phone.store.results();
        let sent = w.settle();
        assert_eq!(
            sent,
            vec![link::sync_line(&[])],
            "seed {seed}: a second sync fetched"
        );
        assert_eq!(w.phone.store.results(), before, "seed {seed}");
        let kept = before
            .iter()
            .filter(|r| w.watch.dropped.contains(&r.entry.f))
            .count();
        let tombstones = w.phone.store.tombstones();
        let by_watch = tombstones
            .keys()
            .filter(|f| w.watch.del.contains(f))
            .count();
        for (t, n) in totals.iter_mut().zip([
            before.len(),
            kept,
            tombstones.len(),
            by_watch,
            w.seen.phone_deletes,
            w.seen.restarts,
            w.seen.forged,
            w.seen.busy,
            w.seen.gave_up,
        ]) {
            *t += n;
        }
    }
    // The runs went through every kind of trouble, often.
    eprintln!(
        "results {}, kept after the watch forgot them {}, tombstones {} ({} from the watch), \
         deletes on the phone {}, restarts {}, forged events {}, steps busy {}, steps given up {}",
        totals[0],
        totals[1],
        totals[2],
        totals[3],
        totals[4],
        totals[5],
        totals[6],
        totals[7],
        totals[8]
    );
    let n = seeds() as usize;
    for (t, per_seed) in totals.iter().zip([10, 1, 2, 1, 2, 2, 2, 5, 1]) {
        assert!(*t >= per_seed * n / 2, "{totals:?}");
    }
}

#[test]
fn a_clean_link_syncs_in_one_round() {
    let mut w = World::with(7, 5);
    let sent = w.settle();
    // A sync, the history, then each log once.
    let listed = w.watch.history.clone().unwrap();
    assert_eq!(sent.len(), 2 + listed.len(), "{sent:#?}");
    assert_eq!(sent[1], link::get_line(HISTORY, 0));
    for (line, e) in sent[2..].iter().zip(&listed) {
        assert_eq!(*line, link::get_line(&e.f, 0));
    }
    w.check(7);
}

#[test]
fn results_the_watch_forgets_are_kept() {
    let mut w = World::with(1, 3);
    w.settle();
    let first: Vec<String> = w
        .watch
        .history
        .clone()
        .unwrap()
        .iter()
        .map(|e| e.f.clone())
        .collect();
    // Thirty more tests push the first three out of the watch's history.
    for _ in 0..KEEP {
        let r = &mut w.rng;
        w.watch.start_test(r);
        w.watch.stop_test(r);
    }
    w.settle();
    for f in &first {
        assert!(w.watch.dropped.contains(f));
        assert_eq!(w.phone.store.log_state(f), LogState::Complete, "{f}");
    }
    assert_eq!(w.phone.store.results().len(), 3 + KEEP);
    w.check(1);
}

#[test]
fn a_result_deleted_on_the_phone_never_comes_back() {
    let mut w = World::with(2, 3);
    w.settle();
    let kept = w.watch.history.clone().unwrap();
    let f = kept[0].f.clone();
    let file = w.watch.files[&f].clone();
    w.phone(Input::Delete(f.clone()));
    w.settle();
    assert!(!w.watch.files.contains_key(&f), "deleted on the watch too");
    assert_eq!(w.phone.store.tombstones()[&f], Tombstone::Done);
    // The watch is restored from a backup: the result and its log are back
    // there, but stay deleted here.
    w.watch.history = Some(kept);
    w.watch.files.insert(f.clone(), file);
    w.watch.state();
    w.flush_watch();
    w.settle();
    assert!(w.phone.store.result(&f).is_none());
    assert_eq!(w.phone.store.tombstones()[&f], Tombstone::Done);
}

#[test]
fn a_result_deleted_on_the_watch_goes() {
    let mut w = World::with(3, 4);
    w.settle();
    assert_eq!(w.phone.store.results().len(), 4);
    let r = &mut w.rng;
    w.watch.delete(r);
    w.flush_watch();
    let gone = w.watch.del.last().unwrap().clone();
    // The state it sends is enough.
    while w.deliver_to_phone() {}
    assert!(w.phone.store.result(&gone).is_none());
    assert!(!w.phone.store.log_path(&gone).exists());
    w.settle();
    w.check(3);
}

#[test]
fn a_sync_cut_short_by_a_restart_carries_on() {
    let mut w = World::with(4, 6);
    w.phone(Input::Opened);
    // Part of the way through the logs...
    let mut pieces = 0;
    while pieces < 4 {
        if w.deliver_to_watch() {
            continue;
        }
        if let Some(line) = w.link.to_phone.pop_front() {
            if line.contains("\"event\":\"log\"") && !line.contains(HISTORY) {
                pieces += 1;
            }
            w.phone.line(&line, w.now, &mut w.link);
        } else {
            w.now += 50;
            w.watch.tick();
            w.flush_watch();
        }
    }
    // ...the app is killed (what was in flight is lost with it).
    w.link.to_phone.clear();
    w.link.to_watch.clear();
    w.watch.xfer = None;
    let partial: Vec<_> = w
        .phone
        .store
        .results()
        .into_iter()
        .filter(|r| matches!(r.log, LogState::Partial(_)))
        .collect();
    w.phone.restart();
    let sent = w.settle();
    // No history again (it was fetched whole), and the partial log goes on
    // from where it got to.
    assert!(!sent.contains(&link::get_line(HISTORY, 0)), "{sent:#?}");
    for r in partial {
        let LogState::Partial(at) = r.log else {
            unreachable!()
        };
        assert!(sent.contains(&link::get_line(&r.entry.f, at)), "{sent:#?}");
    }
    w.check(4);
}

#[test]
fn a_watch_that_never_finishes_a_log_is_given_up_on_for_the_round() {
    let mut w = World::with(5, 2);
    // The watch's link drops every log piece.
    w.phone(Input::Opened);
    let mut gets = BTreeMap::<String, u32>::new();
    for _ in 0..100_000 {
        if let Some(line) = w.link.to_watch.pop_front() {
            if let Some(f) = line
                .split("\"f\":\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
            {
                *gets.entry(f.to_string()).or_default() += 1;
            }
            w.watch.line(&line);
            w.flush_watch();
        } else if let Some(line) = w.link.to_phone.pop_front() {
            if line.contains("\"event\":\"log\"") && !line.contains(HISTORY) {
                continue;
            }
            w.phone.line(&line, w.now, &mut w.link);
        } else if w.watch.xfer.is_some() {
            w.now += 50;
            w.watch.tick();
            w.flush_watch();
        } else {
            break;
        }
    }
    for e in w.watch.history.as_ref().unwrap() {
        assert_eq!(gets.get(&e.f), Some(&(1 + TRIES)), "{}: {gets:?}", e.f);
    }
    assert!(
        matches!(w.phone.engine.progress(), Progress::Done { failed, .. } if failed.len() == 2)
    );
    // A good link next round fetches them.
    w.settle();
    w.check(5);
}
