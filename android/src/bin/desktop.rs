//! The Android app's UI and store in a desktop window, for development:
//! `cargo run`, with the data in `./beep-data` (or `$BEEP_DATA`). There's
//! no watch on the desktop: the lines for it are logged (`RUST_LOG=info`),
//! so a sync asks and then says the watch isn't answering.
//!
//! **Demo data**, to look at the pages without a watch:
//!
//! ```sh
//! BEEP_DATA=/tmp/beep-demo cargo run -- --demo            # fills it, then runs
//! BEEP_DATA=/tmp/beep-demo cargo run -- --demo --running  # with a test running on the watch
//! ```
//!
//! `--demo` adds a dozen results over the last five months on both courses
//! to the data directory (`beep_android::ui::demo::fill`: heart rate logs
//! as the watch writes them, one with GPS speed, the newest one's log still
//! arriving, an old one with none), so use a directory of its own.
//!
//! **Screenshots**, for checking the pages without looking (under
//! `xvfb-run` on a machine with no display):
//!
//! - `BEEP_PAGE`: the page to open: `results`, `result` (the newest with
//!   its log), `result-gps`, `result-arriving`, `result-nolog`, `delete`
//!   (a result with Delete's confirm), `menu`, `watch`, `watch-sending`
//!   (15 m sent to the watch, waiting for its answer) or `settings`.
//! - `BEEP_THEME`: `light` or `dark` (saved, as Settings would).
//! - `BEEP_SIZE`: the window, `WxH` points (411x891 by default, a phone's).
//! - `BEEP_SHOT`: a PNG to write a second after it opens, then it quits.
//!
//! ```sh
//! BEEP_DATA=/tmp/beep-demo BEEP_PAGE=watch BEEP_THEME=dark BEEP_SHOT=watch-dark.png \
//!   xvfb-run -a cargo run -- --demo
//! ```

use beep_android::store::{LogState, Store, Theme};
use beep_android::ui::{App, Page};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() -> eframe::Result {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(std::env::var_os("BEEP_DATA").unwrap_or_else(|| "beep-data".into()));
    if args.iter().any(|a| a == "--demo") {
        let running = args.iter().any(|a| a == "--running");
        if let Err(e) = beep_android::ui::demo::fill(&dir, beep_android::health::now_s(), running) {
            eprintln!("can't fill {}: {e}", dir.display());
            std::process::exit(1);
        }
    }
    let size = std::env::var("BEEP_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some([w.parse().ok()?, h.parse().ok()?])
        })
        .unwrap_or([411.0, 891.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(size),
        ..Default::default()
    };
    eframe::run_native(
        "Beep Test",
        options,
        Box::new(move |_cc| {
            let store = beep_android::platform::open(&dir)?;
            match std::env::var("BEEP_THEME").as_deref() {
                Ok("light") => store.set_theme(Theme::Light)?,
                Ok("dark") => store.set_theme(Theme::Dark)?,
                _ => {}
            }
            let mut app = beep_android::app(&dir)?;
            // As android_main: the direct link if that's on (which the
            // desktop says it can't do).
            beep_android::platform::keep_watch();
            if let Ok(page) = std::env::var("BEEP_PAGE") {
                open(&mut app, &store, &page);
            }
            Ok(Box::new(Desktop {
                app,
                shot: std::env::var_os("BEEP_SHOT").map(PathBuf::from),
                started: Instant::now(),
                asked: false,
            }))
        }),
    )
}

/// Opens the page `BEEP_PAGE` names.
fn open(app: &mut App, store: &Arc<Store>, page: &str) {
    let results = store.results();
    let gps = |f: &str| {
        store
            .log(f)
            .is_some_and(|l| l.samples.iter().any(|s| !s.gps_speed.is_nan()))
    };
    let find = |want: &dyn Fn(&beep_android::store::TestResult) -> bool| {
        results.iter().find(|r| want(r)).map(|r| r.entry.f.clone())
    };
    let result = match page {
        "result" | "delete" => find(&|r| r.log == LogState::Complete),
        "result-gps" => find(&|r| gps(&r.entry.f)),
        "result-arriving" => find(&|r| matches!(r.log, LogState::Missing | LogState::Partial(_))),
        "result-nolog" => find(&|r| r.log == LogState::NoLog),
        _ => None,
    };
    match page {
        "watch" => app.open(Page::Watch),
        "settings" => app.open(Page::Settings),
        "menu" => app.open_menu(),
        "watch-sending" => {
            app.open(Page::Watch);
            app.set_watch(Some(15), None);
        }
        _ => {}
    }
    if let Some(f) = result {
        app.open(Page::Result(f));
        if page == "delete" {
            app.ask_delete();
        }
    }
}

/// The app, and the screenshot it was asked to take.
struct Desktop {
    app: App,
    shot: Option<PathBuf>,
    started: Instant,
    asked: bool,
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.update(ctx, frame);
        let Some(path) = self.shot.clone() else {
            return;
        };
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            if let Err(e) = write_png(&path, &image) {
                eprintln!("can't write {}: {e}", path.display());
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if !self.asked && self.started.elapsed() >= Duration::from_secs(1) {
            self.asked = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

/// Writes `image` as an RGBA PNG: zlib with stored blocks, which every
/// reader takes, so no image crate is needed.
fn write_png(path: &Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let [w, h] = image.size;
    let mut raw = Vec::with_capacity(h * (1 + 4 * w));
    for row in image.pixels.chunks(w) {
        raw.push(0); // no filter
        for p in row {
            raw.extend_from_slice(&p.to_srgba_unmultiplied());
        }
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        png.extend_from_slice(&body);
        png.extend_from_slice(&beep_android::link::crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    std::fs::write(path, png)
}
