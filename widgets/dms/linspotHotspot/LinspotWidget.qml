import QtQuick
import Quickshell
import Quickshell.Io
import qs.Common
import qs.Services
import qs.Widgets
import qs.Modules.Plugins

PluginComponent {
    id: root

    readonly property string pid: "linspotHotspot"

    // ------------------------------------------------------------ settings
    property string binOverride: ""
    property string cfgOverride: ""
    property int pollSeconds: 3

    function loadSettings() {
        if (!pluginService) return
        binOverride = String(pluginService.loadPluginData(pid, "linspotBin", "") || "")
        cfgOverride = String(pluginService.loadPluginData(pid, "configPath", "") || "")
        const n = parseInt(pluginService.loadPluginData(pid, "pollSeconds", "3"))
        pollSeconds = isNaN(n) || n < 1 ? 3 : n
    }

    Connections {
        target: root.pluginService
        function onPluginDataChanged(changedId) {
            if (changedId === root.pid) root.loadSettings()
        }
    }

    // ------------------------------------------------------------- binary
    property string detectedBin: ""
    readonly property string linspotBin: binOverride !== "" ? binOverride : detectedBin

    // pkexec needs the path the polkit policy was installed for (/etc/linspot/install.info)
    Process {
        id: detectProc
        command: ["sh", "-c",
            ". /etc/linspot/install.info 2>/dev/null; " +
            "if [ -n \"$BINDIR\" ] && [ -x \"$BINDIR/linspot\" ]; then echo \"$BINDIR/linspot\"; " +
            "else command -v linspot; fi"]
        stdout: StdioCollector {
            onStreamFinished: {
                root.detectedBin = text.trim()
                root.refresh()
            }
        }
    }

    // ------------------------------------------------------------- state
    property bool running: false
    property bool busy: false
    property string busyText: ""
    property string ssid: ""
    property int channel: 0
    property int clientCount: 0
    property var clients: []
    property string lastError: ""

    property string cfgSsid: "LinSpot"
    property string cfgPassword: ""
    property string cfgBand: "auto"
    property int cfgChannel: 0
    property string cfgIface: ""
    property bool cfgLoaded: false
    property string pendingCfg: ""

    readonly property string configFile: cfgOverride !== "" ? cfgOverride
        : ((Quickshell.env("XDG_CONFIG_HOME") || (Quickshell.env("HOME") + "/.config")) + "/linspot/config.json")

    Process {
        id: cfgReader
        command: ["cat", root.configFile]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const c = JSON.parse(text)
                    root.cfgSsid = c.ssid || "LinSpot"
                    root.cfgPassword = c.password || ""
                    root.cfgBand = c.band || "auto"
                    root.cfgChannel = c.channel || 0
                    root.cfgIface = c.iface || ""
                    root.cfgLoaded = true
                } catch (e) {
                    root.cfgLoaded = false
                }
            }
        }
    }

    // ------------------------------------------------------------ polling
    Process {
        id: statusProc
        command: [root.linspotBin, "status"]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const s = JSON.parse(text)
                    root.running = !!s.running
                    root.ssid = s.ssid || ""
                    root.channel = s.channel || 0
                    root.clientCount = s.clients || 0
                } catch (e) {
                    root.running = false
                }
                if (root.running) leasesProc.running = true
                else { root.clients = []; root.clientCount = 0 }
            }
        }
    }

    Process {
        id: leasesProc
        command: ["cat", "/run/linspot/leases"]
        stdout: StdioCollector {
            onStreamFinished: {
                const now = Math.floor(Date.now() / 1000)
                const out = []
                for (const line of text.split("\n")) {
                    const p = line.trim().split(/\s+/)
                    if (p.length < 4) continue
                    const exp = parseInt(p[0]) || 0
                    if (exp !== 0 && exp <= now) continue
                    out.push({ mac: p[1], ip: p[2], host: p[3] === "*" ? "unknown device" : p[3] })
                }
                root.clients = out
                root.clientCount = out.length
            }
        }
    }

    Timer {
        interval: root.pollSeconds * 1000
        running: true
        repeat: true
        onTriggered: root.refresh()
    }

    function refresh() {
        if (linspotBin === "") return
        if (!statusProc.running) statusProc.running = true
    }

    // ------------------------------------------------------------ actions
    // The config goes to linspot through an environment variable + pipe (never on a command line,
    // so the Wi-Fi password is not visible in `ps`).
    Process {
        id: startProc
        environment: ({ "LINSPOT_CFG": root.pendingCfg })
        command: ["sh", "-c", "printf '%s' \"$LINSPOT_CFG\" | pkexec \"$1\" start --stdin", "sh", root.linspotBin]
        stderr: StdioCollector { id: startErr }
        stdout: StdioCollector { id: startOut }
        onExited: (code) => {
            root.busy = false
            root.busyText = ""
            root.pendingCfg = ""
            if (code === 0) {
                root.lastError = ""
                ToastService.showInfo("Hotspot on", root.cfgSsid)
            } else if (code === 126 || code === 127) {
                root.lastError = "Authentication cancelled"
                ToastService.showWarning("Hotspot", "Authentication cancelled")
            } else {
                const msg = (startErr.text + startOut.text).trim().split("\n").slice(-3).join(" ")
                root.lastError = msg || ("start failed (exit " + code + ")")
                ToastService.showError("Hotspot failed", root.lastError)
            }
            root.refresh()
        }
    }

    Process {
        id: stopProc
        command: ["pkexec", root.linspotBin, "stop"]
        stderr: StdioCollector { id: stopErr }
        onExited: (code) => {
            root.busy = false
            root.busyText = ""
            if (code === 0) {
                root.lastError = ""
                ToastService.showInfo("Hotspot off")
            } else if (code === 126 || code === 127) {
                ToastService.showWarning("Hotspot", "Authentication cancelled")
            } else {
                root.lastError = stopErr.text.trim().split("\n").slice(-2).join(" ")
                ToastService.showError("Stop failed", root.lastError)
            }
            root.refresh()
        }
    }

    property string kickMac: ""
    Process {
        id: kickProc
        command: ["pkexec", "hostapd_cli", "-p", "/run/linspot/hostapd", "-i", "hs0", "deauthenticate", root.kickMac]
        onExited: root.refresh()
    }

    Process {
        id: guiProc
        command: [root.linspotBin, "gui"]
    }

    function startHotspot() {
        if (busy || running || linspotBin === "") return
        if (!cfgLoaded || cfgPassword.length < 8) {
            ToastService.showWarning("LinSpot", "Set an SSID and a password (8+ chars) in the LinSpot app first")
            openGui()
            return
        }
        busy = true
        busyText = "Starting hotspot…"
        pendingCfg = JSON.stringify({
            ssid: cfgSsid,
            password: cfgPassword,
            iface: cfgIface === "" ? null : cfgIface,
            channel: cfgChannel === 0 ? null : cfgChannel,
            band: cfgBand,
            country: null
        })
        startProc.running = true
    }

    function stopHotspot() {
        if (busy || !running || linspotBin === "") return
        busy = true
        busyText = "Stopping hotspot…"
        stopProc.running = true
    }

    function toggleHotspot() {
        if (running) stopHotspot()
        else startHotspot()
    }

    function kickClient(mac) {
        kickMac = mac
        kickProc.running = true
    }

    function openGui() {
        if (linspotBin !== "" && !guiProc.running) guiProc.running = true
    }

    Component.onCompleted: {
        loadSettings()
        cfgReader.running = true
        detectProc.running = true
    }

    // --------------------------------------------------------- bar pill
    // left click -> manage popout, right click -> toggle the hotspot
    pillRightClickAction: () => root.toggleHotspot()

    horizontalBarPill: Component {
        StyledRect {
            width: pillRow.implicitWidth + Theme.spacingM * 2
            height: parent.widgetThickness
            radius: Theme.cornerRadius
            color: Theme.chipSurface

            Row {
                id: pillRow
                anchors.centerIn: parent
                spacing: Theme.spacingXS
                DankIcon {
                    name: root.busy ? "sync" : (root.running ? "wifi_tethering" : "wifi_tethering_off")
                    size: Theme.iconSize - 6
                    color: root.running ? Theme.primary : Theme.surfaceText
                    anchors.verticalCenter: parent.verticalCenter
                }
                StyledText {
                    visible: root.running
                    text: root.clientCount.toString()
                    font.pixelSize: Theme.fontSizeSmall
                    color: Theme.surfaceText
                    anchors.verticalCenter: parent.verticalCenter
                }
            }
        }
    }

    verticalBarPill: Component {
        StyledRect {
            width: parent.widgetThickness
            height: pillCol.implicitHeight + Theme.spacingM * 2
            radius: Theme.cornerRadius
            color: Theme.chipSurface

            Column {
                id: pillCol
                anchors.centerIn: parent
                spacing: Theme.spacingXS
                DankIcon {
                    name: root.busy ? "sync" : (root.running ? "wifi_tethering" : "wifi_tethering_off")
                    size: Theme.iconSize - 6
                    color: root.running ? Theme.primary : Theme.surfaceText
                    anchors.horizontalCenter: parent.horizontalCenter
                }
                StyledText {
                    visible: root.running
                    text: root.clientCount.toString()
                    font.pixelSize: Theme.fontSizeSmall
                    color: Theme.surfaceText
                    anchors.horizontalCenter: parent.horizontalCenter
                }
            }
        }
    }

    // --------------------------------------------------- control center tile
    ccWidgetIcon: root.running ? "wifi_tethering" : "wifi_tethering_off"
    ccWidgetPrimaryText: "Hotspot"
    ccWidgetSecondaryText: root.busy ? root.busyText
        : (root.running ? (root.ssid + " · " + root.clientCount + (root.clientCount === 1 ? " device" : " devices"))
                        : "Off")
    ccWidgetIsActive: root.running
    onCcWidgetToggled: root.toggleHotspot()
    ccDetailContent: managePanel

    // shared management panel (control-center detail page + bar popout)
    property Component managePanel: Component {
        Rectangle {
            id: panel
            implicitHeight: col.implicitHeight + Theme.spacingM * 2
            height: implicitHeight
            radius: Theme.cornerRadius
            color: Theme.surfaceContainerHigh

            Column {
                id: col
                x: Theme.spacingM
                y: Theme.spacingM
                width: panel.width - Theme.spacingM * 2
                spacing: Theme.spacingS

                StyledText {
                    width: parent.width
                    wrapMode: Text.WordWrap
                    text: root.running ? ("Sharing “" + root.ssid + "” on channel " + root.channel) : "Hotspot is off"
                    font.pixelSize: Theme.fontSizeMedium
                    font.weight: Font.Medium
                    color: Theme.surfaceText
                }
                StyledText {
                    visible: root.lastError !== ""
                    width: parent.width
                    wrapMode: Text.WordWrap
                    text: root.lastError
                    font.pixelSize: Theme.fontSizeSmall
                    color: Theme.error
                }
                StyledText {
                    visible: root.running && root.clients.length === 0
                    text: "No devices connected yet"
                    font.pixelSize: Theme.fontSizeSmall
                    color: Theme.surfaceVariantText
                }

                Repeater {
                    model: root.clients
                    delegate: Item {
                        width: col.width
                        height: 48
                        DankIcon {
                            id: devIcon
                            name: "smartphone"
                            size: 22
                            color: Theme.primary
                            anchors.left: parent.left
                            anchors.verticalCenter: parent.verticalCenter
                        }
                        Column {
                            anchors.left: devIcon.right
                            anchors.leftMargin: Theme.spacingS
                            anchors.right: kickBtn.left
                            anchors.rightMargin: Theme.spacingS
                            anchors.verticalCenter: parent.verticalCenter
                            StyledText {
                                width: parent.width
                                elide: Text.ElideRight
                                text: modelData.host
                                font.pixelSize: Theme.fontSizeMedium
                                color: Theme.surfaceText
                            }
                            StyledText {
                                width: parent.width
                                elide: Text.ElideRight
                                text: modelData.ip + "  ·  " + modelData.mac
                                font.pixelSize: Theme.fontSizeSmall
                                color: Theme.surfaceVariantText
                            }
                        }
                        LsButton {
                            id: kickBtn
                            iconName: "block"
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            onClicked: root.kickClient(modelData.mac)
                        }
                    }
                }

                Row {
                    spacing: Theme.spacingS
                    LsButton {
                        text: root.busy ? root.busyText : (root.running ? "Turn off" : "Turn on")
                        iconName: root.running ? "wifi_tethering_off" : "wifi_tethering"
                        primary: !root.running
                        active: !root.busy
                        onClicked: root.toggleHotspot()
                    }
                    LsButton {
                        text: "Open LinSpot"
                        iconName: "settings"
                        onClicked: root.openGui()
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ bar popout
    popoutWidth: 380
    popoutHeight: 420
    popoutContent: Component {
        PopoutComponent {
            headerText: "Hotspot"
            detailsText: root.running ? ("Sharing " + root.ssid) : "Off"
            Loader {
                width: parent.width
                sourceComponent: root.managePanel
            }
        }
    }
}
