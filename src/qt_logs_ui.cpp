#include <QtGui/QFontDatabase>
#include <QtWidgets/QMenu>
#include <QtWidgets/QPlainTextEdit>

#include <chrono>
#include <ctime>

#include "protonctx/src/app_backend.cxxqt.h"

namespace {

// Current local time as `HH:mm:ss.zzz`, mirroring
// QTime::toString("HH:mm:ss.zzz").
//
// Implemented with std::chrono (instead of QTime) to avoid `<QtCore/QTime>`,
// which triggers a GCC 16 `-Wsfinae-incomplete` false positive against
// Qt 6.11's own `qchar.h`. `localtime_r` is thread-safe and reads the process's
// local timezone, matching the previous QTime behavior on the GUI thread.
QString local_timestamp() {
  const auto now = std::chrono::system_clock::now();
  const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                      now.time_since_epoch()) %
                  1000;
  const std::time_t secs = std::chrono::system_clock::to_time_t(now);

  std::tm local{};
  localtime_r(&secs, &local);

  return QStringLiteral("%1:%2:%3.%4")
      .arg(local.tm_hour, 2, 10, QLatin1Char('0'))
      .arg(local.tm_min, 2, 10, QLatin1Char('0'))
      .arg(local.tm_sec, 2, 10, QLatin1Char('0'))
      .arg(static_cast<int>(ms.count()), 3, 10, QLatin1Char('0'));
}

} // namespace

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
        log->appendPlainText(
            QStringLiteral("[%1] %2").arg(local_timestamp(), message));
      });

  // Right-click shows the standard menu (Select All, Copy) plus an appended
  // "Clear logs" action that empties the buffer.
  QObject::connect(log, &QPlainTextEdit::customContextMenuRequested, log,
                   [log](const QPoint &pos) {
                     QMenu *menu = log->createStandardContextMenu();
                     menu->addSeparator();
                     menu->addAction(QStringLiteral("Clear logs"), log,
                                     [log] { log->clear(); });
                     menu->exec(log->viewport()->mapToGlobal(pos));
                     delete menu;
                   });

  return log;
}