import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

function readUsage() {
    try {
        const bin = `${GLib.get_home_dir()}/.local/bin/claude-usage`;
        const [ok, out, , status] = GLib.spawn_command_line_sync(`${bin} --panel`);
        if (ok && status === 0) {
            return new TextDecoder().decode(out).trim();
        }
        return 'err';
    } catch (_e) {
        return 'err';
    }
}

const ClaudeUsageIndicator = GObject.registerClass(
    class ClaudeUsageIndicator extends PanelMenu.Button {
        _init(extensionPath) {
            super._init(0.0, 'Claude Usage');

            const box = new St.BoxLayout({ y_align: Clutter.ActorAlign.CENTER });

            const icon = new St.Icon({
                gicon: Gio.icon_new_for_string(`${extensionPath}/icons/claude-symbolic.svg`),
                style_class: 'system-status-icon',
            });

            this._label = new St.Label({
                text: '...',
                y_align: Clutter.ActorAlign.CENTER,
                style: 'font-family: monospace; margin-left: 4px;',
            });

            box.add_child(icon);
            box.add_child(this._label);
            this.add_child(box);

            this._update();
            this._timeout = GLib.timeout_add_seconds(
                GLib.PRIORITY_DEFAULT,
                60,
                () => { this._update(); return GLib.SOURCE_CONTINUE; }
            );
        }

        _update() {
            this._label.set_text(readUsage());
        }

        destroy() {
            if (this._timeout) {
                GLib.source_remove(this._timeout);
                this._timeout = null;
            }
            super.destroy();
        }
    });

export default class ClaudeUsageExtension extends Extension {
    enable() {
        this._indicator = new ClaudeUsageIndicator(this.path);
        Main.panel.addToStatusArea(this.uuid, this._indicator);
    }

    disable() {
        this._indicator?.destroy();
        this._indicator = null;
    }
}
