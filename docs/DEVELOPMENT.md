# Developing Beep Test

- User guide (also the App Loader's "Read more"): [README.md](../README.md)
- Tests: `npm test` in a headless emulator, see [tests/README.md](../tests/README.md)

## How it's built

| File | On the watch | What it does |
|---|---|---|
| `lib.js` | `beeptest` (module) | The test engine: level tables, cues, HR/GPS logging, results. The schedule is a pure function of time since the start, so it never drifts and can be resumed. |
| `boot.js` | `beeptest.boot.js` | On every app load, resumes a test in progress from `beeptest.state.json`, so it keeps running at the clock. |
| `widget.js` | `beeptest.wid.js` | Shows `level.shuttle` and heart rate on the clock while a test runs. |
| `app.js` | `beeptest.app.js` | The screens and menus. |
| `interface.html`, `beeplog.js` | (App Loader only) | The download page: turns a log into TCX or CSV. |

Data on the watch: `beeptest.json` (past results), `beeptest.settings.json`
(course, GPS), `beeptest.state.json` (the test in progress) and one
`beeptest.<start time>.csv` StorageFile per test with a line a second.

## Installing development builds

**One click (easiest).** Open
[cbishopvelti.github.io/beep](https://cbishopvelti.github.io/beep/) in
Chrome or Edge, press **Connect**, pick the watch and press the install
(or update) button on Beep Test. It's the normal Bangle.js App Loader with
only Beep Test on it, rebuilt from main on every push by
[`.github/workflows/pages.yml`](../.github/workflows/pages.yml)
(`sh tools/build-loader.sh` builds the same thing locally into `_site/`).
The download page for the heart rate logs is there too: the download icon
next to Beep Test under *My Apps*.

The first time, GitHub Pages has to be switched on: in the repository's
Settings > Pages, set Source to **GitHub Actions**.

**Trying a pull request before it's merged.** Every open PR from a branch in
this repository gets its own install page at
`cbishopvelti.github.io/beep/pr-<number>/`, with a yellow banner saying
which PR it is. A bot comment on the PR links to it and is kept up to date.
The preview's version is main's plus a fourth number (`0.01.12.3` is 3 PR
commits on top of main's `0.01.12`), so the App Loader offers it as an
update, and main's next build after the merge as an update again. To go back
without merging, open the main page and press reinstall on Beep Test.
Previews rebuild on every push to the PR or to main and disappear when the
PR closes.

**App Loader from files.** Download this repository (Code, then Download ZIP)
and unzip it. Open the [App Loader](https://banglejs.com/apps/) in Chrome or
Edge and connect to the watch. On the **More...** tab, press **Install App from
Files** and select every file in the folder (it only takes the ones
`metadata.json` lists).

**Espruino Web IDE.** Run `npm run bundle` (Node 18 or later, no packages
needed), or download `beeptest-install` from the latest
[Emulator tests run](https://github.com/cbishopvelti/beep/actions/workflows/test.yml) on GitHub. Open the
[Web IDE](https://www.espruino.com/ide/), connect to the watch, paste
`dist/beeptest-install.js` into the editor on the right and press **Send to
Espruino**. It writes the files, adds Beep Test to the launcher and restarts
the clock. Sending it again updates the app. (This route has no download
page for the logs: use one of the App Loaders for that.)

## Testing on the watch

The emulator tests cover the logic, but these need the real watch. To get
partway into a test without running it, connect the Web IDE and type a line
from the table in the console (left side). It starts a test that began a
while ago.

| To test | Type |
|---|---|
| Level 2 starts (3 buzzes) in 10 s | `require("beeptest").start(20,{start:Date.now()-53000})` |
| Level 8 starts in 10 s, 20m | `require("beeptest").start(20,{start:Date.now()-432000})` |
| The end of level 21 (test complete) in 10 s | `require("beeptest").start(20,{start:Date.now()-1311000})` |
| 15m course, with GPS speed | `require("beeptest").start(15,{gps:true})` |

`require("beeptest").cancel()` throws away a test started this way. Disconnect the Web IDE
before a check that needs the watch on your wrist.

1. **Install.** Beep Test is in the launcher with its icon. Opening it shows
   the course and "Tap to start". No widget on the clock while idle.
2. **Start.** Press the button on the start screen: a menu opens and nothing
   buzzes. Back out, then tap the screen: a 3 s countdown, three buzzes and the
   test starts.
3. **Cues.** With the watch on your wrist, the single shuttle buzz and the
   triple level buzz are easy to tell apart without looking.
4. **Long press.** During a test hold the button about 1.5 s: the watch goes
   to the clock, the widget shows `level.shuttle` and heart rate, and the buzzes
   carry on. Unlock the watch and tap the widget: the test screen comes back. A short press (under 1 s) opens the menu instead.
   A swipe right on the test screen does the same as the long press.
5. **Stop & save.** Press the button, wait a few seconds, choose Stop & save:
   the result is the last shuttle completed when you pressed the button.
6. **Cancel test.** Asks first, then goes back to the start screen with no result.
7. **Export.** In the App Loader, the download icon lists the test; TCX
   imports into Strava or Garmin Connect with heart rate and one lap per level.
8. **GPS speed.** Outdoors, with GPS speed on, the bottom row shows a speed
   once there's a fix, and GPS goes off when the test ends.

## Releasing to the Bangle.js App Loader

To list Beep Test in the official App Loader (banglejs.com/apps), it goes
into the [espruino/BangleApps](https://github.com/espruino/BangleApps)
repository as `apps/beeptest/`. This repository stays the place it is
developed; a release copies the app's files across.

1. Bump `version` in `metadata.json` (two numbers, such as `0.02`) and add a
   line for it at the end of `ChangeLog`, in the same `0.02: what changed`
   form. The App Loader shows that line as the release notes.
2. If the screens changed, `npm run screenshots` retakes the pictures in
   `screenshots/` in the emulator.
3. `npm run store-check` runs BangleApps' own checks (`bin/sanitycheck.js`
   and its ESLint rules, which allow no warnings) on the app. CI runs it on
   every pull request too.
4. In a fork of BangleApps, on a new branch from its `master`, copy the
   files in: `node tools/app-files.js <fork>/apps/beeptest` (it copies only
   what `metadata.json` uses, plus `ChangeLog`, `README.md`, the screenshots
   and the download page). Commit with an `AI-assisted` trailer if AI helped
   (their `AGENTS.md` asks for it, without naming the AI service), push, and
   open a pull request to `espruino/BangleApps` `master`.
5. Once it's merged it appears at
   [espruino.github.io/BangleApps](https://espruino.github.io/BangleApps/)
   (the development App Loader) straight away, and at
   [banglejs.com/apps](https://banglejs.com/apps/) when that is next
   refreshed.

Code in BangleApps is MIT licensed: submitting it there means agreeing to
that for the app.

The install page here (`cbishopvelti.github.io/beep`) keeps publishing
main as `<version>.<build>` (`0.01.12`), which the App Loader sees as newer
than the store's `0.01`, so a watch on development builds stays on them.
