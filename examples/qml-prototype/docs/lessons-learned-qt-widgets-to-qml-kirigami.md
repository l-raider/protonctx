# Lessons Learned: Porting QtWidgets Interfaces to QML and Kirigami

A standalone onboarding and reference guide derived from porting the **protonctx** QtWidgets UI
(`qt_main_ui.cpp`, `qt_logs_ui.cpp`, `qt_settings_ui.cpp`, `qt_about_ui.cpp`) to a **Kirigami/QML**
prototype (`ktest/src/Main.qml` + `ktest/src/main.rs`, cxx-qt).

The guide focuses on the concrete, hard-won details: exact theme formulas, API traps, layout
translations, and the verification workflow that made pixel-level parity possible.

---

## Table of Contents

1. [Executive Summary & Core Architectural Differences](#1-executive-summary--core-architectural-differences)
2. [Component Mapping & Direct Equivalents](#2-component-mapping--direct-equivalents)
3. [State Management & Data Binding](#3-state-management--data-binding)
4. [Layouts, Sizing, and Responsive UI](#4-layouts-sizing-and-responsive-ui)
5. [Styling, Theming, and Platform Integration](#5-styling-theming-and-platform-integration)
6. [Common Pitfalls, Anti-Patterns, and Debugging Tips](#6-common-pitfalls-anti-patterns-and-debugging-tips)
7. [Actionable Migration Checklist](#7-actionable-migration-checklist)
8. [Reference Sources and Links](#8-reference-sources-and-links)

---

## 1. Executive Summary & Core Architectural Differences

### Imperative vs. declarative

| QtWidgets (protonctx) | QML / Kirigami (ktest) |
|---|---|
| Widget tree built in C++ constructors | Declarative object tree with property bindings |
| State pushed via signal/slot and `set*` calls | State expressed as bindings; the runtime pushes changes |
| Explicit `resizeEvent`/layout code | Layout items (`RowLayout`, `ColumnLayout`, `SplitView`) recompute automatically |
| `QStyle`/`QPalette` own the look | Style plugin (`org.kde.desktop`) + `Kirigami.Theme` own the look |
| A widget's size is set once (or in `resizeEvent`) | Sizes are bounds, not values: `Layout.fillWidth`, `anchors`, `implicitWidth/Height` |
| Context menus, sorting, selection are widget features | Often must be assembled from primitives (`Menu`, handlers, proxies) |

### The stack you are actually running

A KDE QML app is a four-layer stack, and theming issues are almost always caused by one layer
not being what you assumed:

```
Kirigami (controls + platform theme: Kirigami.Theme, Units, Dialogs, Page)
   ^
QQC2 style plugin: org.kde.desktop  (kf6-qqc2-desktop-style)
   ^
Qt Quick Controls 2 (Templates + fallback Basic style)
   ^
Qt 6 (QtQuick, QtQuick.Layouts, Qt.labs.qmlmodels, ...)
   ^
KDE platform theme plugin (plasma-integration) -> palette, fonts, icons, KColorScheme
```

- If `QQuickStyle` is not set, QQC2 falls back to **Basic/Fusion**, not Breeze. Everything renders,
  but it does not look like a KDE app.
- `Kirigami.Theme` reads the **KColorScheme** (`kdeglobals`) through the Kirigami platform plugin,
  so theme colors work even where `QPalette` does not (e.g., offscreen platform runs).
- `QPalette` is only reliable in a real Plasma session; under the offscreen platform it was light
  (`palette.mid == #b8b8b8`), which silently breaks any code that uses `palette.*`.

### Effort distribution observed in this port

- ~20% translating structure (menus, splitter, table, status bar).
- ~50% matching the theme and pixel-level visuals (frame color, hover blend, grid lines,
  scrollbars, fonts, corner radius).
- ~30% behavioral edge cases (selection, context menus, sorting, hit areas, auto-scroll).

### The single most valuable habit

When a visual detail does not match, **read the upstream source that draws it** and copy its
formula. Three examples from this session:

- Breeze item-view hover: `kstyle/breezestyle.cpp`, `drawPanelItemViewItemPrimitive()`.
- Kirigami color math: `src/platform/colorutils.cpp` (`linearInterpolation` is HSV-based!).
- Qt header font weight: `qheaderview.cpp` uses `fnt.setBold(true)` for size hints.

---

## 2. Component Mapping & Direct Equivalents

### Widget-to-QML map

| QtWidgets | QML / Kirigami | Notes |
|---|---|---|
| `QMainWindow` | `Kirigami.ApplicationWindow` | Provides `pageStack`, `globalDrawer`, `contextDrawer`; inherits `QQC2.ApplicationWindow` (so `menuBar` exists) |
| `QMenuBar` | `Controls.MenuBar` in the window `menuBar` property, or a custom `page.header` | `org.kde.desktop` styles `MenuBar` with the Header color set; no QML global-menu (appmenu) integration |
| `QMenu` | `Controls.Menu` + `Controls.MenuItem` / `MenuSeparator` | Context menus: `menu.popup(item, point)`; note `Menu`/`MenuItem` are in QtQuick.Controls |
| `QToolBar` | `Controls.ToolBar` inside `page.header`, or `page.actions` | `page.actions` show trailing in the global toolbar; a custom header gives exact placement |
| `QAction` | `Controls.Action` / `Kirigami.Action` | `Kirigami.Action` is drawer-oriented (supports `children`, `separator`); use `Controls.MenuItem` inside `Controls.Menu` |
| `QTableView` | `TableView` (**QtQuick**, not Controls) + model | `TableViewColumn` does **not** exist in Qt 6 |
| `QHeaderView` | Custom `Item` + `Repeater`, or `Controls.HorizontalHeaderView` | See pitfalls; the custom header is the reliable path for sort/resize |
| `QAbstractTableModel` | `TableModel` (`Qt.labs.qmlmodels`), `ListModel`, or a cxx-qt `QAbstractTableModel` | `TableModel` supports multiple columns and roles; `ListModel` is single-column |
| `QSortFilterProxyModel` | `KSortFilterProxyModel` (`org.kde.kitemodels`) | QML-friendly: `sortRoleName`, `sortColumn`, `sortOrder`, `filterString` |
| `QItemSelectionModel` | `ItemSelectionModel` (`QtQml.Models`) | Attach with `selectionModel:` and `selectionBehavior: TableView.SelectRows` |
| `QSplitter` | `Controls.SplitView` | Sizes via `Controls.SplitView.fillHeight/preferredHeight/minimumHeight` |
| `QStatusBar` | `page.footer: Controls.ToolBar` | Height must be constrained explicitly (see §4) |
| `QProgressBar` (busy) | `Controls.BusyIndicator` / `Controls.ProgressBar` | Reference used an indeterminate bar (`range 0..0`) |
| `QDialog` | `Kirigami.Dialog` | `standardButtons: Kirigami.Dialog.Close`, custom `dialogData` |
| `QMessageBox` | `Kirigami.PromptDialog` | `dialogType: Kirigami.PromptDialog.Error`, `subtitle` |
| `QFileDialog` | `QtQuick.Dialogs.FileDialog` | Not wired in the prototype |
| `QPlainTextEdit` (log) | `TextEdit` (QtQuick) inside `Flickable` | ScrollView + TextEdit has hit-area problems; see §4 |
| `QLabel` | `Controls.Label` | Use theme fonts, never hardcode sizes |
| `QCheckBox` | `Controls.CheckBox` | Settings dialog |

### Types that live in QtQuick, not QtQuick.Controls

With `import QtQuick.Controls as Controls`, these are **unqualified** (`QtQuick` is imported plain):

- `TableView` (the view itself), `TextEdit`, `MouseArea`, `TapHandler`, `HoverHandler`,
  `Shortcut`, `Flickable`, `Item`, `Rectangle`.

These require the `Controls.` prefix:

- `Controls.Button`, `ToolButton`, `Label`, `CheckBox`, `Menu`, `MenuItem`, `MenuSeparator`,
  `ToolBar`, `ScrollBar`, `ScrollView`, `SplitView`, `BusyIndicator`, `TableViewDelegate`,
  `HorizontalHeaderView`, `AbstractButton` (for enum values such as `AbstractButton.TextBesideIcon`).

> **Attached properties follow the import alias.** With `import QtQuick.Controls as Controls`,
> write `Controls.SplitView.fillHeight`, `Controls.ScrollBar.vertical`,
> `Controls.SplitView.preferredHeight` — plain `SplitView.fillHeight` is a load-time error:
> `Non-existent attached object`.

### `Kirigami.Action` vs `Controls.MenuItem`

- `GlobalDrawer`/`Kirigami` drawers use `Kirigami.Action` (nested children form submenus,
  `separator: true`, `shortcut: "F5"`).
- `Controls.Menu` should use `Controls.MenuItem`/`Controls.Action`; `Kirigami.Action` is not the
  right type there.

---

## 3. State Management & Data Binding

### Prototype data: `TableModel` (multi-column) vs `ListModel`

`ListModel` is single-column; a three-column table needs `TableModel` with one
`TableModelColumn` per column, or a C++ model. Static prototype data:

```qml
import Qt.labs.qmlmodels

TableModel {
    id: gamesModel

    TableModelColumn { display: "name" }
    TableModelColumn { display: "appId" }
    TableModelColumn { display: "compatTool" }

    rows: [
        { name: "Broforce", appId: 274190, compatTool: "GE-Proton10-34" },
        { name: "Castle Crashers", appId: 204360, compatTool: "proton_hotfix" },
        { name: "Raptor: Call of The Shadows - 2015 Edition", appId: 336060, compatTool: "proton_experimental" }
    ]
}
```

- `TableModel.getRow(row)` returns the row object (the old `get()` is deprecated).
- The delegate sees `model.display` for the cell's column value.

### Sorting: there is no `sortable` / `setSortingEnabled` in QML

Verified against the installed Qt 6.11 type info:

- `QQuickTableView` and `QQuickHeaderViewBase` expose **no** sort API and **no** header-clicked
  signal; `TableViewColumn` does not exist.
- `Qt.labs.qmlmodels.TableModel` has no `sort()` (only `appendRow`, `getRow`, `setRow`,
  `removeRow`, `moveRow`, `clear`).
- QtWidgets' `setSortingEnabled(true)` is only glue: it connects header clicks to
  `model.sort(column, order)`; the model still implements sorting.

The KDE-native "built-in" is `org.kde.kitemodels`' `KSortFilterProxyModel` (a
`QSortFilterProxyModel`). It works directly with a QML `TableModel` — including numeric sorts when
a numeric role is named — and the header click remains your job:

```qml
import org.kde.kitemodels as KItemModels

readonly property int sortColumn: 0
property bool sortAscending: true
property var sortRoleNames: ["name", "appId", "compatTool"]

KItemModels.KSortFilterProxyModel {
    id: gamesProxy
    sourceModel: gamesModel
    sortRoleName: root.sortRoleNames[root.sortColumn]
    sortOrder: root.sortAscending ? Qt.AscendingOrder : Qt.DescendingOrder
    sortColumn: root.sortColumn
}

// header cell:
TapHandler { onTapped: page.toggleSort(headerCell.index) }
```

- `sortRoleName` resolves role names from the source model; numeric `appId` sorted numerically.
- `sortColumn` matters for role-based sorting; set both.
- Preserve the selected row across a sort by looking up a stable ID (App ID) in the proxy, then
  re-selecting with `table.selectionModel.setCurrentIndex(...)`.
- When the real backend (`QAbstractTableModel`) lands, keep this proxy and point `sourceModel` at
  it; do not reintroduce a JavaScript sort.

### Reading model data from QML

`QAbstractItemModel::data()`, `index()`, `rowCount()`, `columnCount()` are `Q_INVOKABLE` in Qt 6,
so proxy data can be read directly from bindings:

```qml
readonly property int selectedRow: table.selectionModel && table.selectionModel.currentIndex.valid
    ? table.selectionModel.currentIndex.row : -1

readonly property string selectedAppId: {
    if (selectedRow < 0)
        return "";
    const value = gamesProxy.data(gamesProxy.index(selectedRow, 1), Qt.DisplayRole);
    return value !== undefined && value !== null ? String(value) : "";
}
```

**Trap:** `gamesProxy.rowCount` is a method on proxy models (`rowCount()`), unlike
`TableModel.rowCount` which is a property. `KSortFilterProxyModel` also exposes the `count`
property — use that. Concatenating the method object yields
`function() { [native code] } games loaded`.

### Selection, hover, and click handling in a table

- View-level tap handlers and `TableView.pointerNavigationEnabled` interfere with custom
  selection. The reliable pattern is a per-cell `MouseArea` that handles hover, left-click
  selection, and right-click menu in one place:

```qml
// on the TableView:
pointerNavigationEnabled: false
selectionBehavior: TableView.SelectRows
selectionMode: TableView.SingleSelection
selectionModel: ItemSelectionModel { model: gamesProxy }

delegate: Rectangle {
    required property int row
    required property int column
    required property bool selected
    required property var model

    MouseArea {
        id: cellMouse
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        hoverEnabled: true
        onClicked: function(mouse) {
            table.selectionModel.setCurrentIndex(
                table.index(tableCell.row, 0),
                ItemSelectionModel.ClearAndSelect | ItemSelectionModel.Rows);
            if (mouse.button === Qt.RightButton) {
                contextMenu.targetRow = tableCell.row;
                contextMenu.popup(table, tableCell.mapToItem(table, Qt.point(mouse.x, mouse.y)));
            }
        }
    }
}
```

- `ItemSelectionModel.ClearAndSelect | ItemSelectionModel.Rows` is what makes a whole row
  selected (needed for `selected` in the delegate and for row actions).
- Right-click must select the row **before** showing the menu, mirroring
  `customContextMenuRequested` in the QtWidgets code.

### Log/viewer state: binding vs. assignment

`root.logText` is a plain property; `TextEdit.text` binds to it. Clearing is a safe assignment
(`root.logText = ""`) that does not break the binding. Avoid assigning to a property that is
itself bound (that breaks the binding).

### C++ bridge notes (cxx-qt)

- `QQuickStyle` and `QString` are available from `cxx-qt-lib` with the `qt_full` feature.
- `QGuiApplication::setApplicationName` / `setApplicationDisplayName` wrappers exist;
  `setDesktopFileName` is not wrapped (needs a small C++ bridge or is skipped).
- `QGuiApplication::font()` and `Kirigami.Theme.defaultFont` are the same font; never hardcode a
  point/pixel size in QML.

---

## 4. Layouts, Sizing, and Responsive UI

### QLayout metrics to Kirigami units

The reference used literal pixels; the QML port maps them to theme units:

| QtWidgets | Kirigami |
|---|---|
| `contentsMargins(6,6,6,6)` | `Kirigami.Units.smallSpacing` / layout margins |
| `layout->setSpacing(4)` | `spacing: Kirigami.Units.smallSpacing` |
| `addSpacing(5)` gap | `Layout.bottomMargin: Kirigami.Units.smallSpacing` on the last row |
| fixed window size 760x520 | `width/height` + `minimumWidth/minimumHeight` from `Kirigami.Units.gridUnit` |
| row height 28 | delegate `implicitHeight: 28` |

Observed values on a standard setup: `gridUnit = 18`, `smallSpacing = 4`,
`cornerRadius = 5`.

### QSplitter to SplitView

```qml
Controls.SplitView {
    orientation: Qt.Vertical

    ColumnLayout {                      // top pane
        Controls.SplitView.fillHeight: true       // stretch factor (1, 0)
        Controls.SplitView.preferredHeight: 420
    }
    Rectangle {                         // bottom pane
        Controls.SplitView.preferredHeight: 140
        Controls.SplitView.minimumHeight: Kirigami.Units.gridUnit * 3
    }
}
```

- Remember the alias-qualified attached properties (`Controls.SplitView.…`).

### SplitView handle: the style draws a line

`org.kde.desktop` implements the handle as `StyleItem { elementType: "splitter" }`, which draws a
Breeze splitter line across the whole splitter width. When an adjacent pane is a rounded
`Rectangle` (`radius: Kirigami.Units.cornerRadius`), the line's ends stay visible where the frame's
corners curve away — it reads as a stray horizontal line below the toolbar that "sticks out" of the
log/table frame corners. The handle's drawing is not controllable through properties, so replace
the handle with a non-drawing item and keep the drag hit area via `containmentMask`:

```qml
Controls.SplitView {
    orientation: Qt.Vertical

    handle: Item {
        id: splitHandle
        implicitWidth: 1
        implicitHeight: 1
        containmentMask: Item {
            x: 0
            y: -Kirigami.Units.smallSpacing
            width: splitHandle.width
            height: splitHandle.height + Kirigami.Units.smallSpacing * 2
        }
    }
}
```

- The handle's implicit size sets the splitter thickness; the visible gap comes from the pane's own
  layout margins (e.g. `Layout.bottomMargin: Kirigami.Units.smallSpacing`). Replacing the style
  handle here also brought the gap to the QtWidgets reference's 6 px.
- A 1 px handle plus a ±`smallSpacing` mask keeps dragging usable (the style default used a ~12 px
  mask).
- Style-drawn lines can be invisible in offscreen renders. To locate the handle band, temporarily
  replace it with a visible probe (`Rectangle { implicitWidth: 1; implicitHeight: 3; color: "magenta" }`)
  and pixel-scan the gap.
- Verify the fix by scanning *every* row of the gap across the full width: all rows should be one
  uniform background color, with only the rounded frame border below.

### Status bar height

A bare `ToolBar` footer rendered ~46 px, while `QStatusBar` in the reference is ~30 px. Pin it:

```qml
footer: Controls.ToolBar {
    implicitHeight: Math.round(Kirigami.Units.gridUnit * 1.7)   // ~30 px
    ...
}
```

### Frames, rounded corners, and "the border only appears under the rows"

`QTableView` draws a Breeze frame with rounded corners and a reserved scrollbar lane. In QML:

- A `Rectangle` with `radius: Kirigami.Units.cornerRadius` and a 1 px border is the frame.
- **Inset the children by the radius** (`anchors.margins: Kirigami.Units.cornerRadius`), otherwise
  cell delegates and the header **overpaint the border**. Symptom: the frame line is visible only
  where there are no cells (below the last row), so it looks like a stray border under the rows
  while the side borders next to data rows are missing.

```qml
Rectangle {                       // tableFrame
    color: Kirigami.Theme.backgroundColor
    radius: Kirigami.Units.cornerRadius
    border.width: 1
    border.color: page.frameColor

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.cornerRadius   // keep delegates off the border
        ...
    }
}
```

### Column widths, resizing, and header alignment

Share one width function between the custom header and the table:

```qml
function columnWidth(column) {
    if (column === 0) return columnWidths[0];          // e.g. 260
    if (column === 1) return columnWidths[1];          // e.g. 110
    return Math.max(80, table.width - columnWidths[0] - columnWidths[1]);   // stretch last
}

TableView {
    columnWidthProvider: function(column) { return page.columnWidth(column); }
    resizableColumns: false                            // we implement handles ourselves
}

function setColumnWidth(column, width) {
    const widths = columnWidths.slice();
    widths[column] = Math.max(40, Math.round(width));
    columnWidths = widths;
    table.forceLayout();                               // provider is not a binding
}
```

Drag handles live in the header cells (`MouseArea`, `cursorShape: Qt.SplitHCursor`,
`mapToItem(tableHeader, …)` for stable deltas). Header cells position via
`page.columnOffset(index)` built from the same widths, so header and body stay aligned.

### Grid lines

`QTableView` draws a 1 px grid; QML `TableView` does not. Add it per cell with a theme-derived
color (`#1c1e21` matched the reference `#1c1f21`):

```qml
readonly property color gridColor: Kirigami.ColorUtils.linearInterpolation(
    Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.04)

Rectangle {  // right grid line
    anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
    width: tableCell.column < 2 ? 1 : 0
    color: tableCell.gridColor
}
Rectangle {  // bottom grid line
    anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
    height: 1
    color: tableCell.gridColor
}
```

### Scrollbars: overlay vs. reserved lane

QQC2 scrollbars are **overlays**, and the Breeze handle is translucent by design
(`kf6-qqc2-desktop-style/ScrollBar.qml`: the desktop handle is drawn by the style's
`background` `StyleItem`; the `contentItem` handle exists only for touch). Over a table the
translucent handle visually blends with the rows behind it, unlike `QTableView`, where the
viewport is narrowed and the bar has its own lane.

The documented fix is to re-parent the attached scrollbar outside the Flickable and reserve space:

```qml
RowLayout {
    spacing: 0
    ColumnLayout {                 // header + table
        Layout.fillWidth: true
        Layout.fillHeight: true
        spacing: 0
        Item { id: tableHeader; ... }
        TableView {
            id: table
            Layout.fillWidth: true
            Layout.fillHeight: true
            Controls.ScrollBar.vertical: Controls.ScrollBar {
                id: tableVerticalScrollBar
                parent: scrollBarLane          // outside the flickable
                anchors.fill: parent
            }
            Controls.ScrollBar.horizontal: Controls.ScrollBar {}
        }
    }
    Item {
        id: scrollBarLane
        Layout.preferredWidth: table.contentHeight > table.height
            ? tableVerticalScrollBar.implicitWidth : 0
        Layout.fillHeight: true
    }
}
```

- Reservation is driven by `contentHeight > height` (stable; no layout feedback loop).
- The header lives inside the same inner `ColumnLayout`, so it narrows with the table and stays
  aligned when the bar appears.

### Log viewer: viewport-filling editor in a Flickable

Problems with `ScrollView { TextEdit {} }`:

- The editor is content-sized, so right-click only works exactly on the text.
- Clearing the log leaves zero hit area — the context menu becomes unreachable.

The canonical Qt `TextEdit` pattern fixes both: the editor fills the viewport, paints its full
content beyond its bounds, and the Flickable clips/pans.

```qml
Flickable {
    id: logFlickable
    property bool followOutput: true

    anchors.fill: parent
    anchors.margins: Kirigami.Units.cornerRadius
    clip: true
    contentWidth: logArea.paintedWidth
    contentHeight: logArea.paintedHeight

    onContentYChanged: followOutput = contentY >= contentHeight - height - 4

    Controls.ScrollBar.vertical: Controls.ScrollBar {}

    TextEdit {
        id: logArea
        width: logFlickable.width
        height: logFlickable.height          // viewport-sized; text paints beyond
        readOnly: true
        wrapMode: TextEdit.NoWrap
        selectByMouse: true
        persistentSelection: true            // selection survives while the menu is open
        font: Kirigami.Theme.fixedWidthFont
        color: Kirigami.Theme.textColor
        selectionColor: Kirigami.Theme.highlightColor
        selectedTextColor: Kirigami.Theme.highlightedTextColor
        text: root.logText

        onTextChanged: {
            cursorPosition = length;
            Qt.callLater(function() {        // wait for paintedHeight/contentHeight to update
                if (logFlickable.followOutput) {
                    logFlickable.contentY = Math.max(0, logFlickable.contentHeight - logFlickable.height);
                }
            });
        }
    }

    TapHandler {                             // right-click anywhere in the viewport
        acceptedButtons: Qt.RightButton
        onTapped: function(event) { page.openLogContextMenu(logArea, event.position); }
    }
}

TapHandler {                                 // fallback: 1px border / corner margin band
    acceptedButtons: Qt.RightButton
    onTapped: function(event) { page.openLogContextMenu(parent, event.position); }
}

function openLogContextMenu(item, position) {
    if (logContextMenu.visible) return;      // both handlers may fire; guard double-popup
    logContextMenu.popup(item, position);
}
```

- `followOutput` is only recomputed when `contentY` changes, so content growth does not clear it;
  this reproduces `QPlainTextEdit::appendPlainText()`'s "stick to bottom unless the user scrolled
  up" behavior.
- `Qt.callLater` is required because `paintedHeight` (and thus `contentHeight`) is not guaranteed
  to be updated when `textChanged` fires.

### Scrollable pages (Kirigami docs warning)

The Kirigami docs warn: **do not put a `ScrollView` inside a `Kirigami.ScrollablePage`** — its
children are already inside a `ScrollView`. For a fixed multi-pane layout (table + log with a
splitter), use a plain `Kirigami.Page`; each pane owns its scrolling. `Kirigami.ScrollablePage`
is for content that should scroll as one (ListView content, etc.).

---

## 5. Styling, Theming, and Platform Integration

### Application initialization (`main.rs`)

```rust
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};

fn main() {
    // Prefer the KDE QQC2 style unless the user/environment overrides it.
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        QQuickStyle::set_style(&QString::from("org.kde.desktop"));
    }

    let mut app = QGuiApplication::new();
    if let Some(mut app) = app.as_mut() {          // Pin reborrow: methods consume the Pin
        app.as_mut().set_application_name(&QString::from("protonctx"));
        app.as_mut().set_application_display_name(&QString::from("protonctx"));
    }

    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/com/example/cxxqt_minimal/src/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
```

- Set the style **before** the QML engine loads.
- Safe fallback: if `org.kde.desktop` is missing, the app degrades to Basic with a warning.
- Runtime environment:
  - Plasma session (`XDG_CURRENT_DESKTOP=KDE`): the KDE platform theme
    (`KDEPlasmaPlatformTheme6`) loads automatically; nothing else needed.
  - Other sessions: install `plasma-integration` (platform theme) and
    `kf6-qqc2-desktop-style` (the `org.kde.desktop` style), then
    `export QT_QPA_PLATFORMTHEME=kde` and `export QT_QUICK_CONTROLS_STYLE=org.kde.desktop`.
  - `QT_QPA_PLATFORMTHEME` must be set before `QGuiApplication`; it is intentionally not forced
    in code so users on other desktops are not overridden.

### QStyle/Palette vs. Kirigami.Theme

- Do not use `palette.*` for themed UI. Under non-Plasma/offscreen platforms it resolves to a light
  Basic palette (`palette.mid == #b8b8b8`), which looks broken on a dark theme.
- `Kirigami.Theme` is driven by KColorScheme and is the correct source of truth.
- `Kirigami.Theme` (PlatformTheme) does **not** expose view/header colors; those exist on the
  internal `BasicThemeDefinition`. Use **color sets** instead.

### Color sets require `inherit: false`

Setting `Kirigami.Theme.colorSet` alone is not enough; Kirigami's own QML always pairs it with
`inherit: false` (22 occurrences in the shipped QML):

```qml
Rectangle {                                   // table/log viewport
    Kirigami.Theme.inherit: false
    Kirigami.Theme.colorSet: Kirigami.Theme.View
    color: Kirigami.Theme.backgroundColor       // #141618 (QPalette::Base)
}

Item {                                        // table header
    Kirigami.Theme.inherit: false
    Kirigami.Theme.colorSet: Kirigami.Theme.Header
    // Kirigami.Theme.backgroundColor -> #292c30
}
```

Verified values on a Breeze Dark scheme:

| colorSet | backgroundColor | alternateBackgroundColor | textColor |
|---|---|---|---|
| `Window` | `#202326` | `#292c30` | `#fcfcfc` |
| `View` | `#141618` | `#1d1f22` | `#fcfcfc` |
| `Header` | `#292c30` | — | `#fcfcfc` |

`View` matches `QPalette::Base`/`AlternateBase` exactly, which is why a QtWidgets table and a QML
table render identical row colors once the color set is applied.

### Frame/separator color: use sRGB, not `linearInterpolation`

KDE's own frame color can be derived from the theme, no hardcoded hex:

```qml
readonly property color frameColor: blendColor(
    Kirigami.Theme.textColor, Kirigami.Theme.backgroundColor, Kirigami.Theme.frameContrast)

function blendColor(foreground, background, alpha) {
    return Qt.rgba(foreground.r * alpha + background.r * (1 - alpha),
                   foreground.g * alpha + background.g * (1 - alpha),
                   foreground.b * alpha + background.b * (1 - alpha),
                   1);
}
```

- `blendColor(text, bg, frameContrast)` produced `#4c4e51`, exactly the QtWidgets frame color
  (`QPalette::Mid`); Kirigami's `linearInterpolation` gave `#474c51` because it interpolates in
  **HSV**.
- `Kirigami.ColorUtils.alphaBlend()` is broken for fractional alpha in this Kirigami version: its
  C++ multiplies 0-255 alpha by 0-255 channels without dividing by 255, `QColor::fromRgb` rejects
  the out-of-range result, and the invalid color renders black. Use `blendColor` instead.

### Item-view hover: Breeze's exact formula

From `breezestyle.cpp` (`drawPanelItemViewItemPrimitive`):

```cpp
color = palette.color(colorGroup, QPalette::Highlight);
if (mouseOver && !hasCustomBackground) {
    if (!selected) color.setAlphaF(Metrics::Blend_Value);      // 0.3
    else           color = color.lighter(Metrics::Focus_LightenColorValue);  // 110
}
```

QML equivalent (per-row, because the blend is over the row's base/alternate color):

```qml
readonly property color rowColor: row % 2 === 1
    ? Kirigami.Theme.alternateBackgroundColor : Kirigami.Theme.backgroundColor
readonly property color hoverColor: page.blendColor(Kirigami.Theme.highlightColor, rowColor, 0.3)

color: selected
    ? (cellMouse.containsMouse ? Qt.lighter(Kirigami.Theme.highlightColor, 1.1)
                               : Kirigami.Theme.highlightColor)
    : (cellMouse.containsMouse ? hoverColor : rowColor)
```

Measured parity: hover on an alternate row `#1e3f21` vs reference `#1e3f22`; hover on a base row
`#17391a`. Do not use `Kirigami.Theme.hoverColor` here — it is the accent `#3daee9`, not the
translucent selection tint.

### Fonts

- Use theme fonts only. The prototype hardcodes **zero** point/pixel sizes; the one pixel-size use
  (sort glyph) was replaced with `font: Kirigami.Theme.smallFont`.
- `Qt.application.font` and `Kirigami.Theme.defaultFont` are the same font in a Plasma session.
- **QtWidgets bold and QML bold are not the same weight in practice.** The QtWidgets header
  renders with `QFont::Bold` through the platform/style font stack; in QML `font.bold: true` maps
  to `font.weight: Font.Bold` (700), which rendered noticeably heavier than the QtWidgets header at
  the same family and size. If bold is too much and normal is not enough, use an intermediate
  weight (`font.weight`):

  | Constant | Value |
  |---|---|
  | `Font.Thin` | 100 |
  | `Font.ExtraLight` | 200 |
  | `Font.Light` | 300 |
  | `Font.Normal` | 400 |
  | `Font.Medium` | 500 |
  | `Font.DemiBold` | 600 |
  | `Font.Bold` | 700 |
  | `Font.ExtraBold` | 800 |
  | `Font.Black` | 900 |

  The port settled on `font.weight: Font.Medium` (500) for the table header; `Font.DemiBold` (600)
  is the next step if that is still too light.
- Intermediate weights must exist in the installed family (Noto Sans ships Regular / Medium /
  SemiBold / Bold), otherwise Qt picks the nearest available weight.
- Do not set `font.bold` and `font.weight` together: they are linked (`bold: true` forces weight
  700, and any weight below 700 sets `bold: false`). Set `font.weight` alone.
- Weight is a style choice, not a size hardcode: family and size still come from the theme.

### Icons

- Use themed symbolic names with `icon.name` (`view-refresh`, `configure`, `application-exit`,
  `edit-copy-symbolic`, `edit-select-all-symbolic`, `help-about`, `application-menu`).
  Resolution follows the Breeze icon theme set by the platform theme.

### Text context menus and private APIs

- The `org.kde.desktop` style provides a **singleton** `TextFieldContextMenu` for
  `TextField`/`TextArea` with Copy/Select All (and editing items), triggered by an internal
  `TapHandler`. It has no extension hook.
- Appending items at runtime through the private singleton's `addItem()`/`insertItem()` API
  **segfaults** when the menu is shown (tested with and without a separator). Do not use it.
- `import org.kde.desktop.private` also turns the app into a hard dependency on qqc2-desktop-style
  (load-time failure instead of a graceful Basic fallback).
- Practical solution for a read-only log: use plain `TextEdit` and provide the menu explicitly
  (`Copy` with `enabled: logArea.selectedText.length > 0`, separator, `Select All`, separator,
  `Clear logs`), opened by a right-button handler.

### Menu bars vs. hamburger (KDE HIG)

- The KDE HIG ("Layout and navigation → Menus") says: desktop/widescreen → **menu bar above the
  toolbar**, mobile/narrow → hamburger. A hamburger is acceptable for roughly ≤15 actions;
  larger apps should use a real menu bar. Do not put window-management actions such as "Quit" in a
  hamburger.
- `Kirigami.ApplicationWindow` inherits `QQC2.ApplicationWindow`, so it has the `menuBar`
  property, and `org.kde.desktop` styles `MenuBar`/`Menu`/`MenuItem` with the Header color set.
- There is no QML `KHamburgerMenu` equivalent (QtWidgets-only), and no Plasma global-menu
  (appmenu) integration for `QQuickMenuBar` — only QtWidgets exports to the global menu.
- A practical hybrid for a desktop prototype: hide the Kirigami global toolbar
  (`page.globalToolBarStyle: Kirigami.ApplicationHeaderStyle.None`), supply a custom
  `page.header: Controls.ToolBar` containing a burger `ToolButton` (opening a flat
  `Controls.Menu`) plus contextual buttons.

---

## 6. Common Pitfalls, Anti-Patterns, and Debugging Tips

### Pitfalls encountered (symptom → cause → fix)

1. **"Table header looks light/wrong"** — `TableViewDelegate`/`HorizontalHeaderView` under Basic
   draw with `palette.light`, independent of the Kirigami theme. Fix: custom header delegate using
   `Kirigami.Theme` colors with the `Header` color set.
2. **`TableViewColumn` unknown** — it does not exist in Qt 6 (Qt5 Controls 1.x legacy). Use
   `Qt.labs.qmlmodels.TableModel` columns or a C++ model.
3. **`HorizontalHeaderView` delegate gets `column == 0` for every cell** when its model is a
   separate `ListModel`. Use a custom header (`Item` + `Repeater`) for full control.
4. **`Non-existent attached object` at load** — attached properties must be alias-qualified:
   `Controls.SplitView.fillHeight`, not `SplitView.fillHeight`.
5. **`Controls.MouseArea is not a type`** — `MouseArea`, `TapHandler`, `HoverHandler`,
   `Shortcut`, `TextEdit`, `Flickable`, `TableView` are QtQuick types; drop the prefix.
6. **Left-click selects nothing while right-click works** — `pointerNavigationEnabled` handles
   left clicks and only moves the current index. Set it `false` and select explicitly in the
   delegate (`ClearAndSelect | Rows`).
7. **`colorSet` seems ignored** — always add `Kirigami.Theme.inherit: false` next to it.
8. **Ugly light-gray outlines around frames** — `Kirigami.Theme.disabledTextColor` is a text color
   (`#a1a9b1`), far too bright for frames. Use the `frameContrast` sRGB blend (`#4c4e51`).
9. **Hover turns teal/gray instead of the accent tint** — `linearInterpolation` is HSV; blend in
   sRGB with `blendColor`. Also avoid `alphaBlend` (broken for fractional alpha → black).
10. **`font.bold: true` looks heavier in QML than QtWidgets bold** — bold is not a portable
    weight: QML maps it to `Font.Bold` (700), which overshot the QtWidgets header weight at the same
    family/size. Use an intermediate `font.weight` (`Font.Medium` 500 / `Font.DemiBold` 600), and
    never set `bold` and `weight` together. See §5.
11. **Scrollbar blends with content** — QQC2/Breeze scrollbars are translucent overlays. Reserve a
    lane and re-parent the attached `ScrollBar` outside the Flickable (Qt docs pattern).
12. **Frame border appears only below the rows** — delegates/header overpaint the 1 px border.
    Inset the frame's children by `radius` (or the border width).
13. **A stray horizontal line below the toolbar that pokes out of rounded frame corners** — the
    `org.kde.desktop` SplitView handle is a `StyleItem { elementType: "splitter" }` drawing a
    full-width Breeze line; it shows where the adjacent rounded `Rectangle` curves away. Override
    `handle:` with a non-drawing `Item` (keep the grab area with `containmentMask`). See §4.
14. **Right-click only works on the text / not at all after clearing** — a content-sized `TextEdit`
    inside `ScrollView`. Use `Flickable` + viewport-sized `TextEdit`
    (`width/height: flickable.width/height`, `contentWidth/Height: paintedWidth/Height`).
15. **Log does not auto-scroll when lines are appended** — a Flickable does not follow content
    growth. Track `followOutput` on `contentY` and scroll in `Qt.callLater` after `textChanged`.
16. **`function() { [native code] } games loaded`** — `rowCount` is a method on proxy models; use
    the `count` property (or call `rowCount()`).
17. **QML errors are invisible during development** — `console.log` output was suppressed in the
    cxx-qt/offscreen setup. Use `QT_FORCE_STDERR_LOGGING=1` (plus
    `QT_LOGGING_RULES="*=true"` for import/plugin detail), or render diagnostics into visible
    `Label`s and screenshot them.
18. **A temporary backup/restore silently reverted real fixes** — after restoring a backup during
    experiments, re-grep for the changes before finishing. This recurred repeatedly, including once
    when the backup had been taken *before* the fix, so the restore undid it silently.
19. **Private API shortcuts bite** — extending the style's singleton text context menu crashed;
    private imports also break graceful style fallback.
20. **Qt.callLater for post-layout work** — `paintedHeight`, `contentHeight`, and layout geometry
    can lag one event-loop turn after a property change.
21. **`QML Page: Created graphical object was not placed in the graphics scene`** — benign; it
    also appears with a minimal `Kirigami.ApplicationWindow` + `Kirigami.Page` in this environment.
22. **`QQuickStyle` fallback** — without setting the style, the app uses Basic/Fusion; verify with
    `QT_LOGGING_RULES="*=true"` and look for `style "org.kde.desktop" set on QQuickStyleSpec`.
23. **Model resets and selection** — re-sorting a model resets rows; re-select using a stable ID
    rather than a row index.
24. **Layout feedback loops** — deriving a reserved width from `visibleArea` can oscillate; derive
    it from invariant facts (`contentHeight > height`, `implicitWidth`).

### Debugging toolkit used throughout

- **Build-time QML validation:** `cargo build` runs `qmlcachegen`; syntax errors surface as
  `qmlcachegen failed for src/Main.qml: ... error: Expected token '}'`.
- **Runtime QML errors:** `QT_FORCE_STDERR_LOGGING=1 QT_LOGGING_RULES="*=true" ./target/debug/...`
  and grep for `Main.qml:<line>`.
- **Headless visual verification:** run with `QT_QPA_PLATFORM=offscreen` and use a temporary
  `Timer` + `page.grabToImage(cb, Qt.size(w, h))` + `result.saveToFile(...)`, then quit.
  Grab a specific `Item` (e.g., the page or a menu's `contentItem`) — grabbing the window
  `contentItem` did not produce a file with the offscreen platform.
- **Probing style-drawn geometry:** replace a style-provided element (SplitView handle, header
  delegate, etc.) with a bright test item of a known size and pixel-scan to find its exact position
  and thickness before restyling it.
- **Pixel-level comparison:** ImageMagick color-run extraction to compare against QtWidgets
  screenshots, e.g. crop a 1 px column and print contiguous color runs, then diff against the
  reference:
  ```bash
  convert shot.png -crop 1x300+400+110 +repage -depth 8 txt:- | \
    awk 'NR>1{split($1,a,",");y=a[2];sub(":","",y);c=$3; if(c!=p){if(NR>2)print s"-"l" "p;s=y;p=c}l=y} END{print s"-"l" "p}'
  ```
- **Compare fonts/text:** threshold a crop to isolate glyphs and compare bounding boxes; compare
  `Kirigami.Theme.defaultFont` against `Qt.application.font` in an in-app label.
- **Read the upstream source:** Breeze (`kstyle/breezestyle.cpp`), Kirigami
  (`src/platform/colorutils.cpp`, `controls/*.qml`), Qt (`qheaderview.cpp`,
  `qquickscrollview.cpp`, `qquicktextedit.cpp`). The exact constants (`Blend_Value = 0.3`,
  `Focus_LightenColorValue = 110`, HSV interpolation) are not in the docs.
- **Check installed QML modules/types:** the `*.qmltypes` files and `qmldir`s under
  `<qt>/qml/` answer "does this property/type exist in this version?" definitively.

---

## 7. Actionable Migration Checklist

### Environment and build

- [ ] Decide the target style: `QQuickStyle::set_style("org.kde.desktop")` before engine load
      (unless `QT_QUICK_CONTROLS_STYLE` is set by the user).
- [ ] Set `applicationName`/`applicationDisplayName`; plan a `.desktop` file for the taskbar icon
      (`setDesktopFileName` is not wrapped by cxx-qt-lib).
- [ ] Document runtime deps: `plasma-integration` + `kf6-qqc2-desktop-style`;
      `QT_QPA_PLATFORMTHEME=kde` + `QT_QUICK_CONTROLS_STYLE=org.kde.desktop` outside Plasma.
- [ ] Confirm the effective style in logs; never rely on the implicit fallback.

### Application shell

- [ ] `Kirigami.ApplicationWindow` + `pageStack.initialPage: Component { Kirigami.Page { ... } }`.
- [ ] Choose the menu UI against the KDE HIG (menu bar for desktop, hamburger for ≤~15 actions);
      know that QML has no KHamburgerMenu and no global-menu export.
- [ ] Recreate menus/actions with `Controls.Menu`/`MenuItem` (or a custom `page.header` toolbar);
      carry shortcuts over (`Shortcut`, `StandardKey.*`).
- [ ] Reproduce dialogs with `Kirigami.Dialog`/`PromptDialog`; keep button/type enum values.

### Layout

- [ ] Translate margins/spacing to `Kirigami.Units` (`smallSpacing`, `largeSpacing`,
      `cornerRadius`, `gridUnit`).
- [ ] `QSplitter` → `Controls.SplitView` with alias-qualified attached properties; encode stretch
      factors via `fillHeight` and initial sizes via `preferredHeight`.
- [ ] `QStatusBar` → footer `ToolBar` with an explicit `implicitHeight` matching the reference.
- [ ] Preserve "extra spacing" items with layout margins (`Layout.bottomMargin`) rather than
      spacer hacks.
- [ ] For framed views, inset content by the corner radius so children do not cover the border.
- [ ] Style or replace the `SplitView` handle if the style draws a splitter line that can show
      through rounded frame corners; keep the `containmentMask` so dragging stays usable.

### Tables and lists

- [ ] Multi-column model: `TableModel` (prototype) or the existing C++ model via cxx-qt.
- [ ] Add `KSortFilterProxyModel` between model and view; wire header clicks to
      `sortColumn`/`sortAscending`; map columns to `sortRoleName`s.
- [ ] Custom header (sort indicator, dividers, hover, drag-resize) if you need the QHeaderView
      feature set; share `columnWidth()` with the table.
- [ ] Grid lines, alternating rows, hover, selection, and right-click handled in the delegate with
      a single `MouseArea` (`hoverEnabled`, left/right buttons).
- [ ] `pointerNavigationEnabled: false`; select with `ClearAndSelect | Rows`; re-select by stable
      ID after sorting.
- [ ] Reserve a scrollbar lane when content overflows; re-parent the attached `ScrollBar` outside
      the Flickable.

### Text and logging

- [ ] Read-only log: `Flickable` + viewport-filling `TextEdit`
      (`contentWidth/Height: paintedWidth/Height`, `clip: true`).
- [ ] Auto-follow: `followOutput` tracked on `contentY`; scroll in `Qt.callLater` from
      `onTextChanged`.
- [ ] Context menu on the whole log area: right-button handlers on the editor and the frame,
      guarded against double-popup; provide Copy/Select All explicitly (the style's menu singleton
      cannot be extended).
- [ ] Set `persistentSelection: true`; theme selection colors from `Kirigami.Theme`.

### Theming

- [ ] Replace `palette.*` usage with `Kirigami.Theme.*`; use color sets (`View`, `Header`,
      `Window`) with `inherit: false`.
- [ ] Frames/separators from `frameContrast` via an sRGB `blendColor`; hover from
      `highlightColor` at 0.3 over the row base; selected+hover `Qt.lighter(highlightColor, 1.1)`.
- [ ] No hardcoded font sizes; choose header emphasis with `font.weight` (`Font.Medium` /
      `Font.DemiBold` rather than `font.bold: true`); monospace via `Kirigami.Theme.fixedWidthFont`.
- [ ] Icons via themed `icon.name` values.
- [ ] Never import `org.kde.desktop.private`; avoid singleton mutation.

### Verification

- [ ] `cargo build` (catches QML syntax via qmlcachegen) and an offscreen run with forced logging
      after every change.
- [ ] Screenshot and pixel-compare against the QtWidgets reference for: frame color/radius, row
      colors, header color, grid color, hover/selection, status bar height, scrollbar placement.
- [ ] Exercise edge cases: empty log, long log (scroll + follow), many rows (scrollbar lane),
      column resize, sort both directions with a selected row, right-click on empty space.
- [ ] After any backup/restore during experiments, re-grep the final file for the intended
      changes.

### Cleanup

- [ ] Remove temporary timers/harnesses/diagnostic labels; verify `grep` finds no leftovers.
- [ ] Remove test rows from the model.
- [ ] Keep the prototype's deviations documented (e.g., hamburger instead of menu bar, triangle
      sort glyph instead of the Breeze chevron).

---

## 8. Reference Sources and Links

- Qt Quick `TableView` (lives in `QtQuick`; no sorting, `pointerNavigationEnabled`):
  https://doc.qt.io/qt-6/qml-qtquick-tableview.html
- Qt Quick Controls `ScrollBar` (attached re-parenting pattern, non-attached usage):
  https://doc.qt.io/qt-6/qml-qtquick-controls-scrollbar.html
- Qt Quick `TextEdit` (Flickable content sizing, `paintedWidth`/`paintedHeight`):
  https://doc.qt.io/qt-6/qml-qtquick-textedit.html
- KDE Human Interface Guidelines (layout/navigation, menus, spacing units):
  https://develop.kde.org/hig/layout_and_nav/
- Kirigami: scrollable pages and list views (ScrollablePage warning, KSortFilterProxyModel hint):
  https://develop.kde.org/docs/getting-started/kirigami/components-scrollablepages_listviews/
- Kirigami API: `Kirigami.Theme`, `Kirigami.Units`, `Kirigami.ColorUtils`, `Kirigami.Page`,
  `Kirigami.Dialog`, `Kirigami.PromptDialog`, `Kirigami.GlobalDrawer`:
  https://api.kde.org/kirigami-index.html
- KDE KItemModels QML (`KSortFilterProxyModel`):
  https://api.kde.org/kitemmodels-index.html
- Breeze style source (item-view hover, header sections, frame metrics):
  https://invent.kde.org/plasma/breeze/-/blob/master/kstyle/breezestyle.cpp
- Kirigami color utilities (HSV `linearInterpolation`, `alphaBlend`):
  https://invent.kde.org/frameworks/kirigami/-/blob/master/src/platform/colorutils.cpp
- qqc2-desktop-style (Breeze QQC2 controls, text context menu singleton):
  https://invent.kde.org/frameworks/qqc2-desktop-style
- plasma-integration (platform theme: fonts, palette, icons, global menu):
  https://invent.kde.org/plasma/plasma-integration

### Case-study files

- QtWidgets reference: `./protonctx/src/qt_main_ui.cpp`,
  `qt_logs_ui.cpp`, `qt_settings_ui.cpp`, `qt_about_ui.cpp`, `app_backend.rs`
- QML port: `./ktest/src/Main.qml`, `main.rs`, `backend.rs`, `Cargo.toml`,
  `build.rs`
