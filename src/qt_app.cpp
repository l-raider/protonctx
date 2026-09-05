#include <memory>

#include <QtWidgets/QAbstractItemView>
#include <QtWidgets/QApplication>
#include <QtWidgets/QDialog>
#include <QtWidgets/QDialogButtonBox>
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
#include <QtGui/QAction>
#include <QtGui/QIcon>
#include <QtGui/QKeySequence>
#include <QtCore/QItemSelectionModel>
#include <QtCore/QPointer>

#include "protonctx/src/app_backend.cxxqt.h"

static int    s_argc    = 1;
static char   s_argv0[] = "protonctx";
static char*  s_argv[]  = { s_argv0, nullptr };

static std::unique_ptr<QApplication> s_app;
static QPointer<QMainWindow> s_main_window;
static QPointer<AppBackend>  s_backend;

namespace {

struct SortState {
    int column = -1;
    Qt::SortOrder order = Qt::AscendingOrder;
};

struct ToolButton {
    const char* label;
    const char* arg;
};

constexpr ToolButton k_tool_buttons[] = {
    {"Explorer", "explorer"},
    {"Registry Editor", "regedit"},
    {"Task Manager", "taskmgr"},
    {"winecfg", "winecfg"},
};

void show_about_dialog(QWidget* parent)
{
    const QString text = QStringLiteral(
        "protonctx %1\n\n"
        "Launch executables inside a Steam game's Proton context.\n\n"
        "Licensed under the GNU GPL v3.")
        .arg(s_backend ? s_backend->getApp_version() : QString());
    QMessageBox::about(parent, QStringLiteral("About protonctx"), text);
}

void show_settings_dialog(QWidget* parent)
{
    QDialog dialog(parent);
    dialog.setWindowTitle(QStringLiteral("Settings"));
    dialog.setModal(true);

    auto* layout = new QVBoxLayout(&dialog);
    auto* label = new QLabel(QStringLiteral("TODO"), &dialog);
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Close, &dialog);

    layout->addWidget(label);
    layout->addWidget(buttons);

    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);

    dialog.exec();
}

} // namespace

extern "C" {
    void qt_app_init()
    {
        if (!s_app) {
            s_app = std::make_unique<QApplication>(s_argc, s_argv);
            s_app->setWindowIcon(QIcon(QStringLiteral(":/icons/icon.svg")));
        }
    }

    int qt_app_exec()
    {
        if (!s_app) {
            return 1;
        }
        const int code = s_app->exec();
        // Destroy QApplication while TLS is still fully valid.
        s_app.reset();
        return code;
    }

    void qt_show_main_window()
    {
        if (s_main_window) {
            s_main_window->show();
            s_main_window->raise();
            s_main_window->activateWindow();
            return;
        }

        s_main_window = new QMainWindow();
        auto* window = s_main_window.data();
        auto* central_widget = new QWidget(window);
        auto* main_layout = new QVBoxLayout(central_widget);
        auto* actions_layout = new QHBoxLayout();
        auto* table_view = new QTableView();
        auto* backend = new AppBackend(central_widget);
        auto* status_label = new QLabel();
        auto* selection_label = new QLabel();
        auto sort_state = std::make_shared<SortState>();

        // Menu bar: Settings and About.
        auto* file_menu = window->menuBar()->addMenu(QStringLiteral("File"));
        auto* settings_action = file_menu->addAction(QStringLiteral("Settings"));
        file_menu->addSeparator();
        auto* exit_action = file_menu->addAction(QStringLiteral("Exit"));
        QObject::connect(exit_action, &QAction::triggered,window, &QWidget::close);
        auto* about_menu = window->menuBar()->addMenu(QStringLiteral("About"));
        auto* about_action = about_menu->addAction(QStringLiteral("About protonctx"));

        main_layout->setContentsMargins(6, 6, 6, 6);
        main_layout->setSpacing(4);
        actions_layout->setSpacing(4);

        // Action row: Browse... plus one button per built-in Wine tool,
        // placed below the table.
        auto* browse_button = new QPushButton(QStringLiteral("Browse..."));
        actions_layout->addWidget(browse_button);

        std::vector<QPushButton*> tool_buttons;
        tool_buttons.reserve(std::size(k_tool_buttons));
        for (const auto& tool : k_tool_buttons) {
            auto* button = new QPushButton(QString::fromLatin1(tool.label));
            actions_layout->addWidget(button);
            tool_buttons.push_back(button);
        }
        actions_layout->addStretch();

        status_label->setTextInteractionFlags(Qt::TextSelectableByMouse);
        selection_label->setTextInteractionFlags(Qt::TextSelectableByMouse);

        table_view->setModel(backend);
        table_view->setSelectionBehavior(QAbstractItemView::SelectRows);
        table_view->setSelectionMode(QAbstractItemView::SingleSelection);
        table_view->setAlternatingRowColors(true);
        table_view->horizontalHeader()->setSectionResizeMode(QHeaderView::Interactive);
        table_view->horizontalHeader()->setStretchLastSection(true);
        table_view->horizontalHeader()->setSectionsClickable(true);
        table_view->horizontalHeader()->setSortIndicatorShown(true);
        table_view->horizontalHeader()->setSortIndicator(-1, Qt::AscendingOrder);
        table_view->verticalHeader()->setVisible(false);
        table_view->verticalHeader()->setDefaultSectionSize(28);

        // Table first, action buttons below it.
        main_layout->addWidget(table_view, 1);
        main_layout->addLayout(actions_layout);

        window->statusBar()->addWidget(status_label, 1);
        window->statusBar()->addPermanentWidget(selection_label);

        // Enable the launch buttons only when a game is selected.
        const auto sync_action_state = [backend, browse_button, tool_buttons]() {
            const bool has_selection = backend->getSelected_row() >= 0;
            browse_button->setEnabled(has_selection);
            for (auto* button : tool_buttons) {
                button->setEnabled(has_selection);
            }
        };

        // Selection -> backend.selected_row -> button state + selection label.
        QObject::connect(
            table_view->selectionModel(),
            &QItemSelectionModel::currentRowChanged,
            table_view,
            [backend, selection_label, sync_action_state](const QModelIndex& current, const QModelIndex&) {
                const int row = current.isValid() ? current.row() : -1;
                backend->select_row(row);
                selection_label->setText(row >= 0 ? QStringLiteral("Selected: %1").arg(row + 1) : QString());
                sync_action_state();
            });

        // Sort on header click, toggling asc/desc on repeat clicks.
        QObject::connect(
            table_view->horizontalHeader(),
            &QHeaderView::sectionClicked,
            table_view,
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
                table_view->horizontalHeader()->setSortIndicator(sort_state->column, sort_state->order);
            });

        // Status text + model resets (sorting/loading) clear selection.
        QObject::connect(backend, &AppBackend::status_textChanged, central_widget, [status_label, backend]() {
            status_label->setText(backend->getStatus_text());
        });
        QObject::connect(backend, &AppBackend::selected_rowChanged, central_widget, sync_action_state);
        QObject::connect(backend, &QAbstractItemModel::modelReset, central_widget, sync_action_state);

        // Launch errors surfaced as a message box.
        QObject::connect(backend, &AppBackend::launch_failed, window, [window](const QString& message) {
            QMessageBox::critical(window, QStringLiteral("Launch Error"), message);
        });

        // Browse...: pick a Windows executable and run it in the selected prefix.
        QObject::connect(browse_button, &QPushButton::clicked, window, [window, backend](bool) {
            const QString path = QFileDialog::getOpenFileName(
                window,
                QStringLiteral("Select executable to run"),
                QString(),
                QStringLiteral("Windows executables (*.exe *.EXE *.bat *.cmd);;All Files (*)"));
            if (!path.isEmpty()) {
                backend->browse_exe(path);
            }
        });

        // Each tool button launches its Wine built-in in the selected prefix.
        for (std::size_t i = 0; i < tool_buttons.size(); ++i) {
            const QString arg = QString::fromLatin1(k_tool_buttons[i].arg);
            QObject::connect(tool_buttons[i], &QPushButton::clicked, backend, [backend, arg](bool) {
                backend->launch_tool(arg);
            });
        }

        QObject::connect(settings_action, &QAction::triggered, window, [window](bool) {
            show_settings_dialog(window);
        });
        QObject::connect(about_action, &QAction::triggered, window, [window](bool) {
            show_about_dialog(window);
        });

        s_backend = backend;

        window->setWindowTitle(QStringLiteral("protonctx"));
        window->resize(760, 520);
        window->setCentralWidget(central_widget);

        sync_action_state();
        status_label->setText(backend->getStatus_text());
        window->show();
    }

    void qt_load_games()
    {
        if (s_backend) {
            s_backend->load_games();
        }
    }
}
