// LinSpot Hotspot - Plasma 6 widget (panel icon + popup).
// All work is done by the `linspot` binary: `status`, `on`, `off`, `gui`.
// "on"/"off" ask for admin rights through polkit (the Plasma polkit agent shows the prompt).
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents3
import org.kde.plasma.plasma5support as P5Support
import org.kde.kirigami as Kirigami

PlasmoidItem {
    id: root

    readonly property string bin: "@LINSPOT_BIN@"

    property bool running: false
    property bool busy: false
    property string ssid: ""
    property int channel: 0
    property var clients: []
    property string lastError: ""
    property int seq: 0
    property bool qrVisible: false
    property string qrSource: ""
    property string qrFile: ""

    toolTipMainText: "LinSpot Hotspot"
    toolTipSubText: busy ? "Working ..."
                  : running ? (ssid + " - " + clients.length + " device(s)")
                  : "Off"
    Plasmoid.icon: "network-wireless-hotspot"
    preferredRepresentation: compactRepresentation

    // --------------------------------------------------------------- shell commands
    P5Support.DataSource {
        id: exec
        engine: "executable"
        connectedSources: []
        onNewData: (source, data) => {
            exec.disconnectSource(source)
            root.finished(source, data["exit code"], data["stdout"] || "", data["stderr"] || "")
        }
    }

    function run(tag, cmd) {
        seq += 1
        // the trailing comment makes every source string unique, so repeated polls always run
        exec.connectSource(cmd + " # " + tag + seq)
    }

    function finished(source, code, out, err) {
        if (source.indexOf("# status") >= 0) {
            parseStatus(out)
        } else if (source.indexOf("# qr") >= 0) {
            if (code === 0 && root.qrFile !== "") {
                root.qrSource = "file://" + root.qrFile
            } else if (code !== 0) {
                root.lastError = (err + out).trim().split("\n").slice(-1).join("") || "could not build the QR code"
            }
        } else if (source.indexOf("# on") >= 0 || source.indexOf("# off") >= 0) {
            busy = false
            if (code === 0) {
                lastError = ""
            } else if (code === 126 || code === 127) {
                lastError = "Authentication cancelled"
            } else {
                const lines = (err + out).trim().split("\n")
                lastError = lines.slice(-2).join(" ") || ("failed (exit " + code + ")")
            }
            poll()
        }
    }

    function parseStatus(out) {
        const parts = out.split("\n---\n")
        let list = []
        let runningNow = false
        try {
            const s = JSON.parse(parts[0])
            runningNow = !!s.running
            ssid = s.ssid || ""
            channel = s.channel || 0
            if (Array.isArray(s.client_list)) {
                // already filtered by the backend (kicked devices are hidden)
                list = s.client_list.map(c => ({ mac: c.mac, ip: c.ip, host: c.host }))
            }
        } catch (e) {
            runningNow = false
        }
        running = runningNow
        // fallback for older LinSpot binaries: read the leases file
        if (running && list.length === 0 && parts.length > 1) {
            const now = Math.floor(Date.now() / 1000)
            for (const line of parts[1].split("\n")) {
                const p = line.trim().split(/\s+/)
                if (p.length < 4) continue
                const exp = parseInt(p[0]) || 0
                if (exp !== 0 && exp <= now) continue
                list.push({ mac: p[1], ip: p[2], host: p[3] === "*" ? "unknown device" : p[3] })
            }
        }
        clients = list
    }

    function poll() {
        run("status", "'" + bin + "' status; echo; echo ---; cat /run/linspot/leases 2>/dev/null")
    }

    function toggle() {
        if (busy) return
        lastError = ""
        busy = true
        // passwordless first (a sudoers rule covers off/on where installed), polkit prompt otherwise
        const act = running ? "off" : "on"
        run(act, "sudo -n '" + bin + "' " + act + " 2>/dev/null || '" + bin + "' " + act)
    }

    function openApp() {
        run("gui", "nohup '" + bin + "' gui >/dev/null 2>&1 &")
    }

    function showQr() {
        if (!root.running) return
        // unique file name every time so the image always reloads
        root.qrFile = "/tmp/linspot-qr-" + Date.now() + ".png"
        run("qr", "'" + bin + "' qr --png '" + root.qrFile + "'")
    }

    Timer {
        interval: 3000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: root.poll()
    }

    // ------------------------------------------------------------------ panel icon
    compactRepresentation: MouseArea {
        id: compact
        hoverEnabled: true
        Layout.minimumWidth: Kirigami.Units.iconSizes.small
        Layout.minimumHeight: Kirigami.Units.iconSizes.small
        onClicked: root.expanded = !root.expanded

        Kirigami.Icon {
            anchors.fill: parent
            source: "network-wireless-hotspot"
            active: compact.containsMouse
            opacity: root.running ? 1.0 : 0.5
        }
        Rectangle {
            visible: root.running && root.clients.length > 0
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            width: Math.max(height, badge.implicitWidth + 4)
            height: Math.max(parent.height * 0.45, 10)
            radius: height / 2
            color: Kirigami.Theme.highlightColor
            PlasmaComponents3.Label {
                id: badge
                anchors.centerIn: parent
                text: root.clients.length
                color: Kirigami.Theme.highlightedTextColor
                font.pixelSize: parent.height * 0.75
            }
        }
    }

    // ------------------------------------------------------------------ popup
    fullRepresentation: ColumnLayout {
        Layout.preferredWidth: Kirigami.Units.gridUnit * 20
        Layout.minimumHeight: Kirigami.Units.gridUnit * 10
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            Kirigami.Icon {
                source: "network-wireless-hotspot"
                Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                Layout.preferredHeight: Kirigami.Units.iconSizes.medium
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                PlasmaComponents3.Label {
                    text: "LinSpot Hotspot"
                    font.bold: true
                }
                PlasmaComponents3.Label {
                    text: root.busy ? "Working ..."
                        : root.running ? ("On - " + root.ssid + " (ch " + root.channel + ")")
                        : "Off"
                    opacity: 0.7
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
            }
            PlasmaComponents3.Switch {
                checked: root.running
                enabled: !root.busy
                // the switch follows the real hotspot state, a click only requests a change
                onToggled: { checked = root.running; root.toggle() }
            }
        }

        PlasmaComponents3.Label {
            visible: root.lastError !== ""
            text: root.lastError
            color: Kirigami.Theme.negativeTextColor
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
        }

        Kirigami.Separator { Layout.fillWidth: true }

        PlasmaComponents3.Label {
            visible: root.running && root.clients.length === 0
            text: "No devices connected"
            opacity: 0.7
        }
        PlasmaComponents3.Label {
            visible: !root.running && !root.busy
            text: "Set the network name and password once in the LinSpot app, then use the switch."
            opacity: 0.7
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
        }

        Repeater {
            model: root.clients
            delegate: RowLayout {
                required property var modelData
                Layout.fillWidth: true
                Kirigami.Icon {
                    source: "smartphone"
                    Layout.preferredWidth: Kirigami.Units.iconSizes.small
                    Layout.preferredHeight: Kirigami.Units.iconSizes.small
                }
                PlasmaComponents3.Label {
                    text: modelData.host
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                }
                PlasmaComponents3.Label {
                    text: modelData.ip
                    opacity: 0.7
                }
            }
        }

        Kirigami.Separator { Layout.fillWidth: true }

        RowLayout {
            Layout.fillWidth: true
            PlasmaComponents3.Button {
                visible: root.running
                text: root.qrVisible ? "Hide QR" : "Show QR"
                icon.name: "view-barcode-qr"
                enabled: !root.busy
                onClicked: {
                    if (root.qrVisible) {
                        root.qrVisible = false
                        root.qrSource = ""
                    } else {
                        root.qrVisible = true
                        root.showQr()
                    }
                }
            }
            Item { Layout.fillWidth: true }
            PlasmaComponents3.Button {
                text: "Open LinSpot"
                icon.name: "preferences-system-network"
                onClicked: root.openApp()
            }
        }

        PlasmaComponents3.Item {
            visible: root.qrVisible && root.qrSource !== ""
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 11
            Image {
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                fillMode: Image.PreserveAspectFit
                source: root.qrSource
                cache: false
            }
        }
        PlasmaComponents3.Label {
            visible: root.qrVisible && root.qrSource === ""
            text: "Building the QR code ..."
            opacity: 0.7
        }

        Item { Layout.fillHeight: true }
    }
}
