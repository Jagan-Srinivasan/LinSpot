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

    property string lsAuthMode: "sudo"          // "sudo" (inline prompt) | "pkexec"
    property bool lsShowCount: true
    property bool lsShowZero: true
    property int lsIconSize: 18
    property int lsFontSize: 12
    property int lsPadding: 8
    property int lsSpacing: 4
    property int lsPopW: 380
    property int lsPopH: 420
    property int lsMaxDevices: 4
    property bool lsOfferFp: true
    property bool lsHideWhenOff: true
    property bool lsAutoFp: true

    function num(key, def, lo, hi) {
        const n = Number(pluginService.loadPluginData(pid, key, def))
        if (isNaN(n)) return def
        return Math.max(lo, Math.min(hi, n))
    }
    function flag(key, def) {
        const v = pluginService.loadPluginData(pid, key, def)
        return v === true || v === "true"
    }

    function loadSettings() {
        if (!pluginService) return
        binOverride = String(pluginService.loadPluginData(pid, "linspotBin", "") || "")
        cfgOverride = String(pluginService.loadPluginData(pid, "configPath", "") || "")
        const n = parseInt(pluginService.loadPluginData(pid, "pollSeconds", "3"))
        pollSeconds = isNaN(n) || n < 1 ? 3 : n

        lsAuthMode = String(pluginService.loadPluginData(pid, "authMode", "sudo")) === "pkexec" ? "pkexec" : "sudo"
        lsShowCount = flag("showCount", true)
        lsShowZero = flag("showZero", true)
        lsIconSize = num("pillIconSize", 18, 12, 32)
        lsFontSize = num("pillFontSize", 12, 8, 20)
        lsPadding = num("pillPadding", 8, 0, 24)
        lsSpacing = num("pillSpacing", 4, 0, 16)
        lsPopW = num("popoutWidth", 380, 300, 600)
        lsPopH = num("popoutHeight", 420, 240, 800)
        lsMaxDevices = num("maxVisibleDevices", 4, 1, 10)
        lsOfferFp = flag("fingerprintOffer", true)
        lsHideWhenOff = flag("hideBarWhenOff", true)
        lsAutoFp = flag("fingerprintAuto", true)
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
                root.nopassProc.running = true
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
    property bool qrVisible: false
    property string qrFile: ""
    property string qrSource: ""
    // measured height of the shared management panel: the control-center detail page and the
    // bar popout both size their window from it, so the QR always fits instead of overflowing
    property real panelHeight: 0
    property real panelBaseHeight: 0
    readonly property real panelExtra: Math.max(0, panelHeight - panelBaseHeight)

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
                    if (Array.isArray(s.client_list)) {
                        // already filtered by the backend (kicked devices are hidden)
                        root.clients = s.client_list.map(c => ({ mac: c.mac, ip: c.ip, host: c.host }))
                        root.clientCount = root.clients.length
                        return
                    }
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

    // ------------------------------------------------------- authentication
    // Why this exists: pkexec opens a separate polkit window. That steals keyboard focus from the
    // control center / popout, DMS treats it as "clicked outside" and closes the panel, and the
    // polkit dialog then gets torn down with it. With the inline mode we ask for the sudo password
    // inside the panel itself, so nothing else is ever opened.
    //
    // The password lives only in `authPw` while a command runs. It is passed to the child via an
    // environment variable + stdin pipe (never on a command line / `ps`) and cleared afterwards.
    property bool needAuth: false
    property string authAction: ""        // "start" | "stop" | "kick"
    property string authError: ""
    property string authPw: ""
    property bool useSudo: false          // mode of the command currently running
    property bool useNopass: false        // current attempt runs through passwordless sudo
    property bool useFp: false            // current attempt is a fingerprint attempt
    property bool aborting: false
    property bool fpAvailable: false      // fprintd has enrolled fingers AND pam_fprintd is configured

    Process {
        id: fpDetectProc
        command: ["sh", "-c",
            "e=0; p=0; " +
            "if command -v fprintd-list >/dev/null 2>&1 && fprintd-list \"$USER\" 2>/dev/null | grep -q -- '- #'; then e=1; fi; " +
            "if grep -rqs pam_fprintd /etc/pam.d/ 2>/dev/null; then p=1; fi; echo \"$e$p\""]
        stdout: StdioCollector {
            onStreamFinished: root.fpAvailable = text.trim() === "11"
        }
    }

    readonly property string authActionLabel: authAction === "start" ? "turn the hotspot on"
        : authAction === "stop" ? "turn the hotspot off"
        : authAction === "kick" ? "disconnect the device" : "continue"

    function askPassword(action) {
        authAction = action
        authError = ""
        useFp = false
        needAuth = true
        if (fpAvailable && lsOfferFp && lsAutoFp) startFingerprint()
    }

    // Fingerprint = same sudo call with an empty password line. With pam_fprintd first in the PAM
    // stack sudo asks the sensor and never reads stdin; with pam_unix first the empty line just
    // fails the password stage and PAM falls through to the sensor. Either order works.
    function startFingerprint() {
        if (busy || !needAuth) return
        authPw = ""
        authError = ""
        useFp = true
        runAction(authAction, true, false)
        busyText = "Touch the fingerprint sensor…"
    }

    function abortScan() {
        if (startProc.running || stopProc.running || kickProc.running) {
            aborting = true
            startProc.running = false
            stopProc.running = false
            kickProc.running = false
        }
    }

    function consumeAbort() {
        if (!aborting) return false
        aborting = false
        busy = false
        busyText = ""
        authPw = ""
        useFp = false
        useNopass = false
        return true
    }

    function cancelAuth() {
        needAuth = false
        authAction = ""
        authError = ""
        authPw = ""
        useFp = false
        useNopass = false
    }

    function submitPassword(pw) {
        if (pw === "" || busy) return
        authPw = pw
        authError = ""
        useFp = false
        runAction(authAction, true, false)
    }

    // sudo wrong-password detection
    function isAuthFailure(errText) {
        const t = (errText || "").toLowerCase()
        return t.indexOf("incorrect password") >= 0 || t.indexOf("try again") >= 0
            || t.indexOf("no password was provided") >= 0 || t.indexOf("a password is required") >= 0 || t.indexOf("authentication failure") >= 0
    }
    function isNotAllowed(errText) {
        const t = (errText || "").toLowerCase()
        return t.indexOf("not in the sudoers") >= 0 || t.indexOf("may not run sudo") >= 0
    }

    // returns true when the failure was an auth problem and has been handled
    function handleSudoFailure(errText) {
        if (isNotAllowed(errText)) {
            cancelAuth()
            lastError = "Your user is not allowed to use sudo. Switch to the Polkit dialog in plugin settings."
            ToastService.showError("Hotspot", lastError)
            return true
        }
        if (isAuthFailure(errText)) {
            authPw = ""
            authError = useNopass ? "Password required"
                : useFp ? "Fingerprint not recognised, enter your password" : "Wrong password, try again"
            useFp = false
            useNopass = false
            needAuth = true
            return true
        }
        return false
    }

    function finishOk() {
        cancelAuth()
        lastError = ""
    }

    // The sudo command prints this marker the moment authentication has succeeded (it only runs as
    // root after that). We close the prompt right then instead of waiting for the hotspot itself.
    readonly property string authMarker: "LINSPOT_AUTH_OK"
    property string startOutText: ""

    function authVerified() {
        const a = authAction
        cancelAuth()
        busyText = a === "start" ? "Starting hotspot…" : a === "stop" ? "Stopping hotspot…" : ""
    }

    function runAction(action, viaSudo, nopass) {
        useSudo = viaSudo
        useNopass = nopass === true
        startOutText = ""
        busy = true
        if (action === "start") {
            busyText = "Starting hotspot…"
            startProc.running = true
        } else if (action === "stop") {
            busyText = "Stopping hotspot…"
            stopProc.running = true
        } else if (action === "kick") {
            kickProc.running = true
        }
    }

    // Single entry point. `fromPanel` = triggered from the control center / popout where a polkit
    // window would close the panel; the bar right-click has no panel open so pkexec is fine there.
    // For stop/kick a passwordless sudoers rule is tried first, so normally no password is asked.
    function request(action, fromPanel) {
        if (busy) return
        if (lsAuthMode === "sudo" && fromPanel && (action === "stop" || action === "kick") && root.nopass) {
            needAuth = false
            runAction(action, false, true)
        } else if (lsAuthMode === "sudo" && fromPanel) {
            if (needAuth && authAction === action) return
            askPassword(action)
        } else {
            runAction(action, false)
        }
    }

    property bool nopass: false
    Process {
        id: nopassProc
        // sudo -l -n exits 0 without prompting iff 'stop'/'kick-root' may run passwordless
        command: ["sh", "-c",
            "if sudo -n -l \"$1\" stop >/dev/null 2>&1 && sudo -n -l \"$1\" kick-root x >/dev/null 2>&1; " +
            "then echo 1; else echo 0; fi", "sh", root.linspotBin]
        stdout: StdioCollector {
            onStreamFinished: root.nopass = text.trim() === "1"
        }
    }

    // ------------------------------------------------------------ actions
    Process {
        id: startProc
        environment: ({ "LINSPOT_CFG": root.pendingCfg, "LS_PW": root.authPw })
        command: root.useSudo
            ? ["sh", "-c", "printf '%s\\n%s' \"$LS_PW\" \"$LINSPOT_CFG\" | sudo -S -p '' sh -c 'echo LINSPOT_AUTH_OK; exec \"$@\"' sh \"$1\" start --stdin", "sh", root.linspotBin]
            : ["sh", "-c", "printf '%s' \"$LINSPOT_CFG\" | pkexec \"$1\" start --stdin", "sh", root.linspotBin]
        stderr: StdioCollector { id: startErr }
        stdout: SplitParser {
            onRead: (line) => {
                if (line === root.authMarker) root.authVerified()
                else root.startOutText += line + "\n"
            }
        }
        onExited: (code) => {
            if (root.consumeAbort()) return
            root.busy = false
            root.busyText = ""
            const errText = startErr.text
            if (code !== 0 && (root.useSudo || root.useNopass) && root.handleSudoFailure(errText)) {
                root.refresh()
                return
            }
            root.pendingCfg = ""
            root.authPw = ""
            if (code === 0) {
                root.finishOk()
                ToastService.showInfo("Hotspot on", root.cfgSsid)
            } else if (!root.useSudo && (code === 126 || code === 127)) {
                root.lastError = "Authentication cancelled"
                ToastService.showWarning("Hotspot", "Authentication cancelled")
            } else {
                root.cancelAuth()
                const msg = (errText + root.startOutText).trim().split("\n").slice(-3).join(" ")
                root.lastError = msg || ("start failed (exit " + code + ")")
                ToastService.showError("Hotspot failed", root.lastError)
            }
            root.refresh()
        }
    }

    Process {
        id: stopProc
        environment: ({ "LS_PW": root.authPw })
        command: root.useNopass
            ? ["sudo", "-n", root.linspotBin, "stop"]
            : root.useSudo
            ? ["sh", "-c", "printf '%s\\n' \"$LS_PW\" | sudo -S -p '' sh -c 'echo LINSPOT_AUTH_OK; exec \"$@\"' sh \"$1\" stop", "sh", root.linspotBin]
            : ["pkexec", root.linspotBin, "stop"]
        stdout: SplitParser {
            onRead: (line) => { if (line === root.authMarker) root.authVerified() }
        }
        stderr: StdioCollector { id: stopErr }
        onExited: (code) => {
            if (root.consumeAbort()) return
            root.busy = false
            root.busyText = ""
            const errText = stopErr.text
            if (code !== 0 && (root.useSudo || root.useNopass) && root.handleSudoFailure(errText)) {
                root.refresh()
                return
            }
            root.authPw = ""
            if (code === 0) {
                root.finishOk()
                ToastService.showInfo("Hotspot off")
            } else if (!root.useSudo && (code === 126 || code === 127)) {
                ToastService.showWarning("Hotspot", "Authentication cancelled")
            } else {
                root.cancelAuth()
                root.lastError = errText.trim().split("\n").slice(-2).join(" ")
                ToastService.showError("Stop failed", root.lastError)
            }
            root.refresh()
        }
    }

    property string kickMac: ""
    Process {
        id: kickProc
        environment: ({ "LS_PW": root.authPw })
        // through the linspot binary (not hostapd_cli directly) so the DHCP lease is dropped
        // as well and the device leaves the connected list immediately
        command: root.useNopass
            ? ["sudo", "-n", root.linspotBin, "kick-root", root.kickMac]
            : root.useSudo
            ? ["sh", "-c", "printf '%s\\n' \"$LS_PW\" | sudo -S -p '' sh -c 'echo LINSPOT_AUTH_OK; exec \"$@\"' sh \"$1\" kick-root \"$2\"", "sh", root.linspotBin, root.kickMac]
            : ["pkexec", root.linspotBin, "kick-root", root.kickMac]
        stdout: SplitParser {
            onRead: (line) => { if (line === root.authMarker) root.authVerified() }
        }
        stderr: StdioCollector { id: kickErr }
        onExited: (code) => {
            if (root.consumeAbort()) return
            root.busy = false
            if (code !== 0 && (root.useSudo || root.useNopass) && root.handleSudoFailure(kickErr.text)) {
                root.refresh()
                return
            }
            root.authPw = ""
            if (code === 0) root.finishOk()
            else if (root.useSudo) root.cancelAuth()
            root.refresh()
        }
    }

    Process {
        id: guiProc
        command: [root.linspotBin, "gui"]
    }

    function startHotspot(fromPanel) {
        if (busy || running || linspotBin === "") return
        if (!cfgLoaded || cfgPassword.length < 8) {
            ToastService.showWarning("LinSpot", "Set an SSID and a password (8+ chars) in the LinSpot app first")
            openGui()
            return
        }
        pendingCfg = JSON.stringify({
            ssid: cfgSsid,
            password: cfgPassword,
            iface: cfgIface === "" ? null : cfgIface,
            channel: cfgChannel === 0 ? null : cfgChannel,
            band: cfgBand,
            country: null
        })
        request("start", fromPanel)
    }

    function stopHotspot(fromPanel) {
        if (busy || !running || linspotBin === "") return
        request("stop", fromPanel)
    }

    function toggleHotspot(fromPanel) {
        if (running) stopHotspot(fromPanel)
        else startHotspot(fromPanel)
    }

    function kickClient(mac) {
        kickMac = mac
        request("kick", true)
    }

    function openGui() {
        if (linspotBin !== "" && !guiProc.running) guiProc.running = true
    }

    // QR sharing: the linspot binary writes the connect-QR PNG (no root needed)
    function showQr() {
        if (linspotBin === "" || !root.running) return
        qrFile = (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/linspot-qr-" + Date.now() + ".png"
        qrSource = ""
        qrProc.command = [linspotBin, "qr", "--png", qrFile]
        qrProc.running = true
    }

    Process {
        id: qrProc
        stdout: StdioCollector {
            onStreamFinished: {
                if (root.qrFile !== "") root.qrSource = "file://" + root.qrFile
            }
        }
    }

    Component.onCompleted: {
        loadSettings()
        fpDetectProc.running = true
        cfgReader.running = true
        detectProc.running = true
    }

    // --------------------------------------------------------- bar pill
    // left click -> manage popout, right click -> toggle the hotspot (pkexec: no panel is open here)
    pillRightClickAction: () => root.toggleHotspot(false)

    // bar pill only exists while the hotspot is on (the control center tile is always there)
    readonly property bool lsPillShown: running || !lsHideWhenOff

    // Hide the whole bar slot (the bar's own padding/background too), not just our pill inside it.
    // The control center tile is a separate item and stays available regardless.
    visible: lsPillShown

    readonly property bool lsCountVisible: running && lsShowCount && (clientCount > 0 || lsShowZero)

    horizontalBarPill: Component {
        StyledRect {
            // both implicit and real size follow the content so the bar always reserves enough room
            visible: root.lsPillShown
            implicitWidth: root.lsPillShown ? pillRow.implicitWidth + root.lsPadding * 2 : 0
            implicitHeight: root.lsPillShown ? Math.max(parent && parent.widgetThickness ? parent.widgetThickness : 0, pillRow.implicitHeight + 8) : 0
            width: implicitWidth
            height: implicitHeight
            radius: Theme.cornerRadius
            color: Theme.chipSurface

            Row {
                id: pillRow
                anchors.centerIn: parent
                spacing: root.lsSpacing
                DankIcon {
                    name: root.busy ? "sync" : (root.running ? "wifi_tethering" : "wifi_tethering_off")
                    size: root.lsIconSize
                    color: root.running ? Theme.primary : Theme.surfaceText
                    anchors.verticalCenter: parent.verticalCenter
                }
                StyledText {
                    visible: root.lsCountVisible
                    text: root.clientCount.toString()
                    font.pixelSize: root.lsFontSize
                    color: Theme.surfaceText
                    anchors.verticalCenter: parent.verticalCenter
                }
            }
        }
    }

    verticalBarPill: Component {
        StyledRect {
            visible: root.lsPillShown
            implicitWidth: root.lsPillShown ? Math.max(parent && parent.widgetThickness ? parent.widgetThickness : 0, pillCol.implicitWidth + 8) : 0
            implicitHeight: root.lsPillShown ? pillCol.implicitHeight + root.lsPadding * 2 : 0
            width: implicitWidth
            height: implicitHeight
            radius: Theme.cornerRadius
            color: Theme.chipSurface

            Column {
                id: pillCol
                anchors.centerIn: parent
                spacing: root.lsSpacing
                DankIcon {
                    name: root.busy ? "sync" : (root.running ? "wifi_tethering" : "wifi_tethering_off")
                    size: root.lsIconSize
                    color: root.running ? Theme.primary : Theme.surfaceText
                    anchors.horizontalCenter: parent.horizontalCenter
                }
                StyledText {
                    visible: root.lsCountVisible
                    text: root.clientCount.toString()
                    font.pixelSize: root.lsFontSize
                    color: Theme.surfaceText
                    anchors.horizontalCenter: parent.horizontalCenter
                }
            }
        }
    }

    // --------------------------------------------------- control center tile
    ccWidgetIcon: root.running ? "wifi_tethering" : "wifi_tethering_off"
    ccWidgetPrimaryText: "Hotspot"
    ccWidgetSecondaryText: root.needAuth ? "Password needed — open details"
        : root.busy ? root.busyText
        : (root.running ? (root.ssid + " · " + root.clientCount + (root.clientCount === 1 ? " device" : " devices"))
                        : "Off")
    ccWidgetIsActive: root.running
    onCcWidgetToggled: root.toggleHotspot(true)
    ccDetailContent: managePanel
    ccDetailHeight: root.panelHeight > 0 ? root.panelHeight : 250

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
                    visible: root.lastError !== "" && !root.needAuth
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

                // device list: scrolls instead of growing past the panel
                Item {
                    visible: root.clients.length > 0
                    width: col.width
                    height: Math.min(root.clients.length, root.lsMaxDevices) * 52
                    clip: true

                    Flickable {
                        anchors.fill: parent
                        contentWidth: width
                        contentHeight: devCol.implicitHeight
                        boundsBehavior: Flickable.StopAtBounds
                        interactive: contentHeight > height

                        Column {
                            id: devCol
                            width: parent.width
                            spacing: 4
                            Repeater {
                                model: root.clients
                                delegate: Item {
                                    width: devCol.width
                                    height: 48
                                    DankIcon {
                                        id: devIcon
                                        name: "smartphone"
                                        size: 22
                                        color: Theme.primary
                                        anchors.left: parent.left
                                        anchors.verticalCenter: parent.verticalCenter
                                    }
                                    LsButton {
                                        id: kickBtn
                                        iconName: "block"
                                        active: !root.busy
                                        anchors.right: parent.right
                                        anchors.verticalCenter: parent.verticalCenter
                                        onClicked: root.kickClient(modelData.mac)
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
                                }
                            }
                        }
                    }
                }

                // inline password prompt (replaces the polkit window)
                Rectangle {
                    id: authBox
                    visible: root.needAuth
                    width: col.width
                    height: authCol.implicitHeight + Theme.spacingM * 2
                    radius: Theme.cornerRadius
                    color: Theme.surfaceContainerHighest
                    border.width: 1
                    border.color: Theme.outlineVariant

                    onVisibleChanged: if (visible) { pwField.clear(); focusTimer.restart() }

                    Timer {
                        id: focusTimer
                        interval: 80
                        onTriggered: pwField.focusField()
                    }

                    Column {
                        id: authCol
                        x: Theme.spacingM
                        y: Theme.spacingM
                        width: authBox.width - Theme.spacingM * 2
                        spacing: Theme.spacingS

                        StyledText {
                            width: parent.width
                            wrapMode: Text.WordWrap
                            text: "Authenticate to " + root.authActionLabel
                            font.pixelSize: Theme.fontSizeSmall
                            color: Theme.surfaceText
                        }
                        Row {
                            visible: root.busy && root.useFp
                            spacing: Theme.spacingS
                            DankIcon {
                                id: fpIcon
                                name: "fingerprint"
                                size: 28
                                color: Theme.primary
                                anchors.verticalCenter: parent.verticalCenter
                                SequentialAnimation on opacity {
                                    running: root.busy && root.useFp
                                    loops: Animation.Infinite
                                    NumberAnimation { to: 0.35; duration: 700 }
                                    NumberAnimation { to: 1.0; duration: 700 }
                                }
                            }
                            StyledText {
                                text: "Touch the fingerprint sensor…"
                                font.pixelSize: Theme.fontSizeSmall
                                color: Theme.surfaceText
                                anchors.verticalCenter: parent.verticalCenter
                            }
                        }
                        Rectangle {
                            id: pwField
                            width: parent.width
                            height: 38
                            opacity: root.busy ? 0.5 : 1.0
                            radius: Theme.cornerRadius
                            color: Theme.surfaceContainerHigh
                            border.width: pwInput.activeFocus ? 2 : 1
                            border.color: pwInput.activeFocus ? Theme.primary : Theme.outlineVariant

                            property alias text: pwInput.text
                            function clear() { pwInput.text = "" }
                            function focusField() { pwInput.forceActiveFocus() }

                            TextInput {
                                id: pwInput
                                anchors.fill: parent
                                anchors.leftMargin: Theme.spacingM
                                anchors.rightMargin: Theme.spacingM
                                verticalAlignment: TextInput.AlignVCenter
                                enabled: !root.busy
                                echoMode: TextInput.Password
                                passwordCharacter: "•"
                                clip: true
                                selectByMouse: true
                                color: Theme.surfaceText
                                font.pixelSize: Theme.fontSizeMedium
                                onAccepted: root.submitPassword(pwInput.text)
                            }
                            StyledText {
                                anchors.left: parent.left
                                anchors.leftMargin: Theme.spacingM
                                anchors.verticalCenter: parent.verticalCenter
                                visible: pwInput.text.length === 0
                                text: "Password"
                                font.pixelSize: Theme.fontSizeMedium
                                color: Theme.surfaceVariantText
                            }
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.IBeamCursor
                                onPressed: (m) => { pwInput.forceActiveFocus(); m.accepted = false }
                            }
                        }
                        StyledText {
                            visible: root.authError !== ""
                            width: parent.width
                            wrapMode: Text.WordWrap
                            text: root.authError
                            font.pixelSize: Theme.fontSizeSmall
                            color: Theme.error
                        }
                        Flow {
                            width: parent.width
                            spacing: Theme.spacingS
                            LsButton {
                                text: root.busy && !root.useFp ? "Checking…" : "Confirm"
                                iconName: "lock_open"
                                primary: true
                                active: !root.busy && pwField.text.length > 0
                                onClicked: { root.submitPassword(pwField.text); pwField.clear() }
                            }
                            LsButton {
                                visible: root.fpAvailable && root.lsOfferFp && !root.busy
                                text: "Fingerprint"
                                iconName: "fingerprint"
                                onClicked: root.startFingerprint()
                            }
                            LsButton {
                                text: root.busy ? "Stop scan" : "Cancel"
                                onClicked: root.busy ? root.abortScan() : root.cancelAuth()
                            }
                        }
                    }
                }

                Flow {
                    width: col.width
                    spacing: Theme.spacingS
                    visible: !root.needAuth
                    LsButton {
                        text: root.busy ? root.busyText : (root.running ? "Turn off" : "Turn on")
                        iconName: root.running ? "wifi_tethering_off" : "wifi_tethering"
                        primary: !root.running
                        active: !root.busy
                        onClicked: root.toggleHotspot(true)
                    }
                    LsButton {
                        text: "Open LinSpot"
                        iconName: "settings"
                        onClicked: root.openGui()
                    }
                    LsButton {
                        text: root.qrVisible ? "Hide QR" : "Show QR"
                        iconName: "qr_code_2"
                        active: root.running && !root.busy
                        onClicked: {
                            root.qrVisible = !root.qrVisible
                            if (root.qrVisible) root.showQr()
                            else root.qrSource = ""
                        }
                    }
                }

                Rectangle {
                    visible: root.qrVisible
                    width: Math.min(col.width, 200)
                    height: width
                    radius: Theme.cornerRadius
                    color: "white"
                    Image {
                        anchors.fill: parent
                        anchors.margins: 12
                        fillMode: Image.PreserveAspectFit
                        source: root.qrSource
                        cache: false
                    }
                    StyledText {
                        anchors.centerIn: parent
                        visible: root.qrVisible && root.qrSource === ""
                        text: "Building the QR code ..."
                        color: "black"
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ bar popout
    popoutWidth: root.lsPopW
    popoutHeight: root.lsPopH + root.panelExtra
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
