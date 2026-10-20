// Ciel's door into GNOME Shell's notifications (src-tauri/src/gnome_ext.rs
// writes this file). Ciel sees the notifications on the bus but cannot speak
// for the notification server: only GNOME Shell can tell an app its
// notification was clicked, which is what takes Discord to the very chat or
// Chrome to the very tab. This finds GNOME's own notification by the id GNOME
// gave it and activates or dismisses it, exactly as a click in GNOME's list.
//
// Nothing else is exposed: two calls each way, by id, on the session bus.

import Gio from 'gi://Gio';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as MessageTray from 'resource:///org/gnome/shell/ui/messageTray.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const PATH = '/io/github/jeaneudesasg/Ciel/Shell';
const IFACE = `<node>
  <interface name="io.github.jeaneudesasg.Ciel.Shell">
    <method name="Activate"><arg type="u" name="id" direction="in"/><arg type="b" direction="out"/></method>
    <method name="Dismiss"><arg type="u" name="id" direction="in"/><arg type="b" direction="out"/></method>
    <method name="ActivateGtk"><arg type="s" name="app" direction="in"/><arg type="s" name="id" direction="in"/><arg type="b" direction="out"/></method>
    <method name="DismissGtk"><arg type="s" name="app" direction="in"/><arg type="s" name="id" direction="in"/><arg type="b" direction="out"/></method>
    <property name="Version" type="u" access="read"/>
  </interface>
</node>`;

/** A notification sent to org.freedesktop.Notifications, by the id its sender got back. */
const fdo = id => Main.notificationDaemon?._fdoNotificationDaemon?._notifications?.get(id) ?? null;
/** One sent to org.gtk.Notifications, by its app and its own id. */
const gtk = (app, id) => Main.notificationDaemon?._gtkNotificationDaemon?._sources?.[app]?._notifications?.[id] ?? null;

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
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, {
            Activate: id => activate(fdo(id)),
            Dismiss: id => dismiss(fdo(id)),
            ActivateGtk: (app, id) => activate(gtk(app, id)),
            DismissGtk: (app, id) => dismiss(gtk(app, id)),
            Version: 1,
        });
        this._dbus.export(Gio.DBus.session, PATH);
    }

    disable() {
        this._dbus?.unexport();
        this._dbus = null;
    }
}
