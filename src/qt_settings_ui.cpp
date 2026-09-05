#include <QtWidgets/QDialog>
#include <QtWidgets/QDialogButtonBox>
#include <QtWidgets/QLabel>
#include <QtWidgets/QVBoxLayout>
#include <QtWidgets/QWidget>

void show_settings_dialog(QWidget *parent) {
  QDialog dialog(parent);
  dialog.setWindowTitle(QStringLiteral("Settings"));
  dialog.setModal(true);

  auto *layout = new QVBoxLayout(&dialog);
  auto *label = new QLabel(QStringLiteral("TODO"), &dialog);
  auto *buttons = new QDialogButtonBox(QDialogButtonBox::Close, &dialog);

  layout->addWidget(label);
  layout->addWidget(buttons);

  QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog,
                   &QDialog::reject);

  dialog.exec();
}