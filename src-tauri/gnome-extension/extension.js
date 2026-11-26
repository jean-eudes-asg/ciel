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
// And it opens GNOME's own screenshot tool, for the island's Screenshot
// button: nothing else can, on Wayland.
//
// And it tells Ciel's dock (src-tauri/src/dock/linux.rs) the apps open and
// their windows as the Shell sees them, on X11 and Wayland alike, and does
// what a click on the dock asks: an app started or brought forward, a window
// minimized or closed, the app grid, the overview, an app's own menu. It
// listens to the Shell only between DockWatch(true) and DockWatch(false):
// nothing runs while the dock is off.
//
// Nothing else is exposed: these calls, on the session bus.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as MessageTray from 'resource:///org/gnome/shell/ui/messageTray.js';
import * as Background from 'resource:///org/gnome/shell/ui/background.js';
import * as UnlockDialog from 'resource:///org/gnome/shell/ui/unlockDialog.js';
import * as BoxPointer from 'resource:///org/gnome/shell/ui/boxpointer.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import * as Volume from 'resource:///org/gnome/shell/ui/status/volume.js';
import {AppMenu} from 'resource:///org/gnome/shell/ui/appMenu.js';
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
    <method name="Screenshot"><arg type="b" direction="out"/></method>
    <method name="DockWatch"><arg type="b" name="on" direction="in"/><arg type="b" direction="out"/></method>
    <method name="DockApps"><arg type="s" direction="out"/></method>
    <method name="DockWindow"><arg type="t" name="id" direction="in"/><arg type="s" name="action" direction="in"/><arg type="b" direction="out"/></method>
    <method name="DockLaunch"><arg type="s" name="app" direction="in"/><arg type="b" name="newWindow" direction="in"/><arg type="b" direction="out"/></method>
    <method name="DockShell"><arg type="s" name="what" direction="in"/><arg type="b" direction="out"/></method>
    <method name="DockMenu"><arg type="s" name="app" direction="in"/><arg type="b" direction="out"/></method>
    <signal name="DockChanged"><arg type="s" name="what"/></signal>
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

/** A running app, or one GNOME knows by its .desktop id. */
function lookupApp(id) {
    const apps = Shell.AppSystem.get_default();
    return apps.lookup_app(id) ?? apps.get_running().find(a => a.get_id() === id) ?? null;
}

const windowById = id => global.display.list_all_windows().find(w => w.get_id() === id) ?? null;

/** Windows a dock shows: not Ciel's island and the like, which skip the taskbar. */
const shownWindows = app => app.get_windows().filter(w => !w.skip_taskbar);

/** Ciel's dock: what is open, and what its clicks ask. */
class Dock {
    constructor(emit) {
        this._emit = emit;
        this._signals = [];
        this._sink = null;
        this._menu = null;
        /** The windows "show desktop" minimized, for the next click to bring back. */
        this._minimized = [];
    }

    ready() {
        this._emit('ready');
    }

    watch(on) {
        this._signals.forEach(([obj, id]) => obj.disconnect(id));
        this._signals = [];
        this._watchSink(false);
        if (!on) {
            this._closeMenu();
            return;
        }
        const apps = () => this._emit('apps');
        const listen = (obj, name, fn) => this._signals.push([obj, obj.connect(name, fn)]);
        listen(Shell.AppSystem.get_default(), 'app-state-changed', apps);
        listen(Shell.WindowTracker.get_default(), 'tracked-windows-changed', apps);
        listen(global.display, 'notify::focus-window', apps);
        listen(global.display, 'window-demands-attention', apps);
        listen(global.display, 'window-marked-urgent', apps);
        listen(global.display, 'in-fullscreen-changed', () => this._emit('fullscreen'));
        listen(Main.layoutManager, 'monitors-changed', () => this._emit('monitors'));
        listen(global.settings, 'changed::favorite-apps', apps);
        listen(Volume.getMixerControl(), 'default-sink-changed', () => {
            this._watchSink(true);
            this._emit('system');
        });
        this._watchSink(true);
    }

    /** The speakers' volume and mute, as the Shell's own slider hears them. */
    _watchSink(on) {
        if (this._sink) {
            const [sink, ids] = this._sink;
            ids.forEach(id => sink.disconnect(id));
            this._sink = null;
        }
        const sink = on ? Volume.getMixerControl().get_default_sink() : null;
        if (!sink)
            return;
        const system = () => this._emit('system');
        this._sink = [sink, [sink.connect('notify::volume', system), sink.connect('notify::is-muted', system)]];
    }

    apps() {
        const focus = global.display.focus_window;
        const apps = Shell.AppSystem.get_default().get_running().map(app => {
            const windows = shownWindows(app);
            return {
                id: app.get_id(),
                name: app.get_name(),
                // Most recently used first.
                windows: windows.map(w => w.get_id()),
                attention: windows.some(w => w.demands_attention || w.urgent),
            };
        }).filter(a => a.windows.length > 0);
        return JSON.stringify({
            apps,
            // The dock's pins.
            favorites: global.settings.get_strv('favorite-apps'),
            focus: focus ? focus.get_id() : 0,
            // The screens a full-screen app is in front on, by their corner.
            fullscreen: Main.layoutManager.monitors
                .filter(m => global.display.get_monitor_in_fullscreen(m.index))
                .map(m => [m.x, m.y]),
        });
    }

    window(id, action) {
        if (Main.sessionMode.isLocked)
            return false;
        const win = windowById(id);
        if (!win)
            return false;
        if (action === 'front')
            Main.activateWindow(win);
        else if (action === 'minimize')
            win.minimize();
        else if (action === 'close')
            win.delete(global.get_current_time());
        else
            return false;
        return true;
    }

    launch(id, newWindow) {
        if (Main.sessionMode.isLocked)
            return false;
        const app = lookupApp(id);
        if (!app)
            return false;
        if (newWindow && app.can_open_new_window())
            app.open_new_window(-1);
        else
            app.activate();
        return true;
    }

    shell(what) {
        if (Main.sessionMode.isLocked)
            return false;
        switch (what) {
        case 'apps':
            if (Main.overview.visible)
                Main.overview.hide();
            else
                Main.overview.showApps();
            return true;
        case 'overview':
            Main.overview.toggle();
            return true;
        case 'desktop':
            this._showDesktop();
            return true;
        }
        return false;
    }

    /** Every window of this workspace minimized; the next click brings them back. */
    _showDesktop() {
        const workspace = global.workspace_manager.get_active_workspace();
        const shown = workspace.list_windows().filter(w =>
            !w.minimized && !w.skip_taskbar && w.get_window_type() === Meta.WindowType.NORMAL);
        if (shown.length > 0) {
            this._minimized = shown.map(w => w.get_id());
            shown.forEach(w => w.minimize());
            return;
        }
        this._minimized.map(windowById).filter(w => w?.minimized).forEach(w => w.unminimize());
        this._minimized = [];
    }

    /** The app's own menu, as GNOME's dash has it, where the pointer is. */
    menu(id) {
        if (Main.sessionMode.isLocked)
            return false;
        const app = lookupApp(id);
        if (!app)
            return false;
        this._closeMenu();
        const [x, y] = global.get_pointer();
        const anchor = new St.Widget({x, y, width: 1, height: 1});
        Main.uiGroup.add_child(anchor);
        const menu = new AppMenu(anchor, St.Side.BOTTOM, {favoritesSection: true, showSingleWindows: true});
        menu.setApp(app);
        Main.uiGroup.add_child(menu.actor);
        const manager = new PopupMenu.PopupMenuManager(anchor);
        manager.addMenu(menu);
        this._menu = {menu, anchor};
        menu.connect('open-state-changed', (_menu, open) => {
            // Once the item chosen has done its work.
            if (!open)
                GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => this._closeMenu(menu));
        });
        menu.open(BoxPointer.PopupAnimation.FULL);
        return true;
    }

    _closeMenu(which = null) {
        if (!this._menu || (which && this._menu.menu !== which))
            return GLib.SOURCE_REMOVE;
        const {menu, anchor} = this._menu;
        this._menu = null;
        menu.destroy();
        anchor.destroy();
        return GLib.SOURCE_REMOVE;
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
            Screenshot: () => {
                if (Main.sessionMode.isLocked)
                    return false;
                Main.screenshotUI.open();
                return true;
            },
            DockWatch: on => {
                this._dock.watch(on);
                return true;
            },
            DockApps: () => this._dock.apps(),
            DockWindow: (id, action) => this._dock.window(id, action),
            DockLaunch: (app, newWindow) => this._dock.launch(app, newWindow),
            DockShell: what => this._dock.shell(what),
            DockMenu: app => this._dock.menu(app),
            Version: 5,
        });
        this._dock = new Dock(what =>
            this._dbus?.emit_signal('DockChanged', new GLib.Variant('(s)', [what])));
        this._dbus.export(Gio.DBus.session, PATH);
        // GNOME turns this extension off and on again whenever one before it
        // is turned off (the Ubuntu Dock, by Ciel's dock): Ciel asks again.
        this._dock.ready();
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
        this._dock?.watch(false);
        this._dock = null;
        this._dbus?.unexport();
        this._dbus = null;
        this._panel?.set(false);
        this._panel = null;
    }
}
