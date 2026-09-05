#include <QtWidgets/QMessageBox>
#include <QtWidgets/QWidget>

// Defined by build.rs from CARGO_PKG_VERSION (Cargo.toml is the single source
// of truth for the version; see build.rs).
#ifndef PROTONCTX_VERSION
#error "PROTONCTX_VERSION must be defined by build.rs (from Cargo.toml)"
#endif

void show_about_dialog(QWidget *parent) {
  const QString text =
      QStringLiteral(
          "protonctx v%1\n\n"
          "Launch executables inside a Steam game's Proton context.\n\n"
          "Licensed under the GNU GPL v3.")
          .arg(QString::fromLatin1(PROTONCTX_VERSION));
  QMessageBox::about(parent, QStringLiteral("About protonctx"), text);
}