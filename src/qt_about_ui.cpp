#include <QtWidgets/QMessageBox>
#include <QtWidgets/QWidget>

// Mirrors the version string the backend exposes via `getApp_version()`
// (app_backend.rs populates app_version from CARGO_PKG_VERSION), so the About
// dialog stays self-contained and does not need the backend instance.
#define PROTONCTX_VERSION "0.1.0"

void show_about_dialog(QWidget *parent) {
  const QString text =
      QStringLiteral(
          "protonctx %1\n\n"
          "Launch executables inside a Steam game's Proton context.\n\n"
          "Licensed under the GNU GPL v3.")
          .arg(QStringLiteral(PROTONCTX_VERSION));
  QMessageBox::about(parent, QStringLiteral("About protonctx"), text);
}