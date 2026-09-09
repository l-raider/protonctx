#include <QtCore/QTime>
#include <QtGui/QFontDatabase>
#include <QtWidgets/QMenu>
#include <QtWidgets/QPlainTextEdit>

#include "protonctx/src/app_backend.cxxqt.h"

// Build the read-only, scrollable process-log panel.
//
// Every emitted `AppBackend::log_line` is appended as a new block, prefixed
// with a `HH:mm:ss.zzz` timestamp. The block count is capped so a long-running
// session cannot grow memory without bound; the oldest lines are dropped first.
QPlainTextEdit *make_log_panel(QWidget *parent, AppBackend *backend) {
  auto *log = new QPlainTextEdit(parent);
  log->setReadOnly(true);
  log->setMaximumBlockCount(10000);
  log->setLineWrapMode(QPlainTextEdit::NoWrap);
  log->setContextMenuPolicy(Qt::CustomContextMenu);
  // Monospace so timestamps/paths align and are easy to scan.
  log->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));

  QObject::connect(
      backend, &AppBackend::log_line, log, [log](const QString &message) {
        const QString stamp =
            QTime::currentTime().toString(QStringLiteral("HH:mm:ss.zzz"));
        log->appendPlainText(QStringLiteral("[%1] %2").arg(stamp, message));
      });

  // Right-click offers a "Clear logs" action that empties the buffer.
  QObject::connect(log, &QPlainTextEdit::customContextMenuRequested, log,
                   [log](const QPoint &pos) {
                     QMenu menu(log);
                     menu.addAction(QStringLiteral("Clear logs"), log,
                                    [log] { log->clear(); });
                     menu.exec(log->viewport()->mapToGlobal(pos));
                   });

  return log;
}