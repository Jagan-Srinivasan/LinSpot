import QtQuick
import Quickshell
import Quickshell.Io
import qs.Services

Item {
    id: root

    // set by DMS
    property var pluginService: null
    property string trigger: "hs"

    signal itemsChanged()

    // -------------------------------------------------------------- state
    property bool running: false
    property string ssid: ""
    property int channel: 0
    property int clientCount: 0
    property var clientList: []
    property string linspotBin: ""

    Process {
        id: detectProc
        command: ["sh", "-c",
            ". /etc/linspot/install.info 2>/dev/null; " +
            "if [ -n \"$BINDIR\" ] && [ -x \"$BINDIR/linspot\" ]; then echo \"$BINDIR/linspot\"; " +
            "else command -v linspot; fi"]
        stdout: StdioCollector {
            onStreamFinished: {
                if (root.linspotBin === "") root.linspotBin = text.trim()
                root.refresh()
            }
        }
    }
    property string pendingCfg: ""
    property string kickMac: ""

    property string cfgSsid: "LinSpot"
    property string cfgPassword: ""
    property string cfgBand: "auto"
    property int cfgChannel: 0
    property string cfgIface: ""
    property bool cfgLoaded: false

    readonly property string configFile:
        (Quickshell.env("XDG_CONFIG_HOME") || (Quickshell.env("HOME") + "/.config")) + "/linspot/config.json"

    Component.onCompleted: {
        if (pluginService) {
            trigger = pluginService.loadPluginData("linspotLauncher", "trigger", "hs")
            const b = pluginService.loadPluginData("linspotLauncher", "linspotBin", "")
            if (b) linspotBin = b
        }
        detectProc.running = true
    }

    function refresh() {
        if (linspotBin === "") return
        if (!cfgReader.running) cfgReader.running = true
        if (!statusProc.running) statusProc.running = true
    }

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
                } catch (e) { root.cfgLoaded = false }
            }
        }
    }

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
                } catch (e) { root.running = false }
                if (root.running) leasesProc.running = true
                else { root.clientList = []; root.itemsChanged() }
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
                root.clientList = out
                root.clientCount = out.length
                root.itemsChanged()
            }
        }
    }

    // ------------------------------------------------------------ actions
    Process {
        id: startProc
        environment: ({ "LINSPOT_CFG": root.pendingCfg })
        command: ["sh", "-c", "printf '%s' \"$LINSPOT_CFG\" | pkexec \"$1\" start --stdin", "sh", root.linspotBin]
        stderr: StdioCollector { id: startErr }
        onExited: (code) => {
            if (code === 0) ToastService.showInfo("Hotspot on", root.cfgSsid)
            else if (code === 126 || code === 127) ToastService.showWarning("Hotspot", "Authentication cancelled")
            else ToastService.showError("Hotspot failed", startErr.text.trim().split("\n").slice(-2).join(" "))
            root.refresh()
        }
    }

    Process {
        id: stopProc
        command: ["pkexec", root.linspotBin, "stop"]
        stderr: StdioCollector { id: stopErr }
        onExited: (code) => {
            if (code === 0) ToastService.showInfo("Hotspot off")
            else if (code === 126 || code === 127) ToastService.showWarning("Hotspot", "Authentication cancelled")
            else ToastService.showError("Stop failed", stopErr.text.trim().split("\n").slice(-2).join(" "))
            root.refresh()
        }
    }

    Process {
        id: kickProc
        command: ["pkexec", "hostapd_cli", "-p", "/run/linspot/hostapd", "-i", "hs0", "deauthenticate", root.kickMac]
        onExited: root.refresh()
    }
    Process { id: guiProc; command: [root.linspotBin, "gui"] }
    Process {
        id: qrProc
        command: ["sh", "-c", "'$1' qr --png /tmp/linspot-qr.png && xdg-open /tmp/linspot-qr.png", "sh", root.linspotBin]
        stderr: StdioCollector { id: qrErr }
        onExited: (code) => {
            if (code !== 0) ToastService.showError("QR code failed", qrErr.text.trim().split("\n").slice(-1).join(" "))
        }
    }

    function startHotspot() {
        if (running || startProc.running) return
        if (!cfgLoaded || cfgPassword.length < 8) {
            ToastService.showWarning("LinSpot", "Set SSID and a password (8+ chars) in the LinSpot app first")
            guiProc.running = true
            return
        }
        const cfg = {
            ssid: cfgSsid,
            password: cfgPassword,
            iface: cfgIface === "" ? null : cfgIface,
            channel: cfgChannel === 0 ? null : cfgChannel,
            band: cfgBand,
            country: null
        }
        pendingCfg = JSON.stringify(cfg)
        startProc.running = true
    }

    function stopHotspot() {
        if (!running || stopProc.running) return
        stopProc.running = true
    }

    // ---------------------------------------------------------- launcher API
    function getItems(query) {
        refresh()
        const q = (query || "").toLowerCase().trim()
        const items = []

        items.push({
            name: running ? "Turn hotspot off" : "Turn hotspot on",
            comment: running ? ("Sharing “" + ssid + "” · " + clientCount + " device(s)")
                             : ("Start “" + cfgSsid + "”"),
            icon: "material:" + (running ? "wifi_tethering_off" : "wifi_tethering"),
            action: "toggle",
            categories: ["LinSpot"]
        })

        if (running) {
            items.push({
                name: "Hotspot: " + ssid,
                comment: "Channel " + channel + " · " + clientCount + " connected",
                icon: "material:wifi_tethering",
                action: "noop",
                categories: ["LinSpot"]
            })
            items.push({
                name: "Show Wi-Fi QR",
                comment: "Scan with a phone to join “" + ssid + "”",
                icon: "material:qr_code_2",
                action: "qr",
                categories: ["LinSpot"]
            })
            for (const c of clientList) {
                items.push({
                    name: "Disconnect " + c.host,
                    comment: c.ip + " · " + c.mac,
                    icon: "material:block",
                    action: "kick:" + c.mac,
                    categories: ["LinSpot"]
                })
            }
        }

        items.push({
            name: "Open LinSpot app",
            comment: "Full settings, country code and logs",
            icon: "material:settings",
            action: "gui",
            categories: ["LinSpot"]
        })

        if (q === "") return items
        return items.filter(i => (i.name + " " + i.comment).toLowerCase().indexOf(q) !== -1)
    }

    function executeItem(item) {
        const a = item.action || ""
        if (a === "toggle") { running ? stopHotspot() : startHotspot() }
        else if (a === "gui") { guiProc.running = true }
        else if (a === "qr") { qrProc.running = true }
        else if (a.startsWith("kick:")) {
            kickMac = a.substring(5)
            kickProc.running = true
        }
    }

    function getContextMenuActions(item) { return [] }
}
