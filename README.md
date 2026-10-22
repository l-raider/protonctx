# protonctx
Disclaimer: This project was made with LLM assistance.

Helper GUI tool to launch executables inside a Steam game's Proton context.

A single-window Linux (mainly KDE Plasma) utility that lists installed Steam games and lets
you run an arbitrary `.exe` (or a built-in Wine tool such as `winecfg`/`taskmgr`) inside a
selected game's Proton prefix. The UI is built with [iced](https://iced.rs/) using its
CPU renderer (tiny-skia) and follows the KDE colour scheme and fonts — no Qt or C++
toolchain is required to build it.

## Features

- Lists installed Steam games across **all** Steam library folders.
- Shows each game's compatibility tool (from Steam's `config.vdf`), falling back to the
  resolved Proton directory when no explicit mapping exists.
- "Select" a game, then either **Browse…** for an executable or launch a built-in tool
  (`winecfg`, Task Manager, Explorer, Registry Editor).
- Right-click a row for a context menu (launch tools, copy the compatdata or
  compatibility-tool path, or **delete the game's shader cache**).
- Toolbar menu with **Settings** (remember the last-used directory) and **About**
  (version info).

## Building

Requirements:

- Rust 2024 edition toolchain (1.98+).
- [mold](https://github.com/rui314/mold): `.cargo/config.toml` links with it for fast
  builds (`dnf install mold` / `apt install mold`; or override with `RUSTFLAGS=""`).
- A C toolchain for linking (`build-essential` / `gcc`); no development headers are
  needed.
- At runtime the X11 libraries (Xlib, xcb, Xcursor, Xrandr, Xi) and xkbcommon are loaded
  dynamically; every desktop install already has them, and Plasma runs X11 apps through
  XWayland. No OpenSSL and no GPU driver are needed.

```sh
cargo build --release
```

## Packaging

Debian and RPM packages are built with `cargo-deb` and `cargo-generate-rpm`
(installed separately: `cargo install cargo-deb cargo-generate-rpm`). Icon PNGs
are rendered from `packaging/icons/hicolor/scalable/apps/protonctx.svg` at
build time, so `rsvg-convert` (`librsvg2-tools`) or ImageMagick `convert` is
also required.

```sh
./build-deb.sh    # -> target/debian/protonctx_*.deb
./build-rpm.sh    # -> target/generate-rpm/protonctx-*.rpm
```

## Usage

```sh
cargo run
```

Dev builds keep protonctx itself unoptimised for fast rebuilds but optimise all
dependencies (the software renderer is unusably slow otherwise), so the first build
takes a few minutes. `cargo run -- --preset light` (or `dark`) forces the bundled
Breeze colours instead of the live KDE scheme.

protonctx runs as an X11 client (XWayland on a Plasma Wayland session, which Plasma 6.8
keeps after dropping the X11 session). iced 0.14 cannot make one window the child of
another on Wayland, but on X11 that is a window property: the About, Settings and
confirmation dialogs are marked transient for the main window (`WM_TRANSIENT_FOR`,
dialog type, modal), so they stay off the taskbar, open centred on the main window and
stay above it, like Qt dialogs. With fractional display scaling, sharpness follows
KWin's *Legacy X11 App Scaling* setting.

### Keyboard shortcuts

| Chord | Action |
|-------|--------|
| `F5` | Refresh the game list |
| `Ctrl+C` | Copy the selected log text |
| `Ctrl+A` | Select the whole log |
| `Ctrl+L` | Clear the log |

### Flatpak: write access for shader-cache deletion

The Flatpak manifest deliberately grants **read-only** access to `$HOME`, which
is enough to discover games and launch executables but not to modify anything on
disk. The **Delete Shader Cache** action writes to Steam's library directories
(removing `<library>/steamapps/shadercache/<app-id>`), so it will fail inside the
sandbox unless you grant write access to the relevant Steam locations yourself.

Grant access to each library you use, for example:

```sh
flatpak override --user --filesystem=~/.local/share/Steam io.github.l_raider.protonctx
# repeat for any secondary libraries listed in libraryfolders.vdf
```

Use `flatpak override --user --nofilesystem=...` to revoke it again. Without this,
the read-only features (game list, launching, copying paths) keep working
normally; only deletion is affected.

## How it works

- **Game discovery**: reads `libraryfolders.vdf` (all libraries) and each `appmanifest_*.acf`
  (installed apps), filtering out compatibility tools, Steam Linux Runtimes, and Steamworks
  redistributables.
- **Compatibility tool**: read from `config.vdf`'s `CompatToolMapping`.
- **Proton directory**: resolved authoritatively from `compatdata/<appid>/config_info`
  (works for both built-in tools under `steamapps/common` and custom tools under
  `compatibilitytools.d`, e.g. GE-Proton).
- **Compatdata (prefix) location**: Steam stores a Proton prefix under the *library* the
  game is installed in, i.e. `<library>/steamapps/compatdata/<appid>`. The launcher points
  `STEAM_COMPAT_DATA_PATH` at the game's library, so a game installed on a secondary library still launches against the correct prefix.
- **Launching**: invokes `<proton_dir>/proton runinprefix <arg>` with the Steam compat
  environment (`STEAM_COMPAT_DATA_PATH`, `STEAM_COMPAT_CLIENT_INSTALL_PATH`, `SteamGameId`).

## License

GNU GPL v3. See [LICENSE](LICENSE).

## Attribution

The application icon
(`packaging/icons/hicolor/scalable/apps/protonctx.svg`) is "Game Development"
by [Sooodesign](https://www.svgrepo.com/svg/426047/game-developement), licensed
under [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/).