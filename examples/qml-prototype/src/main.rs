mod backend;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};

fn main() {
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        QQuickStyle::set_style(&QString::from("org.kde.desktop"));
    }

    let mut app = QGuiApplication::new();

    if let Some(mut app) = app.as_mut() {
        app.as_mut().set_application_name(&QString::from("protonctx"));
        app.as_mut()
            .set_application_display_name(&QString::from("protonctx"));
    }

    let mut engine = QQmlApplicationEngine::new();

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from(
            "qrc:/qt/qml/com/example/cxxqt_minimal/src/Main.qml",
        ));
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
