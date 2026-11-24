# Chia testnet monitor, native

A GPUI (via [gpui-kit](https://gpui-kit.com)) port of the dashboard in `../index.html`. It reads the
same `/api/state` JSON, so `server.py` stays the data source (it needs the Chia Python libraries for
the plot-filter math); only the browser is replaced.

Start `server.py` as usual (see the main README), then:

```sh
cd native
cargo run --release                      # first build takes a few minutes: GPUI is a big dependency
cargo run --release -- --url http://127.0.0.1:9000 --theme dark --size 1200x1000
```

Options: `--url` (default `http://127.0.0.1:8899`), `--theme light|dark` (default: follow the system),
`--size WxH`. Closing the window quits; the server keeps running.

On macOS the app switches off the system's "font smoothing" (it sets `AppleFontSmoothing` to 0 in its own
preferences domain), because that thickens light-on-dark text and makes it look heavier than the web page.
`--system-text` keeps the system behaviour. `CHIA_CONFETTI_DEMO=1` fires the win confetti four seconds after launch.

### macOS app

```sh
./bundle.sh --open        # builds "Testnet Monitor.app" next to the cargo target directory
```

It builds a release binary, wraps it in an `.app` with the icon (`macos/icon.svg`, rendered to
`macos/icon-1024.png`; the script makes the `.icns`), and signs it ad hoc, which is enough to run it on the Mac
that built it. Copy it to `/Applications` if you like. Giving it to other Macs would need a Developer ID
signature and notarization, which the script doesn't do.

A Finder launch can't pass `--url`. To point the app at a server that isn't on `127.0.0.1:8899`, put the URL on the
first line of `~/.chia-monitor-native-url` (or set `CHIA_MONITOR_URL` when running the binary directly).
Order of precedence: `--url`, `CHIA_MONITOR_URL`, that file, the default. With no server reachable the window
says so and keeps retrying.

**Requirements:** Rust 1.85 or newer. On macOS, Xcode with the Metal toolchain
(`xcodebuild -downloadComponent MetalToolchain` if `metal` is missing).
**Platforms:** developed and tested on macOS (Apple silicon). Windows and Linux use GPUI's
DirectX and Vulkan backends and the code avoids platform-specific calls, but they have not been tested.

## Architecture (speed and memory)

- **One process, no runtime.** No webview, JS engine or DOM. Release build is ~6 MB with the fonts embedded.
- **Typed, lean parsing.** `serde` deserialises only the fields the UI reads; the rest of the ~100 KB
  payload is skipped without allocating. The block sequence stays a single byte string.
- **Polling on the background executor.** A 2 s loop fetches and parses off the UI thread, then swaps an
  `Arc<Snapshot>` in. Nothing is copied into the charts; they hold an `Arc` clone.
- **Charts paint quads and paths directly** (`canvas`), so the ribbon (hundreds of bars), the
  signage-point chart and the meters add no elements to the layout tree. Text, tables and tooltips use
  ordinary elements.
- **Idle means idle.** GPUI only redraws on change. The countdown is its own entity on a timer aligned to
  the second, so a tick re-renders one small view. Tooltips are their own entity, so moving the mouse
  never re-renders the page. Confetti is the only thing that asks for animation frames, and only while
  particles are alive.
- **Same look.** Palette tokens, layout grid and sizes are copied from the CSS. Archivo is embedded as
  static instances cut from the variable font at the exact width/weight pairs the page uses (for
  example width 66 / weight 780 for the countdown); `assets/make_fonts.py` regenerates them (SIL OFL, see `assets/fonts/OFL.txt`). Light and dark follow the system.

Left out on purpose or not yet done: the looping "breathing" animations (they would force continuous
redraws), the bar rise-in intro, and the tab-favicon status dot (the window title carries the same status text).

## Benchmark

`bench/sample.py` samples process trees with `ps` (CPU time, RSS) and `footprint` (macOS physical
footprint). Both apps ran side by side, visible, against the same live server for 120 s. The browser was Chrome in `--app` mode with a throwaway profile, counting
every Chrome process (browser, GPU, network, renderer).

| | Chrome tab (6 processes) | Native | Change |
|---|---|---|---|
| CPU, average | 6.43 % of a core | 0.74 % | 8.7x lower |
| RSS, average | 820 MB | 102 MB | 8.0x lower |
| Physical footprint, average | 608 MB (peak 781) | 305 MB (peak 306) | 2.0x lower |

`server.py` serves both and costs the same either way: 3.9 % CPU, 39 MB. Counting it, the whole
browser setup is about 10.3 % CPU and the native setup about 4.6 %.

The native footprint is almost all GPU memory that GPUI's Metal renderer sets aside (about 280 MB at a
600x400 window, 320 MB at 1800x1200), not heap; RSS is the better figure for what the app itself holds.

Reproduce with both running and visible:

```sh
python3 bench/sample.py 120 3 '{"chrome":"<unique chrome --user-data-dir>","native":"chia-monitor-native","server":"server.py"}'
```

## Tests

`cargo test` checks the JSON model against a captured, anonymised `/api/state`
(`tests/fixtures/state.json`) and the number/time formatting against the page's helpers.
