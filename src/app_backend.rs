//! cxx-qt bridge exposing the Steam games table as a `QAbstractTableModel`.
//!
//! The model is consumed by `src/qt_app.cpp`, which builds a native Qt Widgets
//! `QMainWindow`. All
//! Steam discovery and Proton launching is delegated to `crate::steam` and
//! `crate::launcher`, which are reused unchanged from the earlier Slint implementation.

// cxx-qt's generated code attaches `#[automatically_derived]` to inherent impl blocks, which newer rustc warns about.
#![allow(unused_attributes)]

use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{
    Orientation, QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant,
};

use crate::launcher;
use crate::models::Game;

// Qt::DisplayRole — the only role this model reports.
const ROLE_DISPLAY: i32 = 0;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!("QtCore/QAbstractTableModel");
        #[qobject]
        type QAbstractTableModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qt.h");
        #[namespace = "Qt"]
        type Orientation = cxx_qt_lib::Orientation;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[base = QAbstractTableModel]
        #[qproperty(i32, selected_row)]
        #[qproperty(QString, status_text)]
        #[qproperty(QString, app_version)]
        type AppBackend = super::AppBackendRust;

        // QAbstractTableModel overrides.
        #[cxx_override]
        fn data(self: &AppBackend, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &AppBackend, parent: &QModelIndex) -> i32;
        #[cxx_override]
        #[cxx_name = "columnCount"]
        fn column_count(self: &AppBackend, parent: &QModelIndex) -> i32;
        #[cxx_override]
        #[cxx_name = "headerData"]
        fn header_data(
            self: &AppBackend,
            section: i32,
            orientation: Orientation,
            role: i32,
        ) -> QVariant;
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &AppBackend) -> QHash_i32_QByteArray;

        // Inherited model mutation methods (reset is enough: games are loaded once
        // and re-sorted in place).
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut AppBackend>);
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut AppBackend>);

        // Error signal emitted when a launch fails; wired to a QMessageBox in C++.
        #[qsignal]
        fn launch_failed(self: Pin<&mut AppBackend>, message: &QString);

        // Invokables called from the C++ UI.
        #[qinvokable]
        fn load_games(self: Pin<&mut AppBackend>);
        #[qinvokable]
        fn select_row(self: Pin<&mut AppBackend>, row: i32);
        #[qinvokable]
        fn sort_by(self: Pin<&mut AppBackend>, column: i32, ascending: bool);
        #[qinvokable]
        fn launch_tool(self: Pin<&mut AppBackend>, arg: &QString);
        #[qinvokable]
        fn browse_exe(self: Pin<&mut AppBackend>, path: &QString);
    }

    impl cxx_qt::Threading for AppBackend {}
}

/// Rust-side state backing the `AppBackend` qobject.
pub struct AppBackendRust {
    /// Installed Steam games, in display (sorted) order.
    games: Vec<Game>,
    /// Index into `games` of the selected row, or `-1`.
    selected_row: i32,
    /// Status-bar text.
    status_text: QString,
    /// Application version, shown in the About dialog.
    app_version: QString,
}

impl Default for AppBackendRust {
    fn default() -> Self {
        Self {
            games: Vec::new(),
            selected_row: -1,
            status_text: QString::from("Ready"),
            app_version: QString::from(env!("CARGO_PKG_VERSION")),
        }
    }
}

impl qobject::AppBackend {
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            return 0;
        }
        self.rust().games.len() as i32
    }

    fn column_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            return 0;
        }
        // Game | App ID | Compatibility Tool
        3
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        if !index.is_valid() {
            return QVariant::default();
        }
        let row = index.row() as usize;
        let games = &self.rust().games;
        let Some(game) = games.get(row) else {
            return QVariant::default();
        };

        if role != ROLE_DISPLAY {
            return QVariant::default();
        }

        let text = match index.column() {
            0 => game.name.clone(),
            1 => game.app_id.to_string(),
            2 => display_compat_tool(game),
            _ => return QVariant::default(),
        };
        QVariant::from(&QString::from(&text))
    }

    fn header_data(&self, section: i32, orientation: Orientation, role: i32) -> QVariant {
        if role != ROLE_DISPLAY || section < 0 {
            return QVariant::default();
        }

        if orientation == Orientation::Vertical {
            return QVariant::from(&(section + 1));
        }

        let label = match section {
            0 => Some(QString::from("Game")),
            1 => Some(QString::from("App ID")),
            2 => Some(QString::from("Compatibility Tool")),
            _ => None,
        };
        label.map(|text| QVariant::from(&text)).unwrap_or_default()
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut map = QHash::<QHashPair_i32_QByteArray>::default();
        map.insert(ROLE_DISPLAY, QByteArray::from("display"));
        map
    }

    fn load_games(mut self: Pin<&mut Self>) {
        let games = crate::steam::discover_games();
        let count = games.len();

        unsafe {
            self.as_mut().begin_reset_model();
        }
        self.as_mut().rust_mut().games = games;
        unsafe {
            self.as_mut().end_reset_model();
        }

        self.as_mut().set_selected_row(-1);
        let status = if count == 1 {
            "1 game loaded".to_string()
        } else {
            format!("{count} games loaded")
        };
        self.as_mut().set_status_text(QString::from(&status));
    }

    fn select_row(mut self: Pin<&mut Self>, row: i32) {
        self.as_mut().set_selected_row(row);
    }

    fn sort_by(mut self: Pin<&mut Self>, column: i32, ascending: bool) {
        let column_count = self.column_count(&QModelIndex::default());
        if column < 0 || column >= column_count {
            return;
        }

        let col = column as usize;
        unsafe {
            self.as_mut().begin_reset_model();
        }
        self.as_mut().rust_mut().games.sort_by(|a, b| {
            let a_val = sort_key(a, col);
            let b_val = sort_key(b, col);
            if ascending {
                a_val.cmp(&b_val)
            } else {
                b_val.cmp(&a_val)
            }
        });
        unsafe {
            self.as_mut().end_reset_model();
        }
        self.as_mut().set_selected_row(-1);
    }

    fn launch_tool(mut self: Pin<&mut Self>, arg: &QString) {
        let Some(game) = self.rust().selected_game() else {
            return;
        };

        let tool = arg.to_string();
        if let Err(e) = launcher::launch_tool(game, &tool) {
            let msg = QString::from(&format!("Failed to launch {tool}: {e}"));
            self.as_mut().launch_failed(&msg);
        }
    }

    fn browse_exe(mut self: Pin<&mut Self>, path: &QString) {
        let Some(game) = self.rust().selected_game() else {
            return;
        };

        let path = path.to_string();
        if let Err(e) = launcher::proton::run_in_prefix(game, &[&path]) {
            let msg = QString::from(&format!("Failed to launch: {e}"));
            self.as_mut().launch_failed(&msg);
        }
    }
}

impl AppBackendRust {
    /// The currently selected [`Game`], if any.
    fn selected_game(&self) -> Option<&Game> {
        let idx = self.selected_row;
        if idx < 0 {
            return None;
        }
        self.games.get(idx as usize)
    }
}

/// The sort key for a given column index (0 = game, 1 = app id, 2 = compat tool).
fn sort_key(game: &Game, column: usize) -> String {
    match column {
        0 => game.name.to_lowercase(),
        1 => format!("{:010}", game.app_id),
        2 => display_compat_tool(game).to_lowercase(),
        _ => String::new(),
    }
}

/// The value shown in the "Compatibility Tool" column.
///
/// When Steam has an explicit mapping (per-app `CompatToolMapping`), show that name.
/// Otherwise fall back to a short description of the resolved Proton directory, or a
/// generic "(default)" when nothing is known.
fn display_compat_tool(game: &Game) -> String {
    if !game.compat_tool.is_empty() {
        return game.compat_tool.clone();
    }

    if !game.proton_dir.is_empty() {
        if let Some(name) = std::path::Path::new(&game.proton_dir)
            .file_name()
            .and_then(|n| n.to_str())
        {
            return format!("{name} (default)");
        }
    }

    "(default)".to_string()
}
