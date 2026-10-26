// Ciel's door into GNOME Shell's notifications (src-tauri/src/gnome_ext.rs
// writes this file). Ciel sees the notifications on the bus but cannot speak
// for the notification server: only GNOME Shell can tell an app its
// notification was clicked, which is what takes Discord to the very chat or
// Chrome to the very tab. This finds GNOME's own notification by the id GNOME
// gave it and activates or dismisses it, exactly as a click in GNOME's list.
//
// It also hides GNOME's top bar when Ciel's settings say so (Settings →
// General), so the island has the top of the screen: the bar's room goes to
// the windows, and it still shows in the Activities overview. The choice is
// read from Ciel's settings.json when GNOME starts the extension, so it holds
// before Ciel runs; Ciel tells it at once when the switch moves. Locked, or
// with the extension off, the bar is back.
//
// And it gives the lock screen Ciel's picture (Settings → Wallpaper → Lock
// screen). GNOME draws its lock screen from the desktop's background, blurred;
// while Ciel's option is on, it is drawn from org.gnome.desktop.screensaver
// instead, the picture Ciel sets there (and saved the user's before), unblurred.
// Option off, GNOME's own. That is why it also runs while the screen is locked.
//
// Nothing else is exposed: these calls, on the session bus.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as MessageTray from 'resource:///org/gnome/shell/ui/messageTray.js';
import * as Background from 'resource:///org/gnome/shell/ui/background.js';
import * as UnlockDialog from 'resource:///org/gnome/shell/ui/unlockDialog.js';
import St from 'gi://St';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const PATH = '/io/github/jeaneudesasg/Ciel/Shell';
const IFACE = `<node>
  <interface name="io.github.jeaneudesasg.Ciel.Shell">
    <method name="Activate"><arg type="u" name="id" direction="in"/><arg type="b" direction="out"/></method>
    <method name="Dismiss"><arg type="u" name="id" direction="in"/><arg type="b" direction="out"/></method>
    <method name="ActivateGtk"><arg type="s" name="app" direction="in"/><arg type="s" name="id" direction="in"/><arg type="b" direction="out"/></method>
    <method name="DismissGtk"><arg type="s" name="app" direction="in"/><arg type="s" name="id" direction="in"/><arg type="b" direction="out"/></method>
    <method name="SetPanelHidden"><arg type="b" name="hidden" direction="in"/><arg type="b" direction="out"/></method>
    <property name="Version" type="u" access="read"/>
  </interface>
</node>`;

/** A notification sent to org.freedesktop.Notifications, by the id its sender got back. */
const fdo = id => Main.notificationDaemon?._fdoNotificationDaemon?._notifications?.get(id) ?? null;
/** One sent to org.gtk.Notifications, by its app and its own id. */
const gtk = (app, id) => Main.notificationDaemon?._gtkNotificationDaemon?._sources?.[app]?._notifications?.[id] ?? null;

/** Ciel's settings, as saved: settings.json under the user's config folder. */
function cielSettings() {
    try {
        const path = GLib.build_filenamev([GLib.get_user_config_dir(), 'ciel', 'settings.json']);
        const [, bytes] = GLib.file_get_contents(path);
        return JSON.parse(new TextDecoder().decode(bytes));
    } catch {
        return {};
    }
}

const panelWanted = () => cielSettings().hideGnomePanel === true;
const lockWanted = () => cielSettings().wallpaper?.lockScreen === true;

/**
 * The lock screen's background, as unlockDialog.js makes it, from Ciel's
 * picture when the option is on: org.gnome.desktop.screensaver, no blur.
 */
function cielLockBackground(original) {
    return function (monitorIndex) {
        if (!lockWanted())
            return original.call(this, monitorIndex);
        const monitor = Main.layoutManager.monitors[monitorIndex];
        const widget = new St.Widget({
            style_class: 'screen-shield-background',
            x: monitor.x,
            y: monitor.y,
            width: monitor.width,
            height: monitor.height,
        });
        const bgManager = new Background.BackgroundManager({
            container: widget,
            monitorIndex,
            controlPosition: false,
            settingsSchema: 'org.gnome.desktop.screensaver',
        });
        this._bgManagers.push(bgManager);
        this._backgroundGroup.add_child(widget);
    };
}

/** The top bar out of the way: no room kept for it, shown only in the overview. */
class PanelHider {
    constructor() {
        this._hidden = false;
        this._overview = [];
    }

    set(hidden) {
        if (hidden === this._hidden)
            return;
        this._hidden = hidden;
        const layout = Main.layoutManager;
        const box = layout.panelBox;
        // As layout.js adds it, but without the room kept for it when hidden.
        layout.removeChrome(box);
        layout.addChrome(box, {affectsStruts: !hidden, trackFullscreen: true});
        if (hidden) {
            this._overview = [
                Main.overview.connect('showing', () => box.show()),
                Main.overview.connect('hidden', () => box.hide()),
            ];
            if (!Main.overview.visible)
                box.hide();
        } else {
            this._overview.forEach(id => Main.overview.disconnect(id));
            this._overview = [];
            box.show();
        }
    }
}

function activate(notification) {
    if (!notification)
        return false;
    notification.activate();
    return true;
}

function dismiss(notification) {
    if (!notification)
        return false;
    notification.destroy(MessageTray.NotificationDestroyedReason.DISMISSED);
    return true;
}

export default class CielExtension extends Extension {
    enable() {
        this._panel = new PanelHider();
        this._panel.set(panelWanted() && !Main.sessionMode.isLocked);
        // Locked, the lock screen keeps its bar; unlocked, Ciel's choice again.
        this._mode = Main.sessionMode.connect('updated', () =>
            this._panel?.set(panelWanted() && !Main.sessionMode.isLocked));
        const proto = UnlockDialog.UnlockDialog.prototype;
        this._createBackground = proto._createBackground;
        proto._createBackground = cielLockBackground(this._createBackground);
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, {
            Activate: id => activate(fdo(id)),
            Dismiss: id => dismiss(fdo(id)),
            ActivateGtk: (app, id) => activate(gtk(app, id)),
            DismissGtk: (app, id) => dismiss(gtk(app, id)),
            SetPanelHidden: hidden => {
                this._panel.set(hidden && !Main.sessionMode.isLocked);
                return true;
            },
            Version: 3,
        });
        this._dbus.export(Gio.DBus.session, PATH);
    }

    disable() {
        if (this._createBackground) {
            UnlockDialog.UnlockDialog.prototype._createBackground = this._createBackground;
            this._createBackground = null;
        }
        if (this._mode) {
            Main.sessionMode.disconnect(this._mode);
            this._mode = null;
        }
        this._dbus?.unexport();
        this._dbus = null;
        this._panel?.set(false);
        this._panel = null;
    }
}
