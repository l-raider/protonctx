{
  lib,
  rustPlatform,
  fetchFromGitHub,
  pkg-config,
  autoPatchelfHook,
  qt6,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "protonctx";
  version = "1.1.0";

  src = fetchFromGitHub {
    owner = "l-raider";
    repo = "protonctx";
    rev = "a67da8aed2a2451b8668554090dd48fc9e9af01f";
    hash = "sha256-2WxLThI/vLAEcMUVcObzqo/cywuSO1MRopczZHTtCbQ=";
  };

  # No explicit cargoHash/cargoLock: buildRustPackage auto-detects
  # ${src}/Cargo.lock (it is committed upstream) and derives the dependency
  # hash from it.

  # cxx-qt locates Qt via `qmake` on PATH and resolves headers/libs/moc/rcc
  # through `qmake -query`, so qtbase's dev output must be available at build
  # time. wrapQtAppsHook produces a wrapper with QT_PLUGIN_PATH so the SVG
  # imageformat and the platform plugin resolve at runtime; autoPatchelfHook
  # fixes rpaths for the Qt shared libraries the binary links against.
  nativeBuildInputs = [
    pkg-config
    autoPatchelfHook
    qt6.wrapQtAppsHook
    qt6.qtbase
  ];

  buildInputs = [
    qt6.qtbase
    qt6.qtsvg # SVG imageformat plugin for the embedded icon
  ];

  # Install the desktop entry and the pre-rendered hicolor icon theme
  # (committed under packaging/, so no rsvg-convert step is needed at build
  # time).
  postInstall = ''
    install -Dm644 ${src}/packaging/protonctx.desktop \
      "$out/share/applications/protonctx.desktop"

    for size in 16 24 32 48 64 128 256; do
      install -Dm644 \
        "${src}/packaging/icons/hicolor/''${size}x''${size}/apps/protonctx.png" \
        "$out/share/icons/hicolor/''${size}x''${size}/apps/protonctx.png"
    done

    install -Dm644 ${src}/packaging/icons/hicolor/scalable/apps/protonctx.svg \
      "$out/share/icons/hicolor/scalable/apps/protonctx.svg"
  '';

  meta = {
    description = "Launch executables inside a Steam game's Proton context";
    longDescription = ''
      A single-window helper GUI (native Qt Widgets) that lists installed Steam
      games and lets you run an arbitrary .exe — or a built-in Wine tool such as
      winecfg/taskmgr — inside a selected game's Proton prefix.
    '';
    homepage = "https://github.com/l-raider/protonctx";
    license = lib.licenses.gpl3Only;
    mainProgram = "protonctx";
    maintainers = [ ];
    platforms = lib.platforms.linux;
  };
})
