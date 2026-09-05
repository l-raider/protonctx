#include <QtCore/QItemSelectionModel>
#include <QtCore/QPointer>
#include <QtGui/QAction>
#include <QtGui/QClipboard>
#include <QtGui/QGuiApplication>
#include <QtGui/QIcon>
#include <QtWidgets/QAbstractItemView>
#include <QtWidgets/QApplication>
#include <QtWidgets/QFileDialog>
#include <QtWidgets/QHBoxLayout>
#include <QtWidgets/QHeaderView>
#include <QtWidgets/QLabel>
#include <QtWidgets/QMainWindow>
#include <QtWidgets/QMenu>
#include <QtWidgets/QMenuBar>
#include <QtWidgets/QMessageBox>
#include <QtWidgets/QPushButton>
#include <QtWidgets/QStatusBar>
#include <QtWidgets/QTableView>
#include <QtWidgets/QVBoxLayout>
#include <QtWidgets/QWidget>

#include <functional>
#include <memory>
#include <vector>

#include "protonctx/src/app_backend.cxxqt.h"

// Dialogs live in their own translation units; declared here so the menu
// actions in wire_signals() can call them.
void show_about_dialog(QWidget *parent);
void show_settings_dialog(QWidget *parent);

static int s_argc = 1;
static char s_argv0[] = "protonctx";
static char *s_argv[] = {s_argv0, nullptr};

// clazy:excludeall=non-pod-global-static
// The Qt app object must outlive the individual `extern "C"` calls from Rust
// (main.rs drives init/show/load/exec as separate FFI calls), so it is kept in
// static storage. `s_app` is destroyed explicitly in qt_app_exec() while TLS is
// still valid; the QPointers are non-owning and self-null when their targets
// die.
static std::unique_ptr<QApplication> s_app;
static QPointer<QMainWindow> s_main_window;
static QPointer<AppBackend> s_backend;

namespace {

// Default view: sorted by Game name, ascending, matching `load_games()`.
struct SortState {
  int column = 0;
  Qt::SortOrder order = Qt::AscendingOrder;
};

struct ToolButton {
  const char *label;
  const char *tool_id;
};

constexpr ToolButton k_tool_buttons[] = {
    {"Explorer", "explorer"},
    {"Registry Editor", "regedit"},
    {"Task Manager", "taskmgr"},
    {"Wine Configuration", "winecfg"},
};

// Build the File/About menus and wire Exit. Returns the Settings and About
// actions so their `triggered` signals can be wired later.
void setup_menu_bar(QMainWindow *window, QAction **settings_action,
                    QAction **about_action) {
  auto *file_menu = window->menuBar()->addMenu(QStringLiteral("File"));
  *settings_action = file_menu->addAction(QStringLiteral("Settings"));
  file_menu->addSeparator();
  auto *exit_action = file_menu->addAction(QStringLiteral("Exit"));
  QObject::connect(exit_action, &QAction::triggered, window, &QWidget::close);
  auto *about_menu = window->menuBar()->addMenu(QStringLiteral("About"));
  *about_action = about_menu->addAction(QStringLiteral("About protonctx"));
}

// Configure the table view's selection and header behavior.
void setup_table(QTableView *table_view) {
  table_view->setSelectionBehavior(QAbstractItemView::SelectRows);
  table_view->setSelectionMode(QAbstractItemView::SingleSelection);
  table_view->setAlternatingRowColors(true);
  table_view->horizontalHeader()->setSectionResizeMode(
      QHeaderView::Interactive);
  table_view->horizontalHeader()->setStretchLastSection(true);
  table_view->horizontalHeader()->setSectionsClickable(true);
  table_view->horizontalHeader()->setSortIndicatorShown(true);
  // Game column starts sorted ascending; the model sorts the same way on load.
  table_view->horizontalHeader()->setSortIndicator(0, Qt::AscendingOrder);
  table_view->verticalHeader()->setVisible(false);
  table_view->verticalHeader()->setDefaultSectionSize(28);
  // Right-click pops a context menu (see setup_context_menu).
  table_view->setContextMenuPolicy(Qt::CustomContextMenu);
}

// Right-click context menu for a table row. Mirrors the action-row buttons
// (Browse... + the built-in Wine tools) plus "Copy compatdata path", which
// has no button. Selecting a tool launches it in the row's prefix, exactly like
// the corresponding button would.
void setup_context_menu(QTableView *table_view, AppBackend *backend,
                        QPushButton *browse_button) {
  QObject::connect(
      table_view, &QTableView::customContextMenuRequested, table_view,
      [table_view, backend, browse_button](const QPoint &pos) {
        const QModelIndex index = table_view->indexAt(pos);
        if (!index.isValid()) {
          return;
        }
        // Right-clicking selects the row, so the menu acts on it.
        table_view->setCurrentIndex(index);

        QMenu menu(table_view);
        menu.addAction(QStringLiteral("Browse for executable..."), &menu,
                       [browse_button] { browse_button->click(); });
        menu.addSeparator();
        for (const auto &tool : k_tool_buttons) {
          const QString tool_id = QString::fromLatin1(tool.tool_id);
          menu.addAction(QString::fromLatin1(tool.label), &menu,
                         [backend, tool_id] { backend->launch_tool(tool_id); });
        }
        menu.addSeparator();
        menu.addAction(
            QStringLiteral("Copy compatdata path"), &menu, [backend, index] {
              const QString path = backend->compatDataPath(index.row());
              if (!path.isEmpty()) {
                QGuiApplication::clipboard()->setText(path);
              }
            });
        menu.addAction(QStringLiteral("Copy compatibility tool path"), &menu,
                       [backend, index] {
                         const QString path =
                             backend->protonDirPath(index.row());
                         if (!path.isEmpty()) {
                           QGuiApplication::clipboard()->setText(path);
                         }
                       });
        menu.exec(table_view->viewport()->mapToGlobal(pos));
      });
}

// Build the action row (Browse... + one button per built-in Wine tool) and wire
// each tool button to `backend->launch_tool` in the same pass, so the label and
// handler can never drift out of sync.
void build_action_row(QHBoxLayout *actions_layout, AppBackend *backend,
                      QPushButton **browse_button,
                      std::vector<QPushButton *> *tool_buttons) {
  *browse_button = new QPushButton(QStringLiteral("Browse..."));
  actions_layout->addWidget(*browse_button);

  tool_buttons->reserve(std::size(k_tool_buttons));
  for (const auto &tool : k_tool_buttons) {
    auto *button = new QPushButton(QString::fromLatin1(tool.label));
    actions_layout->addWidget(button);
    tool_buttons->push_back(button);
    const QString tool_id = QString::fromLatin1(tool.tool_id);
    QObject::connect(
        button, &QPushButton::clicked, backend,
        [backend, tool_id](bool) { backend->launch_tool(tool_id); });
  }
  actions_layout->addStretch();
}

// Wire all remaining signals (selection, sorting, status, resets, launch
// errors, browse, and the Settings/About actions).
void wire_signals(QMainWindow *window, QWidget *central_widget,
                  QTableView *table_view, AppBackend *backend,
                  QLabel *status_label, QLabel *selection_label,
                  QPushButton *browse_button,
                  const std::shared_ptr<SortState> &sort_state,
                  QAction *settings_action, QAction *about_action,
                  const std::function<void()> &sync_action_state) {
  // Selection -> backend.selected_row -> button state + selection label.
  QObject::connect(
      table_view->selectionModel(), &QItemSelectionModel::currentRowChanged,
      table_view,
      [backend, selection_label, sync_action_state](const QModelIndex &current,
                                                    const QModelIndex &) {
        const int row = current.isValid() ? current.row() : -1;
        backend->select_row(row);
        if (row >= 0) {
          const uint app_id = backend->selectedAppId(row);
          selection_label->setText(QStringLiteral("Selected: %1").arg(app_id));
        } else {
          selection_label->clear();
        }
        sync_action_state();
      });

  // Sort on header click, toggling asc/desc on repeat clicks.
  QObject::connect(
      table_view->horizontalHeader(), &QHeaderView::sectionClicked, table_view,
      [table_view, backend, sort_state](int section) {
        if (sort_state->column == section) {
          sort_state->order = sort_state->order == Qt::AscendingOrder
                                  ? Qt::DescendingOrder
                                  : Qt::AscendingOrder;
        } else {
          sort_state->column = section;
          sort_state->order = Qt::AscendingOrder;
        }
        backend->sort_by(section, sort_state->order == Qt::AscendingOrder);
        table_view->horizontalHeader()->setSortIndicator(sort_state->column,
                                                         sort_state->order);
      });

  // Status text + model resets (sorting/loading) clear selection.
  QObject::connect(backend, &AppBackend::status_textChanged, central_widget,
                   [status_label, backend]() {
                     status_label->setText(backend->getStatus_text());
                   });
  QObject::connect(backend, &AppBackend::selected_rowChanged, central_widget,
                   sync_action_state);
  // On load/sort reset, re-fit the Game column so long names stay visible;
  // the column stays Interactive so the user can still drag it.
  QObject::connect(backend, &QAbstractItemModel::modelReset, central_widget,
                   [table_view, sync_action_state]() {
                     table_view->resizeColumnToContents(0);
                     // A model reset clears the selection, but when the window
                     // next gains focus QAbstractItemView::focusInEvent
                     // auto-selects row 0 if no current index is set. Mark the
                     // current index as deliberately unset so no row is
                     // selected by default.
                     table_view->setCurrentIndex(QModelIndex());
                     table_view->selectionModel()->clearSelection();
                     sync_action_state();
                   });

  // Launch errors surfaced as a message box.
  QObject::connect(backend, &AppBackend::launch_failed, window,
                   [window](const QString &message) {
                     QMessageBox::critical(
                         window, QStringLiteral("Launch Error"), message);
                   });

  // Browse...: pick a Windows executable and run it in the selected prefix.
  QObject::connect(
      browse_button, &QPushButton::clicked, window, [window, backend](bool) {
        const QString path = QFileDialog::getOpenFileName(
            window, QStringLiteral("Select executable to run"), QString(),
            QStringLiteral("Windows executables (*.exe *.EXE *.bat *.cmd);;All "
                           "Files (*)"));
        if (!path.isEmpty()) {
          backend->browse_exe(path);
        }
      });

  QObject::connect(settings_action, &QAction::triggered, window,
                   [window](bool) { show_settings_dialog(window); });
  QObject::connect(about_action, &QAction::triggered, window,
                   [window](bool) { show_about_dialog(window); });
}

} // namespace

extern "C" {
void qt_app_init() {
  if (!s_app) {
    s_app = std::make_unique<QApplication>(s_argc, s_argv);
    s_app->setWindowIcon(QIcon(QStringLiteral(":/icons/icon.svg")));
  }
}

int qt_app_exec() {
  if (!s_app) {
    return 1;
  }
  const int code = s_app->exec();
  // Destroy QApplication while TLS is still fully valid.
  s_app.reset();
  return code;
}

void qt_show_main_window() {
  if (s_main_window) {
    s_main_window->show();
    s_main_window->raise();
    s_main_window->activateWindow();
    return;
  }

  s_main_window = new QMainWindow();
  auto *window = s_main_window.data();
  auto *central_widget = new QWidget(window);
  auto *main_layout = new QVBoxLayout(central_widget);
  auto *actions_layout = new QHBoxLayout();
  auto *table_view = new QTableView();
  auto *backend = new AppBackend(central_widget);
  auto *status_label = new QLabel();
  auto *selection_label = new QLabel();
  auto sort_state = std::make_shared<SortState>();

  QAction *settings_action = nullptr;
  QAction *about_action = nullptr;
  setup_menu_bar(window, &settings_action, &about_action);

  main_layout->setContentsMargins(6, 6, 6, 6);
  main_layout->setSpacing(4);
  actions_layout->setSpacing(4);

  QPushButton *browse_button = nullptr;
  std::vector<QPushButton *> tool_buttons;
  build_action_row(actions_layout, backend, &browse_button, &tool_buttons);

  status_label->setTextInteractionFlags(Qt::TextSelectableByMouse);
  selection_label->setTextInteractionFlags(Qt::TextSelectableByMouse);

  table_view->setModel(backend);
  setup_table(table_view);
  setup_context_menu(table_view, backend, browse_button);

  // Table first, action buttons below it.
  main_layout->addWidget(table_view, 1);
  main_layout->addLayout(actions_layout);

  window->statusBar()->addWidget(status_label, 1);
  window->statusBar()->addPermanentWidget(selection_label);

  // Enable the launch buttons only when a game is selected.
  const auto sync_action_state = [backend, browse_button, tool_buttons]() {
    const bool has_selection = backend->getSelected_row() >= 0;
    browse_button->setEnabled(has_selection);
    for (auto *button : tool_buttons) {
      button->setEnabled(has_selection);
    }
  };

  wire_signals(window, central_widget, table_view, backend, status_label,
               selection_label, browse_button, sort_state, settings_action,
               about_action, sync_action_state);

  s_backend = backend;

  // PROTONCTX_VERSION is defined by build.rs from CARGO_PKG_VERSION (Cargo.toml
  // is the single source of truth for the version).
#ifndef PROTONCTX_VERSION
#error "PROTONCTX_VERSION must be defined by build.rs (from Cargo.toml)"
#endif
  window->setWindowTitle(QStringLiteral("protonctx v%1")
                             .arg(QString::fromLatin1(PROTONCTX_VERSION)));
  window->resize(760, 520);
  window->setCentralWidget(central_widget);
  // Delete the window (and its child model) when it closes, so teardown happens
  // before QApplication destruction rather than during s_app.reset().
  window->setAttribute(Qt::WA_DeleteOnClose);

  sync_action_state();
  status_label->setText(backend->getStatus_text());
  window->show();
}

void qt_load_games() {
  if (s_main_window && s_backend) {
    s_backend->load_games();
  }
}
}
