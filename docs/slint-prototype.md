# Slint prototype vs the iced port

Branch `slint-prototype` replaces the iced 0.14 UI on `main` with Slint 1.18.1
(winit backend, software renderer). Everything outside the UI (Steam discovery,
launching, the portal file chooser, config, theme reading through `native-theme`)
is shared, so the numbers below compare the toolkits, not the apps.

## Measurements

Both binaries built from scratch with the shipping profile (`publicrelease`: fat
LTO, `opt-level = "s"`, stripped) and run side by side under Xvfb (1000×700, no
window manager, `WAYLAND_DISPLAY` unset), live KDE theme, the same six games.

| | iced (`main`) | Slint (`slint-prototype`) |
|---|---|---|
| Stripped binary | 9.6 MB | 14.9 MB (14.2 MB without accessibility) |
| Binary, `xz -9` (≈ package payload) | 2.9 MB | 5.3 MB |
| Dynamic libraries | libc, libm, libgcc_s only (X11 libs are `dlopen`ed) | also fontconfig, freetype, harfbuzz, glib, libpng, libxml2 … (all on every desktop) |
| Idle PSS after start-up | 14.8 MB | 16.7 MB |
| Idle RSS after start-up | 19.8 MB | 24.9 MB |
| Threads | 13 | 7 |
| Idle CPU, 10 s | 0 ms | 0 ms |
| CPU for 10 s of pointer motion over the table (120 Hz) | **1970 ms** | **80 ms** |
| Clean `publicrelease` build | 113 s | 127 s |
| Incremental dev build, one-line UI edit | 1.1–3.2 s | 1.7–2.0 s (`.slint` or `.rs`) |

For reference, the gpui build measured earlier on the live desktop used about
171 MB PSS (iced: about 23 MB there; the Xvfb figures above are lower for both
because nothing is composited).

- **Hover CPU is the headline.** iced repaints the whole window on every
  pointer move; Slint's software renderer repaints only the dirty region (two
  rows), so a hover sweep costs about 1/25 of the CPU. This was the
  first thing that bothered you in the iced port.
- **Size.** Slint brings its own text stack (parley/fontique, skrifa, swash,
  harfrust), resvg for the SVG icons, and AccessKit. `cargo bloat` puts
  `accesskit_unix` at about 1 MB and the font/text crates at about 2 MB; zbus is
  shared with `ashpd` either way. Accessibility (a screen reader can read the
  UI; iced 0.14 has none) costs 0.75 MB and 0.6 MB PSS, so it is kept on.
- **Memory** is within 2 MB PSS of iced. Both are an order of magnitude
  below gpui.

## Code size

| | iced (`main`) | Slint |
|---|---|---|
| UI declaration | `views/*` + `menu.rs` + `assets.rs`: 1 994 lines of Rust | `ui/*.slint`: 1 425 lines |
| Glue / state | `app.rs` 1 187 | `app.rs` 650 + `state.rs` 376 + `log_buffer.rs` 115 + `ui.rs` 144 |
| Tests | `app/update_tests.rs` 825 + `visual_tests.rs` 190 | tests in `state.rs`/`log_buffer.rs` + `visual_tests.rs` 320 |
| Total (UI-related, incl. theme, background, x11 hints) | 5 157 | 3 696 |

The `.slint` markup holds what iced spread over style closures and custom
widgets (scrollbars, the table, menus). The menus and the context menu are
`PopupWindow`s, so there is no hand-written overlay code.

## Visual parity

Headless renders (`cargo test visual_states -- --ignored`) and Xvfb screenshots
match the iced/gpui layout to within 1–2 px: toolbar, sortable/resizable table
with Breeze row fills, outline buttons, split log pane, status bar, both menus
and all four dialogs. Cursors stay the arrow over buttons and menu items (Slint's
`TouchArea` defaults to the arrow; nothing to override, unlike the iced fix).

## Caveats and things that differ

1. **`unsafe_code` is `deny`, not `forbid`.** The code `slint-build` generates
   carries `#[allow(unsafe_code)]`, which `forbid` rejects (E0453). Hand-written
   code still cannot use `unsafe` (`deny` errors just the same); only the
   generated module may override it.
2. **Generated code is excused from three clippy restriction lints**
   (`unwrap_used`, `expect_used`, `todo`) by putting `include_modules!` in its
   own module in `src/ui.rs`.
3. **X11 is forced whenever `DISPLAY` is set**, exactly like the iced port and
   for the same reason: winit (which Slint uses) cannot parent a dialog on
   Wayland. Slint's Wayland backend is still compiled in and used when no X
   display exists.
4. **Dialog parenting needs a remap.** Slint creates the winit window lazily,
   one event-loop turn after `show()`, so the transient/dialog/modal hints
   (`x11_dialog::mark_dialog`) are set once both window ids exist, and the
   dialog is then hidden and shown again so KWin reads them at map time. The
   hints are confirmed with `xprop`; whether KWin centres the dialog on the
   main window and keeps it off the taskbar has to be checked on your desktop
   (Xvfb has no window manager).
5. **Clipboard through `arboard`.** Slint has no public clipboard API for
   arbitrary text (only `TextInput` copy), so the "Copy … path" items use
   `arboard`, X11-only (no Wayland feature), matching how the app runs.
6. **The table is not virtualised.** Rows are a `for` loop in a `Flickable`
   (needed for the shared horizontal scroll with the header). Fine for
   hundreds of games; a library of thousands would want `ListView`.
7. **Themes are per window.** Slint globals belong to one component, so
   `ui::apply_theme` copies the tokens into every window when it is created. Live
   theme switching would mean re-applying them to the open windows.

## Not yet tested on the live desktop

Xvfb and headless tests cover rendering, hover, the input routing of every
button, menu item, header and shortcut, and the dialog keys. These need a check
on Plasma:

- About / Settings / error / delete dialogs: centred on the main window, no
  taskbar entry, above the main window, focus returns to it on close; whether the
  remap flickers.
- The hamburger and context menus as real X11 popups (position, closing on an
  outside click, closing when the main window loses focus).
- Launching a tool (log streaming, the spinner, the exit message), Browse…
  through the portal, both Copy items (paste somewhere), Delete Shader Cache.
- F5, Ctrl+C / Ctrl+A / Ctrl+L, log selection with the mouse, the split and
  column drags, fractional scaling sharpness.
