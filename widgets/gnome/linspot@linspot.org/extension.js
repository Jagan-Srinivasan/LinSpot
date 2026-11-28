// LinSpot Hotspot - GNOME Shell extension (GNOME 45+, ES modules).
// Adds a Quick Settings toggle with a menu listing connected devices.
// All work is done by the `linspot` binary: `status`, `on`, `off`, `gui`.
// "on"/"off" ask for admin rights through polkit (GNOME's polkit agent shows the prompt).
import GObject from 'gi://GObject';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {QuickMenuToggle, SystemIndicator} from 'resource:///org/gnome/shell/ui/quickSettings.js';

const BIN = '@LINSPOT_BIN@';
const LEASES = '/run/linspot/leases';
const ICON = 'network-wireless-hotspot-symbolic';

// run argv, resolve to {code, out, err}; never rejects
function run(argv) {
    return new Promise(resolve => {
        try {
            const proc = Gio.Subprocess.new(
                argv, Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
            proc.communicate_utf8_async(null, null, (p, res) => {
                try {
                    const [, out, err] = p.communicate_utf8_finish(res);
                    resolve({code: p.get_exit_status(), out: out ?? '', err: err ?? ''});
                } catch (e) {
                    resolve({code: -1, out: '', err: String(e)});
                }
            });
        } catch (e) {
            resolve({code: -1, out: '', err: String(e)});
        }
    });
}

const HotspotToggle = GObject.registerClass(
class HotspotToggle extends QuickMenuToggle {
    _init() {
        super._init({
            title: 'Hotspot',
            subtitle: 'Off',
            iconName: ICON,
            toggleMode: true,
        });
        this._busy = false;
        this._running = false;
        this._destroyed = false;
        this.onState = null;

        this.menu.setHeader(ICON, 'LinSpot Hotspot', 'Wi-Fi sharing');
        this._clients = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._clients);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this.menu.addAction('Open LinSpot', () => {
            run(['sh', '-c', `nohup '${BIN}' gui >/dev/null 2>&1 &`]);
        });
        this.menu.addAction('Show Wi-Fi QR (connect by camera)', () => {
            run(['sh', '-c',
                `'${BIN}' qr --png /tmp/linspot-qr.png && xdg-open /tmp/linspot-qr.png`]);
        });

        // toggleMode flips `checked` before 'clicked'; read it as the requested state
        this.connect('clicked', () => this._request(this.checked));
        this.refresh();
        this._timer = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 3, () => {
            this.refresh();
            return GLib.SOURCE_CONTINUE;
        });
    }

    get running() {
        return this._running;
    }

    async _request(want) {
        if (this._busy) {
            this.checked = this._running;
            return;
        }
        this._busy = true;
        this.subtitle = want ? 'Starting ...' : 'Stopping ...';
        const r = await run([BIN, want ? 'on' : 'off']);
        this._busy = false;
        if (this._destroyed)
            return;
        if (r.code !== 0 && r.code !== 126 && r.code !== 127) {
            const msg = (r.err + r.out).trim().split('\n').slice(-2).join(' ');
            Main.notifyError('LinSpot', msg || `failed (exit ${r.code})`);
        } else if (r.code === 126 || r.code === 127) {
            Main.notify('LinSpot', 'Authentication cancelled');
        }
        await this.refresh();
    }

    async refresh() {
        if (this._busy)
            return;
        const st = await run([BIN, 'status']);
        let s = {running: false};
        try {
            s = JSON.parse(st.out);
        } catch (_e) { /* binary missing or bad output: treat as off */ }
        let clients = [];
        if (s.running) {
            const l = await run(['cat', LEASES]);
            const now = Math.floor(Date.now() / 1000);
            for (const line of l.out.split('\n')) {
                const p = line.trim().split(/\s+/);
                if (p.length < 4)
                    continue;
                const exp = parseInt(p[0]) || 0;
                if (exp !== 0 && exp <= now)
                    continue;
                clients.push({ip: p[2], host: p[3] === '*' ? 'unknown device' : p[3]});
            }
        }
        if (this._destroyed || this._busy)
            return;
        this._running = !!s.running;
        this.checked = this._running;
        this.subtitle = this._running ? `${s.ssid} - ${clients.length} device(s)` : 'Off';
        this.menu.setHeader(ICON, 'LinSpot Hotspot',
            this._running ? `${s.ssid} (channel ${s.channel})` : 'Off');
        this._clients.removeAll();
        if (this._running && clients.length === 0) {
            const none = new PopupMenu.PopupMenuItem('No devices connected', {reactive: false});
            this._clients.addMenuItem(none);
        }
        for (const c of clients)
            this._clients.addMenuItem(new PopupMenu.PopupMenuItem(`${c.host}   ${c.ip}`, {reactive: false}));
        if (this.onState)
            this.onState();
    }

    destroy() {
        this._destroyed = true;
        if (this._timer) {
            GLib.source_remove(this._timer);
            this._timer = null;
        }
        super.destroy();
    }
});

const HotspotIndicator = GObject.registerClass(
class HotspotIndicator extends SystemIndicator {
    _init() {
        super._init();
        this._indicator = this._addIndicator();
        this._indicator.iconName = ICON;
        this._indicator.visible = false;
        this._toggle = new HotspotToggle();
        this._toggle.onState = () => {
            this._indicator.visible = this._toggle.running;
        };
        this.quickSettingsItems.push(this._toggle);
    }

    destroy() {
        this.quickSettingsItems.forEach(i => i.destroy());
        super.destroy();
    }
});

export default class LinspotExtension extends Extension {
    enable() {
        this._indicator = new HotspotIndicator();
        Main.panel.statusArea.quickSettings.addExternalIndicator(this._indicator);
    }

    disable() {
        this._indicator?.destroy();
        this._indicator = null;
    }
}
