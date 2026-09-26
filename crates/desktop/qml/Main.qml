import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Window
import com.synveil.desktop 1.0

pragma ComponentBehavior: Bound

ApplicationWindow {
    id: root
    objectName: "synveilMainWindow"
    width: 1080
    height: 720
    minimumWidth: 860
    minimumHeight: 560
    visible: true
    title: qsTr("Synveil")
    color: palette.window

    DesktopUiBridge {
        id: bridge
    }

    FolderDialog {
        id: libraryFolderDialog
        title: qsTr("Choose a local library folder")
        onAccepted: bridge.setLibraryFolder(selectedFolder.toString())
    }

    property var ui_bridge: bridge
    property int liveTestPhase: bridge.live_test ? 0 : -1
    property int liveTestWaitTicks: 0
    property string liveTestSignature: ""
    property bool liveTestRecoveringObserved: false
    property int liveTestAvailableGeneration: -1
    property bool editingConnection: !bridge.profile_configured

    Component.onCompleted: {
        bridge.installTray()
        bridge.startController()
        if (bridge.smoke_test) {
            smokeExitTimer.start()
        }
        console.log("SYNVEIL-QML-LIVE START live_test=" + bridge.live_test)
        if (bridge.live_test) {
            liveTestTimer.start()
        }
    }

    Timer {
        id: smokeExitTimer
        interval: 120
        repeat: false
        onTriggered: bridge.requestQuit()
    }

    Timer {
        id: liveTestTimer
        interval: 20
        repeat: true
        onTriggered: root.runLiveTestStep()
    }

    function observeLiveTestState() {
        var signature = [
            "connection=" + bridge.connection_label,
            "freshness=" + bridge.freshness_label,
            "process=" + bridge.process_label,
            "launch=" + bridge.launch_label,
            "generation=" + bridge.connection_generation,
            "libraries=" + bridge.library_count,
            "root=" + bridge.selected_root_label,
            "auth=" + bridge.selected_auth_label,
            "error=" + bridge.last_error_label,
            "can_sync=" + bridge.selected_can_sync,
            "auth_required=" + bridge.auth_required,
            "auth_in_flight=" + bridge.auth_in_flight,
            "can_sign_out=" + bridge.selected_can_sign_out,
            "profile_configured=" + bridge.profile_configured,
            "profile_authenticated=" + bridge.profile_authenticated,
            "tray=" + bridge.tray_available,
            "library_list_count=" + libraryList.count
        ].join(" ")
        root.observeLiveTestRootLabel(signature)
        if (signature === root.liveTestSignature) {
            return
        }
        root.liveTestSignature = signature
        console.log("SYNVEIL-QML-LIVE STATE " + signature)
    }

    function observeLiveTestRootLabel(observedSignature) {
        if (!bridge.live_test) {
            return
        }
        if (observedSignature.indexOf("root=Folder available") !== -1) {
            root.liveTestAvailableGeneration = bridge.connection_generation
        }
        if (!root.liveTestRecoveringObserved
                && observedSignature.indexOf("root=Checking folder changes") !== -1) {
            root.liveTestRecoveringObserved = true
            console.log("SYNVEIL-QML-LIVE RECOVERING"
                        + " root_label=Checking folder changes"
                        + " freshness=" + bridge.freshness_label
                        + " generation=" + bridge.connection_generation
                        + " prior_available_generation="
                        + root.liveTestAvailableGeneration
                        + " same_controller_generation="
                        + (root.liveTestAvailableGeneration === bridge.connection_generation))
        }
    }

    function runLiveTestStep() {
        root.observeLiveTestState()
        if (root.liveTestPhase < 0) {
            return
        }

        if (root.liveTestPhase === 0) {
            if (bridge.live_test_exit_on_terminal
                    && (bridge.connection_label === qsTr("Incompatible background service")
                        || (bridge.connection_label === qsTr("Connection unavailable")
                            && bridge.freshness_label === qsTr("Status unavailable"))
                        || (bridge.connection_label === qsTr("Background service unavailable")
                            && bridge.freshness_label === qsTr("Status unavailable")))
                    && bridge.launch_label !== qsTr("Background launch is not requested.")) {
                console.log("SYNVEIL-QML-LIVE ACTION quit_terminal"
                            + " connection=" + bridge.connection_label
                            + " error=" + bridge.last_error_label)
                root.liveTestPhase = -1
                liveTestTimer.stop()
                bridge.trayQuit()
                return
            }

            if (bridge.live_test_exit_after_ready
                    && bridge.connection_label === qsTr("Connected")
                    && bridge.freshness_label === qsTr("Current")
                    && bridge.library_count > 0) {
                console.log("SYNVEIL-QML-LIVE ACTION quit_after_ready"
                            + " generation=" + bridge.connection_generation
                            + " tray=" + bridge.tray_available)
                root.liveTestPhase = -1
                liveTestTimer.stop()
                // Route through the native tray-facing signal so this live
                // check exercises the same controller-only quit path that a
                // user can invoke from the tray menu.
                bridge.trayQuit()
                return
            }

            if (bridge.connection_label === qsTr("Connected")
                    && bridge.freshness_label === qsTr("Current")
                    && bridge.library_count > 0
                    && bridge.selected_can_sync) {
                if (bridge.live_test_rapid_clicks) {
                    for (var click = 0; click < 1000; ++click) {
                        bridge.syncNow()
                    }
                    console.log("SYNVEIL-QML-LIVE ACTION sync_now_clicks=1000"
                                + " in_flight=" + bridge.sync_in_flight)
                } else {
                    bridge.syncNow()
                    console.log("SYNVEIL-QML-LIVE ACTION sync_now_clicks=1"
                                + " in_flight=" + bridge.sync_in_flight
                                + " feedback=" + bridge.sync_feedback)
                }
                root.liveTestPhase = 1
                return
            }

            root.liveTestWaitTicks += 1
            if (root.liveTestWaitTicks > 1500) {
                console.log("SYNVEIL-QML-LIVE FAIL ready_timeout")
                root.liveTestPhase = -1
                liveTestTimer.stop()
                bridge.requestQuit()
            }
            return
        }

        if (root.liveTestPhase === 1
                && !bridge.sync_in_flight
                && bridge.sync_feedback.length > 0) {
            console.log("SYNVEIL-QML-LIVE RESULT feedback=" + bridge.sync_feedback
                        + " in_flight=" + bridge.sync_in_flight)
            root.liveTestPhase = 2
            if (bridge.live_test_exit_after_sync) {
                liveTestTimer.stop()
                bridge.requestQuit()
            }
        }
    }

    function submitProfileConfiguration() {
        if (bridge.configuration_busy || serverUrlField.text.trim().length === 0) {
            return false
        }
        bridge.configureProfile(serverUrlField.text, profileLabelField.text)
        root.editingConnection = false
        return true
    }

    function submitLibrarySetup() {
        if (bridge.library_setup_busy
                || libraryNameField.text.trim().length === 0
                || bridge.library_setup_folder.length === 0) {
            return false
        }
        bridge.setupLibrary(libraryNameField.text)
        return true
    }

    Connections {
        target: bridge

        function onOpenRequested() {
            root.show()
            root.raise()
            root.requestActivate()
        }

        function onQuitRequested() {
            bridge.requestQuit()
        }

        function onQuitFinished() {
            Qt.quit()
        }
    }

    onClosing: function(close) {
        if (bridge.close_to_tray && bridge.tray_available) {
            close.accepted = false
            root.hide()
        } else {
            close.accepted = false
            bridge.requestQuit()
        }
    }

    header: ToolBar {
        contentHeight: 84
        ColumnLayout {
            anchors.fill: parent
            anchors.leftMargin: 24
            anchors.rightMargin: 24
            anchors.topMargin: 6
            anchors.bottomMargin: 6
            spacing: 5

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                Label {
                    text: qsTr("SYNVEIL")
                    font.pixelSize: 20
                    font.weight: Font.DemiBold
                    color: palette.text
                }

                Label {
                    text: qsTr("Desktop")
                    font.pixelSize: 15
                    color: palette.text
                }

                Item { Layout.fillWidth: true }

                Button {
                    text: qsTr("Settings")
                    onClicked: settingsDrawer.open()
                    Accessible.name: qsTr("Open desktop settings")
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Label {
                    text: bridge.connection_label
                    font.pixelSize: 12
                    color: palette.text
                    elide: Text.ElideRight
                    Layout.maximumWidth: 140
                    Accessible.name: qsTr("Connection: %1").arg(bridge.connection_label)
                }

                Label {
                    text: bridge.process_label
                    font.pixelSize: 12
                    color: palette.text
                    elide: Text.ElideRight
                    Layout.maximumWidth: 145
                    Accessible.name: qsTr("Background service: %1").arg(bridge.process_label)
                }

                Label {
                    text: bridge.launch_label
                    font.pixelSize: 11
                    color: palette.text
                    elide: Text.ElideRight
                    Layout.maximumWidth: 165
                    Accessible.name: qsTr("Background launch: %1").arg(bridge.launch_label)
                }

                Label {
                    text: bridge.freshness_label
                    font.pixelSize: 12
                    color: palette.text
                    elide: Text.ElideRight
                    Layout.maximumWidth: 110
                    Accessible.name: qsTr("Status freshness: %1").arg(bridge.freshness_label)
                }

                Label {
                    text: qsTr("Attention: %1").arg(bridge.attention_count)
                    visible: bridge.attention_count > 0
                    color: palette.text
                    font.pixelSize: 12
                    elide: Text.ElideRight
                    Layout.maximumWidth: 110
                    Accessible.name: qsTr("Needs attention: %1").arg(bridge.attention_count)
                }

                Label {
                    text: qsTr("Recovery: %1").arg(bridge.recovery_action_required_count)
                    visible: bridge.recovery_action_required_count > 0
                    color: palette.text
                    font.pixelSize: 12
                    elide: Text.ElideRight
                    Layout.maximumWidth: 105
                    Accessible.name: qsTr("Needs recovery: %1")
                                      .arg(bridge.recovery_action_required_count)
                }
            }
        }
    }

    Drawer {
        id: settingsDrawer
        edge: Qt.RightEdge
        width: Math.min(420, root.width * 0.82)
        height: root.height
        modal: true
        interactive: true

        ScrollView {
            id: settingsScroll
            anchors.fill: parent
            padding: 24
            clip: true

            ColumnLayout {
                width: settingsScroll.availableWidth
                spacing: 18

                RowLayout {
                    Layout.fillWidth: true

                    Label {
                        text: qsTr("Settings")
                        font.pixelSize: 22
                        font.weight: Font.DemiBold
                        Layout.fillWidth: true
                    }

                    Button {
                        text: qsTr("Close")
                        onClicked: settingsDrawer.close()
                        Accessible.name: qsTr("Close desktop settings")
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: syncSettingsColumn.implicitHeight + 28
                    radius: 10
                    color: palette.base
                    border.color: palette.midlight

                    ColumnLayout {
                        id: syncSettingsColumn
                        anchors.fill: parent
                        anchors.margins: 14
                        spacing: 9

                        Label {
                            text: qsTr("Synchronization")
                            font.pixelSize: 15
                            font.weight: Font.DemiBold
                        }

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Global sync: %1").arg(bridge.sync_control_label)
                            color: palette.text
                        }

                        RowLayout {
                            Layout.fillWidth: true

                            Button {
                            text: bridge.sync_paused ? qsTr("Resume sync") : qsTr("Pause sync")
                                enabled: !bridge.sync_control_busy
                                         && bridge.connection_label === qsTr("Connected")
                                         && bridge.sync_control_label !== qsTr("Sync status unavailable")
                                onClicked: {
                                    if (bridge.sync_paused) {
                                        bridge.resumeSync()
                                    } else {
                                        bridge.pauseSync()
                                    }
                                }
                                Accessible.name: bridge.sync_paused
                                                 ? qsTr("Resume synchronization")
                                                 : qsTr("Pause synchronization")
                            }

                            BusyIndicator {
                                running: bridge.sync_control_busy
                                visible: running
                                Layout.preferredWidth: 24
                                Layout.preferredHeight: 24
                            }
                        }

                        Label {
                            Layout.fillWidth: true
                            visible: bridge.sync_control_feedback.length > 0
                            text: bridge.sync_control_feedback
                            color: palette.text
                            wrapMode: Text.WordWrap
                            font.pixelSize: 12
                        }
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: startupSettingsColumn.implicitHeight + 28
                    radius: 10
                    color: palette.base
                    border.color: palette.midlight

                    ColumnLayout {
                        id: startupSettingsColumn
                        anchors.fill: parent
                        anchors.margins: 14
                        spacing: 9

                        Label {
                            text: qsTr("Background sync at login")
                            font.pixelSize: 15
                            font.weight: Font.DemiBold
                        }

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Current state: %1").arg(bridge.background_startup_state)
                            color: palette.text
                            wrapMode: Text.WordWrap
                        }

                        CheckBox {
                            id: startupCheckBox
                            text: qsTr("Start the background client when I sign in")
                            checked: bridge.background_startup_state === qsTr("Enabled")
                            enabled: !bridge.background_startup_busy
                            onClicked: bridge.setBackgroundStartup(checked)
                            Accessible.name: qsTr("Start the background client at login")
                        }

                        BusyIndicator {
                            running: bridge.background_startup_busy
                            visible: running
                            Layout.preferredWidth: 24
                            Layout.preferredHeight: 24
                        }

                        Label {
                            Layout.fillWidth: true
                            visible: bridge.background_startup_feedback.length > 0
                            text: bridge.background_startup_feedback
                            color: palette.text
                            wrapMode: Text.WordWrap
                            font.pixelSize: 12
                        }
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: traySettingsColumn.implicitHeight + 28
                    radius: 10
                    color: palette.base
                    border.color: palette.midlight

                    ColumnLayout {
                        id: traySettingsColumn
                        anchors.fill: parent
                        anchors.margins: 14
                        spacing: 9

                        Label {
                            text: qsTr("Window behavior")
                            font.pixelSize: 15
                            font.weight: Font.DemiBold
                        }

                        CheckBox {
                            text: qsTr("Close the window to the system tray")
                            checked: bridge.close_to_tray && bridge.tray_available
                            enabled: bridge.tray_available
                            onClicked: bridge.setCloseToTray(checked)
                            Accessible.name: qsTr("Close the desktop window to the system tray")
                        }

                        Label {
                            Layout.fillWidth: true
                            text: bridge.tray_available
                                  ? qsTr("The background client keeps running when the window is hidden.")
                                  : qsTr("A system tray is unavailable; closing will exit the desktop shell.")
                            color: palette.text
                            wrapMode: Text.WordWrap
                            font.pixelSize: 12
                        }

                        Label {
                            Layout.fillWidth: true
                            visible: bridge.close_to_tray_feedback.length > 0
                            text: bridge.close_to_tray_feedback
                            color: palette.text
                            wrapMode: Text.WordWrap
                            font.pixelSize: 12
                        }
                    }
                }

                Label {
                    Layout.fillWidth: true
                    text: qsTr("Connection: %1 · %2").arg(bridge.connection_label).arg(bridge.freshness_label)
                    color: palette.text
                    wrapMode: Text.WordWrap
                    font.pixelSize: 12
                }

            }
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.margins: 18
        anchors.topMargin: 18
        spacing: 18

        Pane {
            Layout.fillHeight: true
            Layout.preferredWidth: 320
            Layout.minimumWidth: 260
            padding: 0

            ColumnLayout {
                anchors.fill: parent
                spacing: 12

                RowLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: 14
                    Layout.rightMargin: 14

                    Label {
                        text: qsTr("Libraries")
                        font.pixelSize: 17
                        font.weight: Font.DemiBold
                    }

                    Label {
                        text: qsTr("%1 total").arg(bridge.library_count)
                        color: palette.text
                        font.pixelSize: 12
                        Layout.alignment: Qt.AlignRight
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: 14
                    Layout.rightMargin: 14
                    spacing: 10

                    Label {
                        text: qsTr("Needs attention: %1").arg(bridge.attention_count)
                        color: palette.text
                        font.pixelSize: 12
                        Layout.fillWidth: true
                    }

                    Label {
                        text: qsTr("Folders unavailable: %1").arg(bridge.root_unavailable_count)
                        color: palette.text
                        font.pixelSize: 12
                        horizontalAlignment: Text.AlignRight
                        Layout.fillWidth: true
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 1
                    color: palette.midlight
                    opacity: 0.6
                }

                ListView {
                    id: libraryList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    model: bridge.libraries
                    spacing: 4
                    ScrollBar.vertical: ScrollBar { }

                    delegate: ItemDelegate {
                        id: libraryDelegate
                        required property var modelData
                        width: ListView.view.width
                        highlighted: root.ui_bridge.selected_library_id === modelData.libraryId
                        Accessible.name: modelData.label
                        Accessible.description: modelData.rootLabel

                        background: Rectangle {
                            radius: 8
                            color: libraryDelegate.highlighted
                                   ? palette.highlight : "transparent"
                            opacity: libraryDelegate.highlighted ? 0.16 : 1
                        }

                        contentItem: RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 14
                            anchors.rightMargin: 8
                            spacing: 8

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 3

                                Label {
                                    text: libraryDelegate.modelData.label
                                    font.weight: Font.Medium
                                    elide: Text.ElideRight
                                    Layout.fillWidth: true
                                }

                                Label {
                                    text: libraryDelegate.modelData.conflictLabel
                                          + qsTr("  ·  ")
                                          + libraryDelegate.modelData.rootLabel
                                    color: palette.text
                                    font.pixelSize: 12
                                    elide: Text.ElideRight
                                    Layout.fillWidth: true
                                }
                            }

                            ToolButton {
                                text: root.ui_bridge.sync_paused
                                      ? qsTr("Paused")
                                      : (root.ui_bridge.sync_in_flight
                                         ? qsTr("Waiting") : qsTr("Sync"))
                                enabled: libraryDelegate.modelData.canSync
                                          && !root.ui_bridge.sync_in_flight
                                Layout.alignment: Qt.AlignVCenter
                                onClicked: {
                                    root.ui_bridge.selectLibrary(libraryDelegate.modelData.libraryId)
                                    root.ui_bridge.syncLibrary(libraryDelegate.modelData.libraryId)
                                }
                                Accessible.name: root.ui_bridge.sync_paused
                                                  ? qsTr("Sync is paused for %1")
                                                    .arg(libraryDelegate.modelData.label)
                                                  : qsTr("Request a sync for %1")
                                                    .arg(libraryDelegate.modelData.label)
                            }
                        }

                        onClicked: root.ui_bridge.selectLibrary(modelData.libraryId)
                    }

                    Label {
                        anchors.centerIn: parent
                        width: Math.max(0, parent.width - 28)
                        visible: libraryList.count === 0
                        text: bridge.empty_library_message
                        color: palette.text
                        wrapMode: Text.WordWrap
                        horizontalAlignment: Text.AlignHCenter
                        Accessible.name: bridge.empty_library_message
                    }
                }

                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 14
                    Layout.rightMargin: 14
                    Layout.bottomMargin: 10
                    visible: bridge.libraries_truncated
                    text: qsTr("Some libraries are hidden for safety.")
                    color: palette.text
                    wrapMode: Text.WordWrap
                    font.pixelSize: 12
                }
            }
        }

        Pane {
            Layout.fillWidth: true
            Layout.fillHeight: true
            padding: 0

            ScrollView {
                id: mainScroll
                anchors.fill: parent
                padding: 24
                clip: true

                ColumnLayout {
                    width: mainScroll.availableWidth
                    spacing: 18

                    RowLayout {
                        Layout.fillWidth: true

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 4

                            Label {
                                text: bridge.library_count > 0
                                      ? bridge.selected_library_label
                                      : (bridge.profile_display_name.length > 0
                                         ? bridge.profile_display_name
                                         : qsTr("Synveil desktop"))
                                font.pixelSize: 25
                                font.weight: Font.DemiBold
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }

                            Label {
                                text: bridge.profile_message
                                color: palette.text
                                font.pixelSize: 12
                                wrapMode: Text.WordWrap
                                Layout.fillWidth: true
                            }
                        }

                        Button {
                            text: bridge.sync_paused
                                  ? qsTr("Sync paused")
                                  : (bridge.sync_in_flight ? qsTr("Requesting…") : qsTr("Sync Now"))
                            enabled: bridge.selected_can_sync && !bridge.sync_in_flight
                            highlighted: true
                            onClicked: bridge.syncNow()
                            Accessible.name: bridge.sync_paused
                                              ? qsTr("Sync is paused. Resume in Settings to continue.")
                                              : qsTr("Request a sync")
                        }

                        Button {
                            text: qsTr("Edit connection")
                            visible: bridge.profile_configured
                                     && !root.editingConnection
                                     && !bridge.configuration_busy
                            onClicked: root.editingConnection = true
                            Accessible.name: qsTr("Edit server connection")
                        }
                    }

                    Label {
                        Layout.fillWidth: true
                        visible: bridge.sync_feedback.length > 0
                        text: bridge.sync_feedback
                        color: palette.text
                        wrapMode: Text.WordWrap
                    }

                    Label {
                        objectName: "syncPausedStatus"
                        Layout.fillWidth: true
                        visible: bridge.sync_paused
                        text: qsTr("Sync is paused. Resume in Settings to continue.")
                        color: palette.text
                        wrapMode: Text.WordWrap
                        Accessible.name: text
                    }

                    Rectangle {
                        id: recoveryCard
                        Layout.fillWidth: true
                        visible: bridge.recovery_action_required_count > 0
                                 || bridge.recovery_waiting_count > 0
                                 || bridge.recovery_feedback.length > 0
                                 || bridge.recovery_busy
                        implicitHeight: recoveryColumn.implicitHeight + 28
                        radius: 10
                        color: palette.base
                        border.color: palette.midlight

                        ColumnLayout {
                            id: recoveryColumn
                            anchors.fill: parent
                            anchors.margins: 14
                            spacing: 8

                            RowLayout {
                                Layout.fillWidth: true

                                Label {
                                    text: qsTr("Recovery")
                                    font.pixelSize: 15
                                    font.weight: Font.DemiBold
                                    Layout.fillWidth: true
                                }

                                Label {
                                    text: bridge.client_recovery_label
                                    color: palette.text
                                    font.pixelSize: 12
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                text: bridge.recovery_waiting_count > 0
                                      ? qsTr("Synveil is retrying some conditions automatically. Items that need your action are listed below.")
                                      : qsTr("Use only the supported action for each condition. Synveil keeps recovery state across desktop restarts.")
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }

                            ListView {
                                id: recoveryList
                                Layout.fillWidth: true
                                visible: count > 0
                                implicitHeight: Math.min(260, Math.max(56, contentHeight))
                                clip: true
                                model: bridge.recovery_items
                                spacing: 5
                                ScrollBar.vertical: ScrollBar { }

                                delegate: ItemDelegate {
                                    id: recoveryDelegate
                                    required property var modelData
                                    width: ListView.view.width
                                    Accessible.name: modelData.kindLabel
                                    Accessible.description: modelData.detail

                                    contentItem: ColumnLayout {
                                        anchors.fill: parent
                                        anchors.leftMargin: 12
                                        anchors.rightMargin: 12
                                        spacing: 3

                                        RowLayout {
                                            Layout.fillWidth: true

                                            Label {
                                                text: recoveryDelegate.modelData.kindLabel
                                                font.weight: Font.Medium
                                                Layout.fillWidth: true
                                            }

                                            Label {
                                                text: recoveryDelegate.modelData.libraryLabel
                                                color: palette.text
                                                font.pixelSize: 12
                                            }
                                        }

                                        Label {
                                            text: recoveryDelegate.modelData.detail
                                            color: palette.text
                                            wrapMode: Text.WordWrap
                                            font.pixelSize: 12
                                            Layout.fillWidth: true
                                        }

                                        RowLayout {
                                            Layout.fillWidth: true
                                            spacing: 8

                                            Label {
                                                visible: recoveryDelegate.modelData.waiting
                                                text: qsTr("Waiting")
                                                color: palette.text
                                                font.pixelSize: 12
                                            }

                                            Button {
                                                visible: recoveryDelegate.modelData.actionCode === "configure_profile"
                                                text: qsTr("Open connection")
                                                enabled: !bridge.recovery_busy
                                                onClicked: root.editingConnection = true
                                                Accessible.name: qsTr("Open server connection settings")
                                            }

                                            Button {
                                                visible: recoveryDelegate.modelData.actionCode === "authenticate"
                                                text: qsTr("Open sign-in")
                                                enabled: !bridge.recovery_busy
                                                onClicked: bridge.selectLibrary(recoveryDelegate.modelData.libraryId)
                                                Accessible.name: qsTr("Open sign-in for this library")
                                            }

                                            Button {
                                                visible: recoveryDelegate.modelData.actionCode === "start_client"
                                                text: qsTr("Start client")
                                                enabled: !bridge.recovery_busy
                                                onClicked: bridge.startBackgroundClient()
                                                Accessible.name: qsTr("Start the background client")
                                            }

                                            Button {
                                                visible: recoveryDelegate.modelData.actionCode === "check_again"
                                                text: qsTr("Check again")
                                                enabled: !bridge.recovery_busy
                                                onClicked: {
                                                    bridge.selectLibrary(recoveryDelegate.modelData.libraryId)
                                                    bridge.retrySelectedRecovery(
                                                        recoveryDelegate.modelData.libraryId,
                                                        recoveryDelegate.modelData.connectionGeneration)
                                                }
                                                Accessible.name: qsTr("Check this recovery condition again")
                                            }

                                            Button {
                                                visible: recoveryDelegate.modelData.actionCode === "resume_setup"
                                                text: qsTr("Resume setup")
                                                enabled: !bridge.recovery_busy
                                                onClicked: bridge.resumePendingSetup()
                                                Accessible.name: qsTr("Resume library setup")
                                            }
                                        }
                                    }
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                visible: bridge.recovery_items_truncated
                                text: qsTr("Only the first safe recovery conditions are shown.")
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }

                            Label {
                                Layout.fillWidth: true
                                visible: bridge.recovery_feedback.length > 0
                                text: bridge.recovery_feedback
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }

                            BusyIndicator {
                                running: bridge.recovery_busy
                                visible: running
                                Layout.preferredWidth: 24
                                Layout.preferredHeight: 24
                            }
                        }
                    }

                    Rectangle {
                        id: profileCard
                        Layout.fillWidth: true
                        visible: !bridge.profile_configured
                                 || root.editingConnection
                                 || bridge.configuration_busy
                                 || bridge.configuration_feedback.length > 0
                        implicitHeight: profileColumn.implicitHeight + 28
                        radius: 10
                        color: palette.base
                        border.color: palette.midlight

                        ColumnLayout {
                            id: profileColumn
                            anchors.fill: parent
                            anchors.margins: 14
                            spacing: 8

                            Label {
                                text: bridge.profile_configured
                                      ? qsTr("Server connection")
                                      : qsTr("Connect to a Synveil server")
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                            }

                            Label {
                                Layout.fillWidth: true
                                text: bridge.profile_configured
                                      ? qsTr("The server address and label are stored locally without credentials.")
                                      : qsTr("Enter the HTTPS address of the Synveil server. You can authenticate this device after the connection is verified.")
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }

                            TextField {
                                id: serverUrlField
                                Layout.fillWidth: true
                                visible: !bridge.profile_configured
                                         || root.editingConnection
                                         || bridge.configuration_busy
                                enabled: !bridge.configuration_busy
                                text: bridge.profile_server_url
                                placeholderText: qsTr("https://server.example")
                                maximumLength: 2048
                                inputMethodHints: Qt.ImhUrlCharactersOnly
                                Accessible.name: qsTr("Synveil server address")
                                onAccepted: root.submitProfileConfiguration()
                            }

                            TextField {
                                id: profileLabelField
                                Layout.fillWidth: true
                                visible: !bridge.profile_configured
                                         || root.editingConnection
                                         || bridge.configuration_busy
                                enabled: !bridge.configuration_busy
                                text: bridge.profile_display_name
                                placeholderText: qsTr("Server label")
                                maximumLength: 256
                                Accessible.name: qsTr("Server label")
                                onAccepted: root.submitProfileConfiguration()
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 8

                                Button {
                                    text: bridge.profile_configured
                                          ? qsTr("Save connection")
                                          : qsTr("Verify and connect")
                                    visible: !bridge.profile_configured
                                             || root.editingConnection
                                             || bridge.configuration_busy
                                    enabled: !bridge.configuration_busy
                                             && serverUrlField.text.trim().length > 0
                                    onClicked: root.submitProfileConfiguration()
                                    Accessible.name: qsTr("Verify and save server connection")
                                }

                                Button {
                                    text: qsTr("Cancel")
                                    visible: bridge.profile_configured
                                             && root.editingConnection
                                             && !bridge.configuration_busy
                                    onClicked: {
                                        serverUrlField.text = bridge.profile_server_url
                                        profileLabelField.text = bridge.profile_display_name
                                        root.editingConnection = false
                                    }
                                    Accessible.name: qsTr("Cancel server connection edit")
                                }

                                BusyIndicator {
                                    running: bridge.configuration_busy
                                    visible: running
                                    Layout.preferredWidth: 24
                                    Layout.preferredHeight: 24
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                visible: bridge.configuration_feedback.length > 0
                                text: bridge.configuration_feedback
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        visible: bridge.auth_required
                                 || bridge.auth_in_flight
                                 || bridge.selected_can_sign_out
                                 || bridge.auth_feedback.length > 0
                        implicitHeight: authColumn.implicitHeight + 28
                        radius: 10
                        color: palette.base
                        border.color: palette.midlight

                        ColumnLayout {
                            id: authColumn
                            anchors.fill: parent
                            anchors.margins: 14
                            spacing: 8

                            Label {
                                text: qsTr("Device authentication")
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                            }

                            Label {
                                Layout.fillWidth: true
                                text: bridge.credential_store_unavailable
                                      ? qsTr("Secure credential storage is temporarily unavailable. Try again later; Synveil has not removed the stored credential.")
                                      : bridge.auth_required
                                      ? qsTr("Enter the one-time enrollment token to reconnect this device.")
                                      : bridge.auth_status_unknown
                                        ? qsTr("Checking authentication status.")
                                      : qsTr("This device uses the configured secure credential.")
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 8

                                TextField {
                                    id: enrollmentToken
                                    Layout.fillWidth: true
                                    visible: bridge.auth_required || bridge.auth_in_flight
                                    enabled: !bridge.auth_in_flight
                                    placeholderText: qsTr("Enrollment token")
                                    echoMode: TextInput.Password
                                    maximumLength: 69
                                    inputMethodHints: Qt.ImhSensitiveData
                                    persistentSelection: false
                                    selectByMouse: false
                                    Accessible.name: qsTr("Enrollment token")

                                    function submitToken() {
                                        if (bridge.auth_in_flight
                                                || !bridge.auth_required
                                                || text.trim().length === 0) {
                                            return
                                        }
                                        var token = text
                                        clear()
                                        bridge.authenticate(token)
                                    }

                                    onAccepted: submitToken()
                                    onVisibleChanged: {
                                        if (!visible) {
                                            clear()
                                        }
                                    }
                                }

                                Button {
                                    text: qsTr("Sign in")
                                    visible: bridge.auth_required
                                    enabled: !bridge.auth_in_flight
                                             && enrollmentToken.text.trim().length > 0
                                    onClicked: enrollmentToken.submitToken()
                                    Accessible.name: qsTr("Sign in this device")
                                }

                                Button {
                                    text: qsTr("Sign out")
                                    visible: bridge.selected_can_sign_out
                                    enabled: !bridge.auth_in_flight
                                    onClicked: bridge.signOut()
                                    Accessible.name: qsTr("Sign out this device")
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                visible: bridge.auth_feedback.length > 0
                                text: bridge.auth_feedback
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        visible: bridge.library_setup_required
                                 || bridge.library_setup_busy
                                 || (bridge.library_setup_feedback.length > 0
                                     && bridge.library_count === 0)
                        implicitHeight: librarySetupColumn.implicitHeight + 28
                        radius: 10
                        color: palette.base
                        border.color: palette.midlight

                        ColumnLayout {
                            id: librarySetupColumn
                            anchors.fill: parent
                            anchors.margins: 14
                            spacing: 8

                            Label {
                                text: qsTr("Set up a library")
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                            }

                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Choose a writable local folder. If setup was interrupted, choose the same folder to continue safely. Synveil will not delete existing files.")
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }

                            TextField {
                                id: libraryNameField
                                Layout.fillWidth: true
                                enabled: !bridge.library_setup_busy
                                placeholderText: qsTr("Library name")
                                maximumLength: 1024
                                Accessible.name: qsTr("Library name")
                                onAccepted: root.submitLibrarySetup()
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 8

                                Button {
                                    text: qsTr("Choose folder")
                                    enabled: !bridge.library_setup_busy
                                    onClicked: libraryFolderDialog.open()
                                    Accessible.name: qsTr("Choose local library folder")
                                }

                                Label {
                                    Layout.fillWidth: true
                                    text: bridge.library_setup_folder.length > 0
                                          ? bridge.library_setup_folder
                                          : qsTr("No folder selected")
                                    color: palette.text
                                    elide: Text.ElideMiddle
                                    Accessible.name: qsTr("Selected library folder")
                                }
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 8

                                Button {
                                    id: createLibraryButton
                                    text: qsTr("Create library")
                                    enabled: !bridge.library_setup_busy
                                             && libraryNameField.text.trim().length > 0
                                             && bridge.library_setup_folder.length > 0
                                    highlighted: true
                                    onClicked: root.submitLibrarySetup()
                                    Accessible.name: qsTr("Create library")
                                }

                                BusyIndicator {
                                    running: bridge.library_setup_busy
                                    visible: running
                                    Layout.preferredWidth: 24
                                    Layout.preferredHeight: 24
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                visible: bridge.library_setup_feedback.length > 0
                                text: bridge.library_setup_feedback
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }
                        }
                    }

                    Rectangle {
                        id: attentionCard
                        Layout.fillWidth: true
                        visible: bridge.attention_count > 0
                                 || bridge.attention_feedback.length > 0
                                 || bridge.attention_resolution_busy
                        implicitHeight: attentionColumn.implicitHeight + 28
                        radius: 10
                        color: palette.base
                        border.color: palette.midlight

                        ColumnLayout {
                            id: attentionColumn
                            anchors.fill: parent
                            anchors.margins: 14
                            spacing: 8

                            RowLayout {
                                Layout.fillWidth: true

                                Label {
                                    text: qsTr("Needs attention")
                                    font.pixelSize: 15
                                    font.weight: Font.DemiBold
                                    Layout.fillWidth: true
                                }

                                Label {
                                    text: qsTr("%1 conflicts · %2 other problems")
                                          .arg(bridge.conflict_attention_count)
                                          .arg(bridge.other_attention_count)
                                    color: palette.text
                                    font.pixelSize: 12
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Review the safe details below. Decisions apply only to the selected conflict and never open local files.")
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }

                            ListView {
                                id: attentionList
                                Layout.fillWidth: true
                                visible: count > 0
                                implicitHeight: Math.min(180, Math.max(54, contentHeight))
                                clip: true
                                model: bridge.attention_items
                                spacing: 4
                                ScrollBar.vertical: ScrollBar { }

                                delegate: ItemDelegate {
                                    id: attentionDelegate
                                    required property var modelData
                                    width: ListView.view.width
                                    highlighted: bridge.selected_attention_id === modelData.attentionId
                                    Accessible.name: modelData.pathLabel
                                    Accessible.description: modelData.categoryLabel

                                    background: Rectangle {
                                        radius: 8
                                        color: attentionDelegate.highlighted
                                               ? palette.highlight : "transparent"
                                        opacity: attentionDelegate.highlighted ? 0.16 : 1
                                    }

                                    contentItem: ColumnLayout {
                                        anchors.fill: parent
                                        anchors.leftMargin: 12
                                        anchors.rightMargin: 12
                                        spacing: 2

                                        Label {
                                            text: attentionDelegate.modelData.pathLabel
                                            font.weight: Font.Medium
                                            elide: Text.ElideMiddle
                                            Layout.fillWidth: true
                                        }

                                        Label {
                                            text: attentionDelegate.modelData.categoryLabel
                                                  + qsTr("  ·  ")
                                                  + attentionDelegate.modelData.libraryLabel
                                            color: palette.text
                                            font.pixelSize: 12
                                            elide: Text.ElideRight
                                            Layout.fillWidth: true
                                        }
                                    }

                                    onClicked: bridge.selectAttention(modelData.attentionId)
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                visible: bridge.attention_count > 0 && attentionList.count === 0
                                text: qsTr("No conflict item details are available. Other library status still needs attention.")
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: bridge.selected_attention_id.length > 0
                                spacing: 5

                                Label {
                                    text: bridge.selected_attention_category_label
                                    font.weight: Font.DemiBold
                                    color: palette.text
                                }

                                Label {
                                    Layout.fillWidth: true
                                    text: bridge.selected_attention_path
                                    elide: Text.ElideMiddle
                                    Accessible.name: qsTr("Selected attention path")
                                }

                                Label {
                                    Layout.fillWidth: true
                                    text: bridge.selected_attention_library_label
                                          + qsTr("  ·  ")
                                          + bridge.selected_attention_kind_label
                                    color: palette.text
                                    font.pixelSize: 12
                                }

                                Label {
                                    Layout.fillWidth: true
                                    text: bridge.selected_attention_detail
                                    color: palette.text
                                    wrapMode: Text.WordWrap
                                    font.pixelSize: 12
                                }

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8

                                    Button {
                                        text: qsTr("Accept remote")
                                        visible: bridge.selected_attention_can_accept_remote
                                        enabled: !bridge.attention_resolution_busy
                                        onClicked: bridge.acceptSelectedConflict()
                                        Accessible.name: qsTr("Accept the remote conflict state")
                                    }

                                    Button {
                                        text: qsTr("Retry local")
                                        visible: bridge.selected_attention_can_retry_local
                                        enabled: !bridge.attention_resolution_busy
                                        onClicked: bridge.retrySelectedConflict()
                                        Accessible.name: qsTr("Retry the local change against the current remote state")
                                    }

                                    BusyIndicator {
                                        running: bridge.attention_resolution_busy
                                        visible: running
                                        Layout.preferredWidth: 24
                                        Layout.preferredHeight: 24
                                    }
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                visible: bridge.attention_feedback.length > 0
                                text: bridge.attention_feedback
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }

                            Label {
                                Layout.fillWidth: true
                                visible: bridge.attention_items_truncated
                                text: qsTr("Only the first safe attention items are shown; the counts above remain exact.")
                                color: palette.text
                                wrapMode: Text.WordWrap
                                font.pixelSize: 12
                            }
                        }
                    }

                    GridLayout {
                        Layout.fillWidth: true
                        columns: 2
                        columnSpacing: 12
                        rowSpacing: 12

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 82
                            radius: 10
                            color: palette.base
                            border.color: palette.midlight

                            Column {
                                anchors.fill: parent
                                anchors.margins: 14
                                spacing: 5
                                Label { text: qsTr("Runtime"); color: palette.text; font.pixelSize: 12 }
                                Label {
                                    text: bridge.selected_runtime_label
                                    font.pixelSize: 15
                                    font.weight: Font.Medium
                                    elide: Text.ElideRight
                                    width: parent.width
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 82
                            radius: 10
                            color: palette.base
                            border.color: palette.midlight

                            Column {
                                anchors.fill: parent
                                anchors.margins: 14
                                spacing: 5
                                Label { text: qsTr("Folder"); color: palette.text; font.pixelSize: 12 }
                                Label {
                                    text: bridge.selected_root_label
                                    color: palette.text
                                    font.pixelSize: 15
                                    font.weight: Font.Medium
                                    elide: Text.ElideRight
                                    width: parent.width
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 82
                            radius: 10
                            color: palette.base
                            border.color: palette.midlight

                            Column {
                                anchors.fill: parent
                                anchors.margins: 14
                                spacing: 5
                                Label { text: qsTr("Authentication"); color: palette.text; font.pixelSize: 12 }
                                Label {
                                    text: bridge.selected_auth_label
                                    color: palette.text
                                    font.pixelSize: 15
                                    font.weight: Font.Medium
                                    elide: Text.ElideRight
                                    width: parent.width
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 82
                            radius: 10
                            color: palette.base
                            border.color: palette.midlight

                            Column {
                                anchors.fill: parent
                                anchors.margins: 14
                                spacing: 5
                                Label { text: qsTr("Conflicts"); color: palette.text; font.pixelSize: 12 }
                                Label {
                                    text: bridge.selected_conflict_label
                                    color: palette.text
                                    font.pixelSize: 15
                                    font.weight: Font.Medium
                                    elide: Text.ElideRight
                                    width: parent.width
                                }
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 1
                        color: palette.midlight
                        opacity: 0.6
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 8

                        Label {
                            text: qsTr("Latest status")
                            font.pixelSize: 17
                            font.weight: Font.DemiBold
                        }

                        Label {
                            text: bridge.selected_outcome_label
                            color: palette.text
                            wrapMode: Text.WordWrap
                            Layout.fillWidth: true
                        }

                        Label {
                            text: bridge.last_error_label
                            visible: bridge.last_error_label !== qsTr("No connection error")
                            color: palette.text
                            wrapMode: Text.WordWrap
                            Layout.fillWidth: true
                        }
                    }

                    Label {
                        Layout.fillWidth: true
                        text: qsTr("Connection: %1  ·  %2").arg(bridge.connection_label).arg(bridge.connection_detail)
                        color: palette.text
                        wrapMode: Text.WordWrap
                        font.pixelSize: 12
                    }
                }
            }
        }
    }
}
