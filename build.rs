use cxx_qt_build::CxxQtBuilder;

fn main() {
    CxxQtBuilder::new()
        .crate_include_root(None)
        .qt_module("Widgets")
        .cpp_file("src/qt_app.cpp")
        .qrc("src/qt/resources.qrc")
        .files(["src/app_backend.rs"])
        .build();
}
