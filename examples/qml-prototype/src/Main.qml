import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as Controls
import Qt.labs.qmlmodels
import QtQml.Models
import org.kde.kirigami as Kirigami
import org.kde.kitemmodels as KItemModels

Kirigami.ApplicationWindow {
    id: root

    title: "protonctx (Kirigami prototype)"
    width: 760
    height: 520
    minimumWidth: Kirigami.Units.gridUnit * 26
    minimumHeight: Kirigami.Units.gridUnit * 16

    property bool loading: false
    property int sortColumn: 0
    property bool sortAscending: true
    property var sortRoleNames: ["name", "appId", "compatTool"]

    property string logText: "[09:41:03.120] protonctx starting\n[09:41:03.145] Scanning Steam libraries...\n[09:41:03.402] Found 3 games\n[09:41:03.403] Ready"

    function appendLog(message) {
        const now = new Date();
        const pad = (value, length) => ("000" + value).slice(-length);
        const stamp = pad(now.getHours(), 2) + ":" + pad(now.getMinutes(), 2)
            + ":" + pad(now.getSeconds(), 2) + "." + pad(now.getMilliseconds(), 3);
        logText += "\n[" + stamp + "] " + message;
    }

    function refreshGames() {
        loading = true;
        appendLog("Refreshing game list...");
        refreshTimer.restart();
    }

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

    KItemModels.KSortFilterProxyModel {
        id: gamesProxy

        sourceModel: gamesModel
        sortRoleName: root.sortRoleNames[root.sortColumn]
        sortOrder: root.sortAscending ? Qt.AscendingOrder : Qt.DescendingOrder
        sortColumn: root.sortColumn
    }

    ListModel {
        id: headerModel

        ListElement { display: "Game" }
        ListElement { display: "App ID" }
        ListElement { display: "Compatibility Tool" }
    }

    Timer {
        id: refreshTimer
        interval: 700
        repeat: false
        onTriggered: {
            root.loading = false;
            root.appendLog("Scan complete: " + gamesProxy.count + " games found.");
        }
    }

    Controls.Menu {
        id: contextMenu

        property int targetRow: -1

        Controls.MenuItem {
            text: "Browse for executable..."
            onTriggered: root.appendLog("Browse for executable... (row " + contextMenu.targetRow + ")")
        }
        Controls.MenuSeparator {}
        Controls.MenuItem {
            text: "Explorer"
            onTriggered: root.appendLog("Launching tool: Explorer (row " + contextMenu.targetRow + ")")
        }
        Controls.MenuItem {
            text: "Registry Editor"
            onTriggered: root.appendLog("Launching tool: Registry Editor (row " + contextMenu.targetRow + ")")
        }
        Controls.MenuItem {
            text: "Task Manager"
            onTriggered: root.appendLog("Launching tool: Task Manager (row " + contextMenu.targetRow + ")")
        }
        Controls.MenuItem {
            text: "Wine Configuration"
            onTriggered: root.appendLog("Launching tool: Wine Configuration (row " + contextMenu.targetRow + ")")
        }
        Controls.MenuSeparator {}
        Controls.MenuItem {
            text: "Copy compatdata path"
            onTriggered: root.appendLog("Copy compatdata path (row " + contextMenu.targetRow + ")")
        }
        Controls.MenuItem {
            text: "Copy compatibility tool path"
            onTriggered: root.appendLog("Copy compatibility tool path (row " + contextMenu.targetRow + ")")
        }
        Controls.MenuSeparator {}
        Controls.MenuItem {
            text: "Delete Shader Cache"
            enabled: false
        }
    }

    Kirigami.Dialog {
        id: settingsDialog

        title: "Settings"
        padding: Kirigami.Units.largeSpacing
        preferredWidth: Kirigami.Units.gridUnit * 20
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel

        ColumnLayout {
            spacing: Kirigami.Units.smallSpacing

            Controls.CheckBox {
                text: "Remember last used directory"
                checked: true
                Layout.fillWidth: true
                onToggled: root.appendLog("Settings: remember last directory = " + checked)
            }
        }
    }

    Kirigami.Dialog {
        id: aboutDialog

        title: "About protonctx"
        padding: Kirigami.Units.largeSpacing
        preferredWidth: Kirigami.Units.gridUnit * 20
        standardButtons: Kirigami.Dialog.Close

        Item {
            implicitWidth: Kirigami.Units.gridUnit * 20
            implicitHeight: aboutColumn.implicitHeight

            ColumnLayout {
                id: aboutColumn

                anchors.fill: parent
                spacing: Kirigami.Units.smallSpacing

                Kirigami.Icon {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: Kirigami.Units.iconSizes.huge
                    Layout.preferredHeight: Kirigami.Units.iconSizes.huge
                    source: "applications-games"
                }

                Kirigami.Heading {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    level: 2
                    text: "protonctx v0.1.0"
                }

                Kirigami.Heading {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    level: 3
                    type: Kirigami.Heading.Type.Secondary
                    wrapMode: Text.WordWrap
                    text: "Launch executables inside a Steam game's Proton context."
                }

                Controls.Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    color: Kirigami.Theme.disabledTextColor
                    text: "Licensed under the GNU GPL v3."
                }
            }
        }
    }

    Kirigami.PromptDialog {
        id: launchErrorDialog

        title: "Launch Error"
        dialogType: Kirigami.PromptDialog.Error
        subtitle: "The selected executable could not be launched."
        standardButtons: Kirigami.Dialog.Close
    }

    Controls.Menu {
        id: mainMenu

        Controls.MenuItem {
            text: "Settings"
            icon.name: "configure"
            onTriggered: settingsDialog.open()
        }
        Controls.MenuItem {
            text: "About protonctx"
            icon.name: "help-about"
            onTriggered: aboutDialog.open()
        }
        Controls.MenuSeparator {}
        Controls.MenuItem {
            text: "Exit"
            icon.name: "application-exit"
            onTriggered: root.close()
        }
    }

    Shortcut {
        sequence: "F5"
        enabled: !root.loading
        onActivated: root.refreshGames()
    }

    pageStack.initialPage: mainPage

    Component {
        id: mainPage

        Kirigami.Page {
            id: page

            title: ""
            padding: 0
            globalToolBarStyle: Kirigami.ApplicationHeaderStyle.None

            header: Controls.ToolBar {
                RowLayout {
                    anchors.fill: parent
                    spacing: Kirigami.Units.smallSpacing

                    Controls.ToolButton {
                        id: menuButton

                        icon.name: "application-menu"
                        onClicked: mainMenu.popup(menuButton, 0, menuButton.height)
                    }
                    Controls.ToolButton {
                        icon.name: "view-refresh"
                        text: "Refresh games"
                        display: Controls.AbstractButton.TextBesideIcon
                        enabled: !root.loading
                        onClicked: root.refreshGames()
                    }

                    Item { Layout.fillWidth: true }
                }
            }

            readonly property int selectedRow: table.selectionModel && table.selectionModel.currentIndex.valid
                ? table.selectionModel.currentIndex.row : -1

            readonly property string selectedAppId: {
                if (selectedRow < 0) {
                    return "";
                }
                const value = gamesProxy.data(gamesProxy.index(selectedRow, 1), Qt.DisplayRole);
                return value !== undefined && value !== null ? String(value) : "";
            }

            readonly property color frameColor: blendColor(Kirigami.Theme.textColor, Kirigami.Theme.backgroundColor, Kirigami.Theme.frameContrast)

            function blendColor(foreground, background, alpha) {
                return Qt.rgba(foreground.r * alpha + background.r * (1 - alpha),
                               foreground.g * alpha + background.g * (1 - alpha),
                               foreground.b * alpha + background.b * (1 - alpha),
                               1);
            }

            property var columnWidths: [260, 110]

            function toggleSort(column) {
                const keep = selectedAppId;
                if (root.sortColumn === column) {
                    root.sortAscending = !root.sortAscending;
                } else {
                    root.sortColumn = column;
                    root.sortAscending = true;
                }
                if (keep === "") {
                    return;
                }
                for (let i = 0; i < gamesProxy.count; ++i) {
                    const value = gamesProxy.data(gamesProxy.index(i, 1), Qt.DisplayRole);
                    if (value !== undefined && value !== null && String(value) === keep) {
                        table.selectionModel.setCurrentIndex(gamesProxy.index(i, 0), ItemSelectionModel.ClearAndSelect | ItemSelectionModel.Rows);
                        break;
                    }
                }
            }

            function columnOffset(column) {
                if (column === 0) {
                    return 0;
                }
                if (column === 1) {
                    return columnWidth(0);
                }
                return columnWidth(0) + columnWidth(1);
            }

            function columnWidth(column) {
                if (column === 0) {
                    return columnWidths[0];
                }
                if (column === 1) {
                    return columnWidths[1];
                }
                return Math.max(80, table.width - columnWidths[0] - columnWidths[1]);
            }

            function setColumnWidth(column, width) {
                const widths = columnWidths.slice();
                widths[column] = Math.max(40, Math.round(width));
                columnWidths = widths;
                table.forceLayout();
            }

            function openLogContextMenu(item, position) {
                if (logContextMenu.visible) {
                    return;
                }
                logContextMenu.popup(item, position);
            }

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                spacing: 0

                Controls.SplitView {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
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

                    ColumnLayout {
                        spacing: Kirigami.Units.smallSpacing
                        Controls.SplitView.fillHeight: true
                        Controls.SplitView.preferredHeight: 420

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            Kirigami.Theme.inherit: false
                            Kirigami.Theme.colorSet: Kirigami.Theme.View
                            color: Kirigami.Theme.backgroundColor
                            radius: Kirigami.Units.cornerRadius
                            border.width: 1
                            border.color: page.frameColor

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: Kirigami.Units.cornerRadius
                                spacing: 0

                                RowLayout {
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    spacing: 0

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        Layout.fillHeight: true
                                        spacing: 0


                                        Item {
                                            id: tableHeader

                                            Layout.fillWidth: true
                                            Layout.preferredHeight: 28
                                            Kirigami.Theme.inherit: false
                                            Kirigami.Theme.colorSet: Kirigami.Theme.Header

                                            Repeater {
                                                model: headerModel

                                                delegate: Rectangle {
                                                    id: headerCell

                                                    required property int index
                                                    required property string display

                                                    Kirigami.Theme.inherit: false
                                                    Kirigami.Theme.colorSet: Kirigami.Theme.Header

                                                    x: page.columnOffset(index)
                                                    width: page.columnWidth(index)
                                                    height: tableHeader.height
                                                    color: hoverHandler.hovered
                                                        ? Kirigami.ColorUtils.linearInterpolation(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.1)
                                                        : Kirigami.Theme.backgroundColor

                                                    Rectangle {
                                                        anchors.top: parent.top
                                                        anchors.bottom: parent.bottom
                                                        anchors.right: parent.right
                                                        width: headerCell.index < 2 ? 1 : 0
                                                        color: page.frameColor
                                                    }
                                                    Rectangle {
                                                        anchors.left: parent.left
                                                        anchors.right: parent.right
                                                        anchors.bottom: parent.bottom
                                                        height: 1
                                                        color: page.frameColor
                                                    }
                                                    RowLayout {
                                                        anchors.centerIn: parent
                                                        spacing: Kirigami.Units.smallSpacing / 2

                                                        Controls.Label {
                                                            text: headerCell.display
                                                            color: Kirigami.Theme.textColor
                                                            font.weight: Font.Medium
                                                        }
                                                        Controls.Label {
                                                            text: root.sortAscending ? "▲" : "▼"
                                                            visible: root.sortColumn === headerCell.index
                                                            color: Kirigami.Theme.textColor
                                                            font: Kirigami.Theme.smallFont
                                                        }
                                                    }

                                                    TapHandler {
                                                        onTapped: page.toggleSort(headerCell.index)
                                                    }

                                                    HoverHandler {
                                                        id: hoverHandler
                                                    }

                                                    MouseArea {
                                                        id: resizeHandle

                                                        anchors.top: parent.top
                                                        anchors.bottom: parent.bottom
                                                        anchors.right: parent.right
                                                        width: headerCell.index < 2 ? 6 : 0
                                                        cursorShape: Qt.SplitHCursor
                                                        preventStealing: true

                                                        property real pressPosition
                                                        property real pressWidth

                                                        onPressed: function(mouse) {
                                                            pressPosition = mapToItem(tableHeader, mouse.x, 0).x;
                                                            pressWidth = page.columnWidths[headerCell.index];
                                                        }
                                                        onPositionChanged: function(mouse) {
                                                            if (!pressed) {
                                                                return;
                                                            }
                                                            const position = mapToItem(tableHeader, mouse.x, 0).x;
                                                            page.setColumnWidth(headerCell.index, pressWidth + position - pressPosition);
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        TableView {
                                            id: table

                                            Layout.fillWidth: true
                                            Layout.fillHeight: true
                                            clip: true
                                            model: gamesProxy
                                            alternatingRows: true
                                            pointerNavigationEnabled: false
                                            resizableColumns: false
                                            selectionBehavior: TableView.SelectRows
                                            selectionMode: TableView.SingleSelection

                                            columnWidthProvider: function(column) {
                                                return page.columnWidth(column);
                                            }

                                            Controls.ScrollBar.vertical: Controls.ScrollBar {
                                                id: tableVerticalScrollBar
                                                parent: scrollBarLane
                                                anchors.fill: parent
                                            }
                                            Controls.ScrollBar.horizontal: Controls.ScrollBar {}

                                            selectionModel: ItemSelectionModel {
                                                model: gamesProxy
                                            }

                                            delegate: Rectangle {
                                                id: tableCell

                                                required property int row
                                                required property int column
                                                required property bool selected
                                                required property var model

                                                Kirigami.Theme.inherit: false
                                                Kirigami.Theme.colorSet: Kirigami.Theme.View

                                                readonly property color gridColor: Kirigami.ColorUtils.linearInterpolation(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.04)
                                                readonly property color rowColor: row % 2 === 1 ? Kirigami.Theme.alternateBackgroundColor : Kirigami.Theme.backgroundColor
                                                readonly property color hoverColor: page.blendColor(Kirigami.Theme.highlightColor, rowColor, 0.3)

                                                implicitHeight: 28
                                                color: selected
                                                    ? (cellMouse.containsMouse ? Qt.lighter(Kirigami.Theme.highlightColor, 1.1) : Kirigami.Theme.highlightColor)
                                                    : (cellMouse.containsMouse ? hoverColor : rowColor)

                                                Rectangle {
                                                    anchors.top: parent.top
                                                    anchors.bottom: parent.bottom
                                                    anchors.right: parent.right
                                                    width: tableCell.column < 2 ? 1 : 0
                                                    color: tableCell.gridColor
                                                }
                                                Rectangle {
                                                    anchors.left: parent.left
                                                    anchors.right: parent.right
                                                    anchors.bottom: parent.bottom
                                                    height: 1
                                                    color: tableCell.gridColor
                                                }

                                                Controls.Label {
                                                    anchors.fill: parent
                                                    anchors.leftMargin: Kirigami.Units.smallSpacing
                                                    anchors.rightMargin: Kirigami.Units.smallSpacing
                                                    verticalAlignment: Text.AlignVCenter
                                                    horizontalAlignment: Text.AlignLeft
                                                    elide: Text.ElideRight
                                                    text: tableCell.model.display !== undefined ? tableCell.model.display : ""
                                                    color: tableCell.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                                                }

                                                MouseArea {
                                                    id: cellMouse

                                                    anchors.fill: parent
                                                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                                                    hoverEnabled: true

                                                    onClicked: function(mouse) {
                                                        table.selectionModel.setCurrentIndex(table.index(tableCell.row, 0), ItemSelectionModel.ClearAndSelect | ItemSelectionModel.Rows);
                                                        if (mouse.button === Qt.RightButton) {
                                                            contextMenu.targetRow = tableCell.row;
                                                            contextMenu.popup(table, tableCell.mapToItem(table, Qt.point(mouse.x, mouse.y)));
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                    }

                                    Item {
                                        id: scrollBarLane

                                        Layout.preferredWidth: table.contentHeight > table.height ? tableVerticalScrollBar.implicitWidth : 0
                                        Layout.fillHeight: true
                                    }
                                }
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            Layout.bottomMargin: Kirigami.Units.smallSpacing
                            spacing: Kirigami.Units.smallSpacing

                            Controls.Button {
                                text: "Browse..."
                                enabled: page.selectedRow >= 0
                                onClicked: root.appendLog("Browse... (row " + page.selectedRow + ")")
                            }
                            Controls.Button {
                                text: "Explorer"
                                enabled: page.selectedRow >= 0
                                onClicked: root.appendLog("Launching tool: Explorer (row " + page.selectedRow + ")")
                            }
                            Controls.Button {
                                text: "Registry Editor"
                                enabled: page.selectedRow >= 0
                                onClicked: root.appendLog("Launching tool: Registry Editor (row " + page.selectedRow + ")")
                            }
                            Controls.Button {
                                text: "Task Manager"
                                enabled: page.selectedRow >= 0
                                onClicked: root.appendLog("Launching tool: Task Manager (row " + page.selectedRow + ")")
                            }
                            Controls.Button {
                                text: "Wine Configuration"
                                enabled: page.selectedRow >= 0
                                onClicked: root.appendLog("Launching tool: Wine Configuration (row " + page.selectedRow + ")")
                            }

                            Item { Layout.fillWidth: true }
                        }
                    }

                    Rectangle {
                        Kirigami.Theme.inherit: false
                        Kirigami.Theme.colorSet: Kirigami.Theme.View
                        color: Kirigami.Theme.backgroundColor
                        radius: Kirigami.Units.cornerRadius
                        border.width: 1
                        border.color: page.frameColor
                        Controls.SplitView.preferredHeight: 140
                        Controls.SplitView.minimumHeight: Kirigami.Units.gridUnit * 3

                        Controls.Menu {
                            id: logContextMenu

                            Controls.MenuItem {
                                text: "Copy"
                                icon.name: "edit-copy-symbolic"
                                enabled: logArea.selectedText.length > 0
                                onTriggered: logArea.copy()
                            }
                            Controls.MenuSeparator {}
                            Controls.MenuItem {
                                text: "Select All"
                                icon.name: "edit-select-all-symbolic"
                                onTriggered: logArea.selectAll()
                            }
                            Controls.MenuSeparator {}
                            Controls.MenuItem {
                                text: "Clear logs"
                                onTriggered: root.logText = ""
                            }
                        }

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
                                height: logFlickable.height
                                readOnly: true
                                wrapMode: TextEdit.NoWrap
                                selectByMouse: true
                                persistentSelection: true
                                font: Kirigami.Theme.fixedWidthFont
                                color: Kirigami.Theme.textColor
                                selectionColor: Kirigami.Theme.highlightColor
                                selectedTextColor: Kirigami.Theme.highlightedTextColor
                                text: root.logText

                                onTextChanged: {
                                    cursorPosition = length;
                                    Qt.callLater(function() {
                                        if (logFlickable.followOutput) {
                                            logFlickable.contentY = Math.max(0, logFlickable.contentHeight - logFlickable.height);
                                        }
                                    });
                                }

                                TapHandler {
                                    acceptedButtons: Qt.RightButton

                                    onTapped: function(event) {
                                        page.openLogContextMenu(logArea, event.position);
                                    }
                                }
                            }
                        }

                        TapHandler {
                            acceptedButtons: Qt.RightButton

                            onTapped: function(event) {
                                page.openLogContextMenu(parent, event.position);
                            }
                        }
                    }
                }
            }

            footer: Controls.ToolBar {
                implicitHeight: Math.round(Kirigami.Units.gridUnit * 1.7)

                RowLayout {
                    anchors.fill: parent
                    spacing: Kirigami.Units.smallSpacing

                    Controls.Label {
                        text: root.loading ? "Loading games..." : gamesProxy.count + " games loaded"
                    }
                    Controls.BusyIndicator {
                        visible: false
                        running: visible
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 2
                    }
                    Item { Layout.fillWidth: true }
                    Controls.Label {
                        text: page.selectedRow >= 0 ? "Selected: " + page.selectedAppId : ""
                    }
                }
            }
        }
    }
}
