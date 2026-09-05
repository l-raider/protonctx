# protonctx

Helper GUI tool to launch executables inside a Steam game's Proton context.

A single-window Linux (mainly KDE Plasma) utility that lists installed Steam games and lets
you run an arbitrary `.exe` (or a built-in Wine tool such as `winecfg`/`taskmgr`) inside a
selected game's Proton prefix.

## Features

- Lists installed Steam games across **all** Steam library folders.
- Shows each game's compatibility tool (from Steam's `config.vdf`), falling back to the
  resolved Proton directory when no explicit mapping exists.
- "Select" a game, then either **Browse…** for an executable or launch a built-in tool
  (`winecfg`, Task Manager, Explorer, Registry Editor).
- Standard top menu with **Settings** (placeholder) and **About** (version info).

## Building

Requirements:

- Rust 2024 edition toolchain (1.87+).
- Qt 6 development libraries (the UI is native Qt Widgets via cxx-qt). On Fedora:
  `dnf install qt6-qtbase-devel`. A C++ toolchain and `qmake` are also required.

```sh
cargo build --release
```

## Usage

```sh
cargo run
```

## How it works

- **Game discovery**: reads `libraryfolders.vdf` (all libraries) and each `appmanifest_*.acf`
  (installed apps), filtering out compatibility tools, Steam Linux Runtimes, and Steamworks
  redistributables.
- **Compatibility tool**: read from `config.vdf`'s `CompatToolMapping`.
- **Proton directory**: resolved authoritatively from `compatdata/<appid>/config_info`
  (works for both built-in tools under `steamapps/common` and custom tools under
  `compatibilitytools.d`, e.g. GE-Proton).
- **Launching**: invokes `<proton_dir>/proton runinprefix <arg>` with the Steam compat
  environment (`STEAM_COMPAT_DATA_PATH`, `STEAM_COMPAT_CLIENT_INSTALL_PATH`, `SteamGameId`).

## License

GNU GPL v3. See [LICENSE](LICENSE).