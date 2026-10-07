use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("com.example.cxxqt_minimal").qml_file("src/Main.qml"),
    )
    .files(["src/backend.rs"])
    .build();
}
