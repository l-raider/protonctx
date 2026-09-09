use cxx_qt_build::CxxQtBuilder;

fn main() {
    // Single source of truth for the version: CARGO_PKG_VERSION (Cargo.toml).
    // Defined for the C++ translation units so the About dialog and any other
    // UI code can consume it without a hand-kept copy.
    // The value is wrapped in quotes because cc-rs emits `-DNAME=value`
    // verbatim; without the quotes the macro would expand to the numeric
    // literal 0.1.1 instead of the string "0.1.1".
    let version = format!("\"{}\"", env!("CARGO_PKG_VERSION"));

    // SAFETY: the closure only adds a `-DPROTONCTX_VERSION=...` define to the
    // cc::Build flags; it touches no other state.
    unsafe {
        CxxQtBuilder::new()
            .crate_include_root(None)
            .cc_builder(move |cc| {
                cc.define("PROTONCTX_VERSION", Some(version.as_str()));
            })
    }
    .qt_module("Widgets")
    .qt_module("Gui")
    .cpp_file("src/qt_main_ui.cpp")
    .cpp_file("src/qt_settings_ui.cpp")
    .cpp_file("src/qt_about_ui.cpp")
    .cpp_file("src/qt_logs_ui.cpp")
    .qrc("src/qt/resources.qrc")
    .files(["src/app_backend.rs"])
    .build();
}
