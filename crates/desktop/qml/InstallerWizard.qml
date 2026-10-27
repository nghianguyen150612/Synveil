import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

pragma ComponentBehavior: Bound

Pane {
    id: root
    required property var bridge
    readonly property var wizardView: bridge.installer_wizard_view
    objectName: "installerWizard"
    visible: wizardView.visible
    z: 100
    padding: 0
    focus: visible

    background: Rectangle {
        color: palette.window
    }

    function deviceRoleName(code) {
        if (code === "host") return qsTr("Set up a Synveil host")
        if (code === "client") return qsTr("Connect this machine as a client")
        if (code === "existing") return qsTr("Configure an existing installation")
        return qsTr("No device role selected")
    }

    function connectionModeName(code) {
        if (code === "local_only") return qsTr("Local-only")
        if (code === "private_lan") return qsTr("Private LAN")
        if (code === "manual") return qsTr("Advanced / manual configuration")
        if (code === "tailscale") return qsTr("Tailscale")
        return qsTr("No connection selected")
    }

    function platformName(code) {
        if (code === "linux") return qsTr("Linux")
        if (code === "windows") return qsTr("Windows")
        if (code === "macos") return qsTr("macOS")
        return qsTr("Other")
    }

    function errorMessage(code) {
        if (code === "safe_preflight_failed") {
            return qsTr("The read-only preview check could not finish. No installation was attempted.")
        }
        if (code === "invalid_selection") {
            return qsTr("That choice is not supported here. Select an available option.")
        }
        return qsTr("This wizard step is unavailable. Return to the previous step or close the preview.")
    }

    function unavailableMessage(code) {
        if (code === "tailscale_planned") {
            return qsTr("Tailscale support is planned. This build has no Tailscale adapter and cannot enroll or connect a device.")
        }
        if (code === "unsupported_platform") {
            return qsTr("No supported setup path is available for this platform.")
        }
        return qsTr("The selected setup capability is unavailable on this device.")
    }

    ScrollView {
        id: scrollView
        anchors.fill: parent
        clip: true
        contentWidth: availableWidth
        ScrollBar.vertical.policy: ScrollBar.AsNeeded

        ColumnLayout {
            width: scrollView.availableWidth
            spacing: 0

            Pane {
                Layout.alignment: Qt.AlignHCenter
                Layout.fillWidth: true
                Layout.maximumWidth: 780
                padding: 28

                ColumnLayout {
                    anchors.fill: parent
                    spacing: 18

                    RowLayout {
                        Layout.fillWidth: true
                        Label {
                            id: wizardTitle
                            Layout.fillWidth: true
                            focus: true
                            text: qsTr("Easy Setup preview")
                            font.pixelSize: 28
                            font.weight: Font.DemiBold
                        }
                        Button {
                            text: qsTr("Cancel")
                            Accessible.name: qsTr("Close Easy Setup preview and return to Welcome")
                            onClicked: root.bridge.cancelInstallerWizard()
                        }
                    }

                    Label {
                        Layout.fillWidth: true
                        text: qsTr("Choose a device role, review a connection preference, then check what this build can actually preview.")
                        wrapMode: Text.WordWrap
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12
                        Repeater {
                            model: [qsTr("1 · This device"), qsTr("2 · Connection"), qsTr("3 · Review")]
                            delegate: Label {
                                required property int index
                                required property string modelData
                                Layout.fillWidth: true
                                text: modelData
                                font.weight: index === root.wizardView.step
                                             ? Font.DemiBold : Font.Normal
                                color: index <= root.wizardView.step
                                       ? palette.highlight : palette.mid
                                Accessible.name: index === root.wizardView.step
                                                 ? qsTr("Current step: %1").arg(modelData)
                                                 : modelData
                            }
                        }
                    }

                    ProgressBar {
                        Layout.fillWidth: true
                        from: 0
                        to: 2
                        value: root.wizardView.step
                        Accessible.name: qsTr("Easy Setup progress, step %1 of 3")
                                         .arg(root.wizardView.step + 1)
                    }

                    Label {
                        Layout.fillWidth: true
                        visible: root.wizardView.state === "unavailable"
                        text: root.unavailableMessage(root.wizardView.unavailableCode)
                        color: palette.brightText
                        wrapMode: Text.WordWrap
                        Accessible.name: text
                    }

                    Label {
                        Layout.fillWidth: true
                        visible: root.wizardView.state === "error"
                        text: root.errorMessage(root.wizardView.errorCode)
                        color: palette.brightText
                        wrapMode: Text.WordWrap
                        Accessible.name: text
                    }
                    Button {
                        visible: root.wizardView.state === "error"
                        text: root.wizardView.detailsOpen
                              ? qsTr("Hide details") : qsTr("Show safe details")
                        Accessible.name: text
                        onClicked: root.bridge.setInstallerWizardDetailsOpen(!root.wizardView.detailsOpen)
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: root.wizardView.state === "error"
                                 && root.wizardView.detailsOpen
                        text: qsTr("The check did not complete. No installation effect was started. Raw diagnostics are omitted. Retry is available only for the safe read-only check.")
                        wrapMode: Text.WordWrap
                        Accessible.name: text
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        visible: root.wizardView.step === 0
                                 && root.wizardView.state === "device_choice"
                        spacing: 12

                        Label {
                            Layout.fillWidth: true
                            visible: !root.wizardView.hostSupported
                                     && !root.wizardView.clientSupported
                                     && !root.wizardView.existingSupported
                            text: qsTr("No supported setup path is available on this platform.")
                            wrapMode: Text.WordWrap
                            Accessible.name: text
                        }

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("This device")
                            font.pixelSize: 20
                            font.weight: Font.DemiBold
                        }
                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Detected platform: %1").arg(root.wizardView.platformSummary)
                            wrapMode: Text.WordWrap
                            Accessible.name: text
                        }
                        Label {
                            Layout.fillWidth: true
                            visible: root.wizardView.deviceRole === "client"
                            text: qsTr("Recommended for most people: connect this machine to a Synveil server that is already set up.")
                            wrapMode: Text.WordWrap
                        }
                        Label {
                            Layout.fillWidth: true
                            visible: root.wizardView.deviceRole === "host"
                            text: qsTr("Recommended on this supported platform: use the existing managed Host setup.")
                            wrapMode: Text.WordWrap
                        }

                        RadioButton {
                            id: hostRole
                            Layout.fillWidth: true
                            text: qsTr("Set up a Synveil host")
                            checked: root.wizardView.deviceRole === "host"
                            enabled: root.wizardView.hostSupported
                                     && root.wizardView.state === "device_choice"
                            Accessible.name: text
                            Accessible.description: enabled
                                ? qsTr("The existing managed Host setup supports this platform. This wizard only previews choices.")
                                : qsTr("Unavailable. Managed Host setup currently supports Ubuntu 24.04 and Fedora 42 on x86-64.")
                            onClicked: root.bridge.selectInstallerWizardDevice("host")
                        }
                        Label {
                            Layout.fillWidth: true
                            leftPadding: 36
                            text: root.wizardView.hostSupported
                                  ? qsTr("The existing Host setup handles server preparation and its own reviewed authorization flow. This preview does not start it.")
                                  : qsTr("Unavailable here. Managed Host setup currently supports Ubuntu 24.04 and Fedora 42 on x86-64.")
                            wrapMode: Text.WordWrap
                        }

                        RadioButton {
                            id: clientRole
                            Layout.fillWidth: true
                            text: qsTr("Connect this machine as a client")
                            checked: root.wizardView.deviceRole === "client"
                            enabled: root.wizardView.clientSupported
                                     && root.wizardView.state === "device_choice"
                            Accessible.name: text
                            Accessible.description: qsTr("Uses the existing desktop connection setup. The server address and sign-in are handled there.")
                            onClicked: root.bridge.selectInstallerWizardDevice("client")
                        }
                        Label {
                            Layout.fillWidth: true
                            leftPadding: 36
                            text: root.wizardView.clientSupported
                                  ? qsTr("Available through the existing Connect setup.")
                                  : qsTr("Unavailable because this desktop profile could not be loaded.")
                            wrapMode: Text.WordWrap
                        }

                        RadioButton {
                            Layout.fillWidth: true
                            text: qsTr("Configure an existing installation")
                            checked: root.wizardView.deviceRole === "existing"
                            enabled: root.wizardView.existingSupported
                                     && root.wizardView.state === "device_choice"
                            Accessible.name: text
                            Accessible.description: qsTr("Unavailable from first-run Welcome because no configured installation is detected.")
                        }
                        Label {
                            Layout.fillWidth: true
                            leftPadding: 36
                            text: qsTr("Unavailable from this first-run screen. Open an existing Host or connected client from the main desktop.")
                            wrapMode: Text.WordWrap
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        visible: root.wizardView.step === 1
                                 && root.wizardView.state === "connection_choice"
                        spacing: 12

                        Label {
                            Layout.fillWidth: true
                            visible: root.wizardView.previewInvalidated
                            text: qsTr("A previous preview was invalidated because a setup choice changed. Review the current choices again.")
                            wrapMode: Text.WordWrap
                            Accessible.name: text
                        }

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Connection")
                            font.pixelSize: 20
                            font.weight: Font.DemiBold
                        }
                        Label {
                            Layout.fillWidth: true
                            text: qsTr("This is a preview preference only. It will not save an endpoint, change a listener, or grant sign-in access.")
                            wrapMode: Text.WordWrap
                        }

                        RadioButton {
                            Layout.fillWidth: true
                            text: qsTr("Local-only")
                            checked: root.wizardView.connectionMode === "local_only"
                            enabled: root.wizardView.localOnlySupported
                                     && root.wizardView.state === "connection_choice"
                            Accessible.name: text
                            Accessible.description: qsTr("Uses the existing local-only setup path where supported. The wizard does not inspect or change it.")
                            onClicked: root.bridge.selectInstallerWizardConnection("local_only")
                        }
                        RadioButton {
                            Layout.fillWidth: true
                            text: qsTr("Private LAN")
                            checked: root.wizardView.connectionMode === "private_lan"
                            enabled: root.wizardView.privateLanSupported
                                     && root.wizardView.state === "connection_choice"
                            Accessible.name: text
                            Accessible.description: qsTr("The current Host setup or explicit HTTPS server address can use a private LAN. No network settings are changed here.")
                            onClicked: root.bridge.selectInstallerWizardConnection("private_lan")
                        }
                        RadioButton {
                            Layout.fillWidth: true
                            text: qsTr("Advanced / manual configuration")
                            checked: root.wizardView.connectionMode === "manual"
                            enabled: root.wizardView.manualSupported
                                     && root.wizardView.state === "connection_choice"
                            Accessible.name: text
                            Accessible.description: qsTr("The existing desktop Connect flow accepts a server address. No address is entered or saved in this preview.")
                            onClicked: root.bridge.selectInstallerWizardConnection("manual")
                        }
                        RadioButton {
                            Layout.fillWidth: true
                            text: qsTr("Tailscale — planned, not connected")
                            checked: root.wizardView.connectionMode === "tailscale"
                            enabled: false
                            Accessible.name: text
                            Accessible.description: qsTr("Unavailable. Synveil has no Tailscale adapter or enrollment integration.")
                        }
                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Network reachability is not login authorization. Authentication and device enrollment stay in the current client flow.")
                            wrapMode: Text.WordWrap
                        }

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Installation channel")
                            font.pixelSize: 20
                            font.weight: Font.DemiBold
                        }
                        Label {
                            Layout.fillWidth: true
                            text: qsTr("Choose the channel to inspect. The check identifies support and missing evidence; it never switches channels or starts installation.")
                            wrapMode: Text.WordWrap
                        }
                        Repeater {
                            model: root.wizardView.channelOptions
                            delegate: RadioButton {
                                required property var modelData
                                Layout.fillWidth: true
                                text: qsTr("%1 — %2")
                                      .arg(modelData.label)
                                      .arg(modelData.availability)
                                checked: modelData.selected
                                enabled: root.wizardView.state === "connection_choice"
                                Accessible.name: text
                                Accessible.description: modelData.active
                                    ? qsTr("A channel marker is present. Its artifact identity and ownership still need verification.")
                                    : qsTr("This channel will be checked against bounded, read-only host evidence.")
                                onClicked: root.bridge.selectInstallerWizardChannel(modelData.code)
                            }
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        visible: root.wizardView.step === 2
                        spacing: 12

                        Label {
                            Layout.fillWidth: true
                            text: root.wizardView.state === "error"
                                  ? qsTr("Preview check needs attention")
                                  : qsTr("Review setup choices")
                            font.pixelSize: 20
                            font.weight: Font.DemiBold
                        }
                        BusyIndicator {
                            Layout.alignment: Qt.AlignHCenter
                            running: root.wizardView.busy
                            visible: running
                            Accessible.name: qsTr("Checking safe preview availability")
                        }
                        Label {
                            Layout.fillWidth: true
                            visible: root.wizardView.busy
                            text: qsTr("Checking the current build’s read-only setup capabilities…")
                            wrapMode: Text.WordWrap
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: !root.wizardView.busy
                            spacing: 10

                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Installation intent: preview only; no install action was requested.")
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Device role: %1").arg(root.deviceRoleName(root.wizardView.deviceRole))
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Connection preference: %1").arg(root.connectionModeName(root.wizardView.connectionMode))
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Detected platform: %1").arg(root.wizardView.platformSummary)
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Role capability evidence: Host setup %1; client connection setup %2.")
                                      .arg(root.wizardView.hostSupported ? qsTr("supported") : qsTr("unavailable"))
                                      .arg(root.wizardView.clientSupported ? qsTr("supported") : qsTr("unavailable"))
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: !root.wizardView.reportReady
                                      ? qsTr("Read-only capability check: not completed.")
                                      : root.wizardView.evidenceComplete
                                        ? qsTr("Read-only capability check: completed with operating-system evidence.")
                                        : qsTr("Read-only capability check: completed with incomplete operating-system evidence.")
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: root.wizardView.preflightRan
                                      ? qsTr("InstallerEngine preflight: completed by a read-only adapter.")
                                      : qsTr("InstallerEngine preflight: not run. This inspection does not call InstallationAdapter::preflight or create an installation plan.")
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Artifact evidence: %1.").arg(root.wizardView.artifactSummary)
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: root.wizardView.planSummary
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Installation channel: %1").arg(root.wizardView.selectedChannel.length > 0
                                      ? root.wizardView.selectedChannel : qsTr("not selected"))
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Detected channel evidence:\n%1").arg(root.wizardView.channelSummary)
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Target scope: %1").arg(root.wizardView.scopeSummary)
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: root.wizardView.privilegeAssessed
                                      ? qsTr("Privilege requirement: reported by the authoritative plan preview.")
                                      : qsTr("Privilege requirement: not assessed. No installation effect or native authorization was requested.")
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Ordered operations and resources to be touched: unavailable until a verified artifact and authoritative plan exist.")
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Preservation: this inspection changes no files, credentials, settings, server state, network listeners, or installer journal. Eventual installation preservation expectations are unavailable without a validated plan.")
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Reversibility, verification requirements, and confirmation prerequisites: unavailable without an authoritative plan. No installation can be started from this wizard.")
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                visible: root.wizardView.blockingFinding
                                text: qsTr("This inspection has blocking findings. Review the safe findings below.")
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Connection capability: %1").arg(root.wizardView.connectionSummary)
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: root.wizardView.findingSummary
                                wrapMode: Text.WordWrap
                            }
                            Button {
                                text: root.wizardView.detailsOpen
                                      ? qsTr("Hide technical details") : qsTr("Show safe technical details")
                                Accessible.name: text
                                onClicked: root.bridge.setInstallerWizardDetailsOpen(!root.wizardView.detailsOpen)
                            }
                            Label {
                                Layout.fillWidth: true
                                visible: root.wizardView.detailsOpen
                                text: qsTr("The inspection reports bounded platform and channel categories. Raw paths, command output, credentials, and remote endpoint details are not shown.")
                                wrapMode: Text.WordWrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Preview only — installation is not started.")
                                font.weight: Font.DemiBold
                                wrapMode: Text.WordWrap
                                Accessible.name: text
                            }
                            Label {
                                Layout.fillWidth: true
                                text: qsTr("Safe next action: resolve the missing artifact or capability evidence, then use the existing Host or Connect flow for its own review and authorization boundaries.")
                                wrapMode: Text.WordWrap
                            }
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        Button {
                            text: qsTr("Back")
                            enabled: root.wizardView.canBack
                                     && !root.wizardView.busy
                            Accessible.name: qsTr("Go back one Easy Setup step")
                            onClicked: root.bridge.backInstallerWizard()
                        }
                        Item { Layout.fillWidth: true }
                        Button {
                            visible: root.wizardView.canRetry
                            text: qsTr("Retry safe check")
                            Accessible.name: text
                            onClicked: root.bridge.retryInstallerWizardPreflight()
                        }
                        Button {
                            visible: root.wizardView.step < 2
                            enabled: root.wizardView.canContinue
                                     && !root.wizardView.busy
                            text: root.wizardView.step === 0
                                  ? qsTr("Continue") : qsTr("Review choices")
                            Accessible.name: text
                            onClicked: root.bridge.continueInstallerWizard()
                        }
                        Button {
                            visible: root.wizardView.step === 2
                                     && root.wizardView.state !== "preflight"
                            text: qsTr("Finish preview")
                            Accessible.name: qsTr("Finish preview and return to Welcome")
                            onClicked: root.bridge.cancelInstallerWizard()
                        }
                    }
                }
            }
        }
    }

    onVisibleChanged: {
        if (visible) {
            wizardTitle.forceActiveFocus(Qt.TabFocusReason)
        }
    }

    Keys.onEscapePressed: root.bridge.cancelInstallerWizard()
}
