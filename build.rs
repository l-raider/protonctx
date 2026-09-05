use cxx_qt_build::CxxQtBuilder;

fn main() {
    CxxQtBuilder::new()
        .crate_include_root(None)
        .qt_module("Widgets")
        .cpp_file("src/qt_main_ui.cpp")
        .cpp_file("src/qt_settings_ui.cpp")
        .cpp_file("src/qt_about_ui.cpp")
        .qrc("src/qt/resources.qrc")
        .files(["src/app_backend.rs"])
        .build();
}
