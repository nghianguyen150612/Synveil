import QtQuick
import QtQuick.Controls
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

    property var ui_bridge: bridge
    property int liveTestPhase: bridge.live_test ? 0 : -1
    property int liveTestWaitTicks: 0
    property string liveTestSignature: ""
    property bool liveTestRecoveringObserved: false
    property int liveTestAvailableGeneration: -1

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
        if (bridge.tray_available) {
            close.accepted = false
            root.hide()
        } else {
            close.accepted = false
            bridge.requestQuit()
        }
    }

    header: ToolBar {
        contentHeight: 64
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 24
            anchors.rightMargin: 24
            spacing: 14

            Label {
                text: qsTr("SYNVEIL")
                font.pixelSize: 20
                font.weight: Font.DemiBold
                color: palette.text
            }

            Label {
                text: qsTr("Desktop")
                font.pixelSize: 15
                color: palette.mid
            }

            Item { Layout.fillWidth: true }

            Label {
                text: bridge.connection_label
                font.pixelSize: 13
                color: bridge.connection_label === qsTr("Connected")
                       ? "#2f7d5b" : palette.mid
                Accessible.name: qsTr("Connection")
            }

            Label {
                text: bridge.process_label
                font.pixelSize: 12
                color: palette.mid
                elide: Text.ElideRight
                Layout.maximumWidth: 250
                Accessible.name: qsTr("Background service")
            }

            Label {
                text: bridge.launch_label
                font.pixelSize: 11
                color: palette.mid
                elide: Text.ElideRight
                Layout.maximumWidth: 280
                Accessible.name: qsTr("Background launch")
            }

            Label {
                text: bridge.freshness_label
                font.pixelSize: 12
                color: palette.mid
                Accessible.name: qsTr("Freshness")
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
                        color: palette.mid
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
                        text: qsTr("Attention: %1").arg(bridge.attention_count)
                        color: bridge.attention_count > 0 ? "#a86213" : palette.mid
                        font.pixelSize: 12
                        Layout.fillWidth: true
                    }

                    Label {
                        text: qsTr("Folders unavailable: %1").arg(bridge.root_unavailable_count)
                        color: bridge.root_unavailable_count > 0 ? "#a86213" : palette.mid
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
                                    color: libraryDelegate.modelData.needsAttention
                                           ? "#a86213" : palette.mid
                                    font.pixelSize: 12
                                    elide: Text.ElideRight
                                    Layout.fillWidth: true
                                }
                            }

                            ToolButton {
                                text: qsTr("Sync")
                                enabled: libraryDelegate.modelData.canSync
                                          && !root.ui_bridge.sync_in_flight
                                Layout.alignment: Qt.AlignVCenter
                                onClicked: {
                                    root.ui_bridge.selectLibrary(libraryDelegate.modelData.libraryId)
                                    root.ui_bridge.syncLibrary(libraryDelegate.modelData.libraryId)
                                }
                                Accessible.name: qsTr("Request a sync for %1")
                                                  .arg(libraryDelegate.modelData.label)
                            }
                        }

                        onClicked: root.ui_bridge.selectLibrary(modelData.libraryId)
                    }

                    Label {
                        anchors.centerIn: parent
                        visible: libraryList.count === 0
                        text: qsTr("No libraries reported")
                        color: palette.mid
                    }
                }

                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 14
                    Layout.rightMargin: 14
                    Layout.bottomMargin: 10
                    visible: bridge.libraries_truncated
                    text: qsTr("Some libraries are hidden for safety.")
                    color: palette.mid
                    wrapMode: Text.WordWrap
                    font.pixelSize: 12
                }
            }
        }

        Pane {
            Layout.fillWidth: true
            Layout.fillHeight: true
            padding: 24

            ColumnLayout {
                anchors.fill: parent
                spacing: 18

                RowLayout {
                    Layout.fillWidth: true

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 4

                        Label {
                            text: bridge.selected_library_label
                            font.pixelSize: 25
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }

                        Label {
                            text: bridge.profile_message
                            color: palette.mid
                            font.pixelSize: 12
                            wrapMode: Text.WordWrap
                            Layout.fillWidth: true
                        }
                    }

                    Button {
                        text: qsTr("Sync Now")
                        enabled: bridge.selected_can_sync && !bridge.sync_in_flight
                        highlighted: true
                        onClicked: bridge.syncNow()
                        Accessible.name: qsTr("Request a sync")
                    }
                }

                Label {
                    Layout.fillWidth: true
                    visible: bridge.sync_feedback.length > 0
                    text: bridge.sync_feedback
                    color: palette.mid
                    wrapMode: Text.WordWrap
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
                            Label { text: qsTr("Runtime"); color: palette.mid; font.pixelSize: 12 }
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
                            Label { text: qsTr("Folder"); color: palette.mid; font.pixelSize: 12 }
                            Label {
                                text: bridge.selected_root_label
                                color: bridge.selected_root_label === qsTr("Folder available")
                                       ? palette.text : "#a86213"
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
                            Label { text: qsTr("Authentication"); color: palette.mid; font.pixelSize: 12 }
                            Label {
                                text: bridge.selected_auth_label
                                color: bridge.selected_auth_label === qsTr("Authentication ready")
                                       ? palette.text : "#a86213"
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
                            Label { text: qsTr("Conflicts"); color: palette.mid; font.pixelSize: 12 }
                            Label {
                                text: bridge.selected_conflict_label
                                color: bridge.selected_needs_attention ? "#a86213" : palette.text
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
                        color: palette.mid
                        wrapMode: Text.WordWrap
                        Layout.fillWidth: true
                    }

                    Label {
                        text: bridge.last_error_label
                        visible: bridge.last_error_label !== qsTr("No connection error")
                        color: "#a86213"
                        wrapMode: Text.WordWrap
                        Layout.fillWidth: true
                    }
                }

                Item { Layout.fillHeight: true }

                Label {
                    Layout.fillWidth: true
                    text: qsTr("Connection: %1  ·  %2").arg(bridge.connection_label).arg(bridge.connection_detail)
                    color: palette.mid
                    wrapMode: Text.WordWrap
                    font.pixelSize: 12
                }
            }
        }
    }
}
