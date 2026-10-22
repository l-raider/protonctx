{
  lib,
  rustPlatform,
  fetchFromGitHub,
  autoPatchelfHook,
  libxkbcommon,
  libx11,
  libxcb,
  libxcursor,
  libxi,
  libxrandr,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "protonctx";
  version = "1.2.2";

  src = fetchFromGitHub {
    owner = "l-raider";
    repo = "protonctx";
    # Tag the release that contains the iced port so this cannot resolve to an
    # older (Qt or GPUI) tree.
    rev = "v${finalAttrs.version}";
    # Update on each release: run `nix-prefetch-github l-raider protonctx --rev v${version}`
    # and paste the resulting hash here.
    hash = lib.fakeHash;
  };

  # No explicit cargoHash/cargoLock: buildRustPackage auto-detects
  # ${src}/Cargo.lock (it is committed upstream) and derives the dependency
  # hash from it.

  # iced (winit + softbuffer on X11, tiny-skia renderer) dlopen()s the X11
  # and xkbcommon libraries at runtime; the binary itself only links glibc.
  # The dlopen()'d libraries are added to the rpath in postFixup, since
  # autoPatchelfHook only handles NEEDED entries.
  nativeBuildInputs = [
    autoPatchelfHook
  ];

  runtimeDependencies = [
    libxkbcommon
    libx11
    libxcb
    libxcursor
    libxi
    libxrandr
  ];

  postFixup = ''
    patchelf --add-rpath "${lib.makeLibraryPath finalAttrs.runtimeDependencies}" \
      "$out/bin/protonctx"
  '';

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
      A single-window helper GUI (iced) that lists installed Steam games and
      lets you run an arbitrary .exe — or a built-in Wine tool such as
      winecfg/taskmgr — inside a selected game's Proton prefix.
    '';
    homepage = "https://github.com/l-raider/protonctx";
    license = lib.licenses.gpl3Only;
    mainProgram = "protonctx";
    maintainers = [ ];
    platforms = lib.platforms.linux;
  };
})
