# bleep

A bleep test (20 m multistage shuttle run) app for [Bangle.js](https://banglejs.com/)
smartwatches, with heart rate logging and TCX export.

The app lives in [`apps/bleeptest`](apps/bleeptest) and follows the
[BangleApps](https://github.com/espruino/BangleApps) layout, so it can be dropped straight
into the official app loader. See its [README](apps/bleeptest/README.md) for how to use it.

## Installing on a watch

**Your own App Loader (recommended, gives you the TCX download page)**

1. Fork [espruino/BangleApps](https://github.com/espruino/BangleApps) and copy
   `apps/bleeptest` into its `apps/` folder.
2. Enable GitHub Pages on the fork (*Settings → Pages*, deploy from the default branch).
3. Open `https://<your-user>.github.io/BangleApps/`, connect your watch and install
   **Bleep Test**. The download icon next to it under *My Apps* opens the export page.

**Quick try with the Web IDE**

Open the [Espruino Web IDE](https://www.espruino.com/ide/), connect, paste `app.js` and
upload it to storage as `bleeptest.app.js`. The HR logs are saved on the watch, but you'll
need the App Loader page above to convert them to TCX.

## Development

`npm test` runs the app, engine, boot file and widget against a simulated Bangle.js with a
virtual clock. It checks the level tables for all three courses, stopping from the menu, a
test that carries on at the clock face (widget, cues, HR log), a full test without drift,
and the TCX export. The generated files land in `test/out/`.
