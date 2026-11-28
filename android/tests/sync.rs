//! The sync engine against a fake watch: [`FakeWatch`] is the watch's side
//! of the link as `sync.js` and `lib.js` do it (the spec's tables: `sync`,
//! `get` and `set` in, `state`, `log`, `logend` and `busy` out, a piece a
//! tick, the 30 results and 30 deletions it remembers), and [`Link`] carries
//! the lines between it and the phone, dropping, duplicating and reordering
//! them when it's lossy.
//!
//! Half the seeds' watches (the even ones) have speed progressions (the Speeds setting:
//! `state` says `p`, `set` takes it, results carry it) and half are from
//! before it (no `p` anywhere, and a `p` set from the phone ignored). A
//! third of them (seeds 2, 5, 8...) run a `sync.js` from before `n`: their
//! `state` never has one, so the phone falls back to taking a `state` soon
//! after its sync as the answer.
//!
//! Each seeded run throws a while of chaos at both ends (tests run, are
//! cancelled and deleted on the watch, results deleted on the phone, the
//! settings changed, the phone app killed and started again, the watch
//! disconnecting or going back to the clock mid-transfer, a bad patch where
//! its log pieces are lost, other apps sending junk), then lets the link
//! settle and
//! checks that the phone ended up with what the watch has: every result
//! the watch lists and the phone didn't delete, with its log byte for byte,
//! nothing deleted on either side, the results the watch forgot kept, and
//! the watch's settings as it has them.
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
/// The step of each run's bad patch ([`World::step`]).
const BAD_PATCH: usize = 700;
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
    /// Whether its engine has speed progressions (`lib.js` `PROGS`), and
    /// the one it's set to.
    progs: bool,
    p: u32,
    /// Whether its `sync.js` echoes a sync's `n` in `state` (and says
    /// `n` `""` in every other); one from before `n` has none.
    echo: bool,
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
    fn new(progs: bool, echo: bool) -> FakeWatch {
        FakeWatch {
            echo,
            files: BTreeMap::new(),
            history: None,
            del: Vec::new(),
            m: 20,
            gps: false,
            progs,
            p: 0,
            run: None,
            xfer: None,
            connected: true,
            clock: 1_781_000_000,
            out: Vec::new(),
            dropped: BTreeSet::new(),
            deleted: BTreeSet::new(),
        }
    }

    /// `JSON.stringify` of the history, keys in `lib.js` `stop`'s order
    /// (`p` after `m` with progressions, none without).
    fn history_json(&self) -> Option<Vec<u8>> {
        let h = self.history.as_ref()?;
        let items: Vec<String> = h
            .iter()
            .map(|e| {
                let p = if self.progs {
                    format!(",\"p\":{}", e.p)
                } else {
                    String::new()
                };
                format!(
                    "{{\"t\":{},\"m\":{}{p},\"l\":{},\"s\":{},\"d\":{},\"dur\":{},\"max\":{},\"avg\":{},\"f\":\"{}\"}}",
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
        self.answer("");
    }

    /// `state`, answering a sync whose `n` was `n` (`""`: none).
    fn answer(&mut self, n: &str) {
        let hist = match self.history_json() {
            Some(h) => format!("{}:{}", h.len(), link::crc32(&h)),
            None => "0:0".into(),
        };
        let mut extra = vec![
            ("event", "state".into()),
            ("v", "0.05".into()),
            ("m", self.m.to_string()),
            ("gps", self.gps.to_string()),
            ("run", self.run.clone().unwrap_or_default()),
            ("hist", hist),
            ("del", self.del.join(",")),
        ];
        if self.echo {
            extra.push(("n", n.to_string()));
        }
        if self.progs {
            extra.push(("p", self.p.to_string()));
        }
        self.send(&extra);
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
                // sync.js echoes an n of digits only.
                let n = e["n"]
                    .as_str()
                    .filter(|n| n.len() <= 20 && n.bytes().all(|b| b.is_ascii_digit()))
                    .unwrap_or("")
                    .to_string();
                self.answer(&n);
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
                // `P[e.p]`: a whole number below 3 (a number, not "1").
                if let Some(p) = e["p"].as_u64().filter(|p| self.progs && *p < 3) {
                    self.p = p as u32;
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
            p: self.p,
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
        if self.progs {
            self.p = rng.below(3) as u32;
        }
        self.state();
    }

    /// A `load()` (back to the clock, say): the transfer's timer and
    /// `sync.js` go, and if a transfer was cut off the boot file says
    /// `state` once the next app is up.
    fn load(&mut self) {
        if self.xfer.take().is_some() {
            self.state();
        }
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
    /// Files given up on, counted once a round (by the round's end).
    gave_up: usize,
    gave_up_at: Option<i64>,
    /// Transfers a load() on the watch cut off.
    loads: usize,
}

struct World {
    rng: Rng,
    watch: FakeWatch,
    phone: Phone,
    link: Link,
    now: i64,
    seen: Seen,
    /// Until this step, the log pieces the watch sends are lost (a bad
    /// patch of its radio), so transfers come up short and are given up
    /// on.
    lossy_logs_until: usize,
    lossy_logs: bool,
}

impl World {
    fn new(seed: u64) -> World {
        let mut rng = Rng::new(seed);
        let mut watch = FakeWatch::new(seed.is_multiple_of(2), seed % 3 != 2);
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
            lossy_logs_until: 0,
            lossy_logs: false,
        }
    }

    /// A watch with `n` results and a phone with none, over a link that
    /// loses nothing.
    fn with(seed: u64, n: usize) -> World {
        let mut w = World::new(seed);
        w.link.lossy = false;
        w.watch = FakeWatch::new(seed.is_multiple_of(2), true);
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
        if self.lossy_logs && line.contains("\"event\":\"log\"") && !line.contains(HISTORY) {
            return true;
        }
        self.phone.line(&line, self.now, &mut self.link);
        true
    }

    fn phone(&mut self, input: Input) {
        self.phone.input(input, self.now, &mut self.link);
    }

    /// One step of chaos: 50 ms go by.
    fn step(&mut self, i: usize) {
        self.now += 50;
        if i == BAD_PATCH {
            // A test ends with the watch connected and the phone asking,
            // and then its log's pieces are lost for a while.
            self.watch.connect(true);
            let r = &mut self.rng;
            self.watch.start_test(r);
            self.watch.stop_test(r);
            self.phone(Input::SyncNow);
            self.lossy_logs_until = i + 200;
        }
        self.lossy_logs = i < self.lossy_logs_until;
        self.watch.tick();
        self.flush_watch();
        for _ in 0..2 {
            self.deliver_to_watch();
        }
        for _ in 0..3 {
            self.deliver_to_phone();
        }
        if i.is_multiple_of(20) {
            self.phone(Input::Tick);
        }
        let r = &mut self.rng;
        let roll = r.below(1000);
        // In the bad patch nothing else happens (no tests, no changes, no
        // syncs asked for, no restarts, each of which starts a round
        // afresh), so the transfers it spoils are given up on.
        let roll = if self.lossy_logs { 999 } else { roll };
        match roll {
            0..=14 => self.watch.start_test(r),
            15..=39 => self.watch.stop_test(r),
            40 => self.watch.cancel_test(),
            41..=43 => self.watch.delete(r),
            44..=46 => self.watch.settings(r),
            47..=48 => {
                let on = !self.watch.connected;
                self.watch.connect(on);
            }
            87..=90 => {
                if self.watch.xfer.is_some() {
                    self.seen.loads += 1;
                }
                self.watch.load();
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
                // The Watch page offers speeds only when the watch has
                // them, but a set with p goes to an older watch as well.
                let p = r.chance(0.5).then(|| r.below(3) as u32);
                self.phone(Input::SetWatch { m, gps, p });
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
            Progress::Done { at_ms, failed }
                if !failed.is_empty() && self.seen.gave_up_at != Some(*at_ms) =>
            {
                self.seen.gave_up += failed.len();
                self.seen.gave_up_at = Some(*at_ms);
            }
            _ => {}
        }
        if i.is_multiple_of(25) {
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
        self.lossy_logs = false;
        self.lossy_logs_until = 0;
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
            // Its log went from the watch too, even one the watch's
            // history had forgotten (lib.js keeps those files).
            assert!(
                !self.watch.files.contains_key(f),
                "{}",
                ctx(&format!("{f} deleted, but its log is still on the watch"))
            );
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
        // The watch's settings as it has them (its speeds only when it has
        // the setting).
        let w = store.watch_state();
        assert_eq!(
            (w.m, w.gps, w.p),
            (
                Some(self.watch.m),
                Some(self.watch.gps),
                self.watch.progs.then_some(self.watch.p)
            ),
            "{}",
            ctx("the watch's settings")
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
    let mut totals = [0usize; 10];
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
            vec![link::sync_line(&[], None)],
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
            w.seen.loads,
        ]) {
            *t += n;
        }
    }
    // The runs went through every kind of trouble, often.
    eprintln!(
        "results {}, kept after the watch forgot them {}, tombstones {} ({} from the watch), \
         deletes on the phone {}, restarts {}, forged events {}, steps busy {}, files given up on {}, \
         transfers cut off by a load() {}",
        totals[0],
        totals[1],
        totals[2],
        totals[3],
        totals[4],
        totals[5],
        totals[6],
        totals[7],
        totals[8],
        totals[9]
    );
    let n = seeds() as usize;
    for (t, per_seed) in totals.iter().zip([10, 1, 2, 1, 2, 2, 2, 5, 1, 1]) {
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

/// Delivers what's in flight and ticks the watch until nothing is left,
/// with no input from the phone's user (the app closed).
fn run_by_itself(w: &mut World) {
    for _ in 0..1_000_000 {
        if w.deliver_to_watch() || w.deliver_to_phone() {
            continue;
        }
        if w.watch.xfer.is_none() {
            return;
        }
        w.now += 50;
        w.watch.tick();
        w.flush_watch();
    }
    panic!("never settled");
}

#[test]
fn a_delete_made_while_the_watch_is_away_reaches_it_when_it_connects() {
    // Three results, then thirty more: the watch's history has forgotten
    // the first three (the phone keeps them), whose logs are still on it.
    let mut w = World::with(8, 3);
    w.settle();
    let forgotten = w.watch.history.clone().unwrap()[2].f.clone();
    for _ in 0..KEEP {
        let r = &mut w.rng;
        w.watch.start_test(r);
        w.watch.stop_test(r);
    }
    w.settle();
    let listed = w.watch.history.clone().unwrap();
    assert!(w.watch.dropped.contains(&forgotten));
    assert!(w.watch.files.contains_key(&forgotten));
    let kept = listed[3].f.clone();
    // The watch goes out of range; one of each is deleted on the phone,
    // and Gadgetbridge drops the syncs.
    w.watch.connect(false);
    w.phone(Input::Delete(forgotten.clone()));
    w.phone(Input::Delete(kept.clone()));
    while w.deliver_to_watch() {}
    // It comes back and says state by itself; the app stays closed (no
    // Opened, no Sync now).
    w.now += 3_600_000;
    w.watch.connect(true);
    w.flush_watch();
    run_by_itself(&mut w);
    for f in [&forgotten, &kept] {
        assert!(!w.watch.files.contains_key(f), "{f} still on the watch");
        assert_eq!(w.phone.store.tombstones()[f], Tombstone::Done, "{f}");
    }
    assert!(
        !w.watch
            .history
            .as_ref()
            .unwrap()
            .iter()
            .any(|e| e.f == kept)
    );
    assert_eq!(w.phone.store.pending_deletes(), Vec::<String>::new());
    w.check(8);
}

#[test]
fn a_delete_whose_sync_is_lost_again_isnt_settled_by_the_next_state() {
    // A result the watch's history has forgotten (its log still on it),
    // deleted on the phone while the watch is away.
    let mut w = World::with(8, 3);
    w.settle();
    let forgotten = w.watch.history.clone().unwrap()[2].f.clone();
    for _ in 0..KEEP {
        let r = &mut w.rng;
        w.watch.start_test(r);
        w.watch.stop_test(r);
    }
    w.settle();
    assert!(w.watch.files.contains_key(&forgotten));
    w.watch.connect(false);
    w.phone(Input::Delete(forgotten.clone()));
    while w.deliver_to_watch() {}
    // The watch comes back: its connect state reaches the phone, which
    // answers with a sync carrying the delete...
    w.now += 3_600_000;
    w.watch.connect(true);
    w.flush_watch();
    assert!(w.deliver_to_phone());
    assert_eq!(w.link.to_watch.len(), 1);
    // ...which is lost (the link drops again), and the watch reconnects
    // a few seconds later and says state again by itself: not the
    // answer, so the delete goes again, and reaches it this time.
    w.watch.connect(false);
    while w.deliver_to_watch() {}
    w.now += 5_000;
    w.watch.connect(true);
    w.flush_watch();
    run_by_itself(&mut w);
    assert!(
        !w.watch.files.contains_key(&forgotten),
        "{forgotten}'s log is still on the watch"
    );
    assert_eq!(w.phone.store.tombstones()[&forgotten], Tombstone::Done);
    w.settle();
    w.check(8);
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
fn a_load_on_the_watch_mid_transfer_carries_on_with_the_app_closed() {
    let mut w = World::with(6, 4);
    w.phone(Input::Opened);
    // Part of the way through the first log...
    let mut pieces = 0;
    while pieces < 2 {
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
    // ...the watch goes back to the clock. No Tick follows (the app is
    // closed): only what the watch says moves the phone on.
    w.watch.load();
    w.flush_watch();
    for _ in 0..1_000_000 {
        if w.deliver_to_watch() || w.deliver_to_phone() {
            continue;
        }
        if w.watch.xfer.is_none() {
            break;
        }
        w.now += 50;
        w.watch.tick();
        w.flush_watch();
    }
    w.check(6);
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

#[test]
fn the_speeds_go_both_ways_with_a_watch_that_has_them() {
    // Seed 2: a watch with speed progressions.
    let mut w = World::with(2, 2);
    assert!(w.watch.progs);
    w.settle();
    assert_eq!(w.phone.store.watch_state().p, Some(0));
    w.phone(Input::SetWatch {
        m: None,
        gps: None,
        p: Some(2),
    });
    w.settle();
    assert_eq!(w.watch.p, 2);
    assert_eq!(w.phone.store.watch_state().p, Some(2));
    // The next test runs them, and its result says so on the phone.
    let r = &mut w.rng;
    w.watch.start_test(r);
    w.watch.stop_test(r);
    w.flush_watch();
    w.settle();
    let newest = &w.phone.store.results()[0];
    assert_eq!(newest.entry.p, 2);
    assert_eq!(newest.log, LogState::Complete);
    w.check(2);

    // Seed 3: a watch from before the Speeds setting ignores a p, and the
    // phone knows it has none.
    let mut w = World::with(3, 2);
    assert!(!w.watch.progs);
    w.phone(Input::SetWatch {
        m: Some(15),
        gps: None,
        p: Some(1),
    });
    w.settle();
    assert_eq!((w.watch.m, w.watch.p), (15, 0));
    assert_eq!(w.phone.store.watch_state().p, None);
    assert!(w.phone.store.results().iter().all(|r| r.entry.p == 0));
    w.check(3);
}
