#include <QtWidgets/QCheckBox>
#include <QtWidgets/QDialog>
#include <QtWidgets/QDialogButtonBox>
#include <QtWidgets/QVBoxLayout>
#include <QtWidgets/QWidget>

#include "protonctx/src/app_backend.cxxqt.h"

void show_settings_dialog(QWidget *parent, AppBackend *backend) {
  QDialog dialog(parent);
  dialog.setWindowTitle(QStringLiteral("Settings"));
  dialog.setModal(true);

  auto *layout = new QVBoxLayout(&dialog);
  auto *remember_box =
      new QCheckBox(QStringLiteral("Remember last used directory"), &dialog);
  auto *buttons = new QDialogButtonBox(QDialogButtonBox::Close, &dialog);

  // Initialize from the persisted preference first, then connect the signal:
  // setChecked() emits `toggled`, which would otherwise trigger an immediate
  // (redundant) config write — and write `false` if the stored value differed —
  // before the user has even interacted with the dialog.
  remember_box->setChecked(backend->getRemember_last_dir());
  QObject::connect(
      remember_box, &QCheckBox::toggled, &dialog,
      [backend](bool checked) { backend->applyRememberLastDir(checked); });

  layout->addWidget(remember_box);
  layout->addWidget(buttons);

  QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog,
                   &QDialog::reject);

  dialog.exec();
}