// Windows: the desktop layer, where a live wallpaper window goes — above the
// picture, behind the desktop icons.
//
// Progman ("Program Manager") is the desktop. Sent the undocumented message
// 0x052C, it makes a WorkerW window for wallpaper transitions, and that
// window is what wallpaper apps draw into. Where it lands depends on the build:
//
//   * Classic (Windows 10, Windows 11 before 24H2): the icons (SHELLDLL_DefView)
//     move out of Progman into a top-level WorkerW, and a second, empty
//     top-level WorkerW sits right behind it. That one is the parent.
//   * Raised (Windows 11 24H2 and later, build 26300 here): Progman keeps
//     SHELLDLL_DefView and gets a WorkerW *child* of its own, behind the icons.
//     The wallpaper window becomes Progman's child, right under
//     SHELLDLL_DefView and over that WorkerW. Progman has no redirection
//     surface of its own (WS_EX_NOREDIRECTIONBITMAP), so the window is made
//     layered to get one: WebView2's content shows either way, but anything
//     drawn the classic way (the window's own background) would not.
//
// Only the `windows` crate is used here, so the file stands on its own.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    MapWindowPoints, RedrawWindow, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetAncestor, GetClassNameW, GetWindow, GetWindowLongPtrW,
    GetWindowRect, IsWindow, SendMessageTimeoutW, SetLayeredWindowAttributes, SetParent, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GA_PARENT, GWL_EXSTYLE, GWL_STYLE, GW_HWNDFIRST, GW_HWNDNEXT,
    LWA_ALPHA, SMTO_NORMAL, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW, SW_HIDE, WS_CAPTION, WS_CHILD,
    WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
    WS_THICKFRAME,
};

/// Progman's request for the wallpaper WorkerW (what every wallpaper app sends).
const SPAWN_WORKERW: u32 = 0x052C;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Classic,
    Raised,
}

impl Layout {
    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Classic => "classic",
            Layout::Raised => "raised (24H2+)",
        }
    }
}

/// Where a wallpaper window goes right now.
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub layout: Layout,
    /// The window it becomes a child of.
    pub parent: HWND,
    /// The sibling it goes right under (SHELLDLL_DefView in the raised
    /// layout); `None` puts it on top of its parent's other children.
    pub under: Option<HWND>,
}

// HWNDs are plain handles: the layer is looked up again whenever it is used.
unsafe impl Send for Layer {}

fn class_of(hwnd: HWND) -> String {
    let mut name = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut name) };
    String::from_utf16_lossy(&name[..len.max(0) as usize])
}

fn child_of_class(parent: HWND, class: PCWSTR) -> Option<HWND> {
    unsafe { FindWindowExW(Some(parent), None, class, PCWSTR::null()) }.ok()
}

/// Finds the desktop layer, asking Progman for its WorkerW first.
pub fn find() -> Option<Layer> {
    let progman = unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.ok()?;
    let mut result = 0usize;
    unsafe {
        // wParam 0xD / lParam 1 is what makes 24H2 build its WorkerW too.
        SendMessageTimeoutW(progman, SPAWN_WORKERW, WPARAM(0xD), LPARAM(1), SMTO_NORMAL, 1000, Some(&mut result));
    }
    // Raised: the icons stayed in Progman and the WorkerW is its child.
    if let Some(def_view) = child_of_class(progman, w!("SHELLDLL_DefView")) {
        if child_of_class(progman, w!("WorkerW")).is_some() {
            return Some(Layer { layout: Layout::Raised, parent: progman, under: Some(def_view) });
        }
    }
    // Classic: the top-level window that holds the icons, and the WorkerW
    // right behind it.
    struct Found(Option<HWND>);
    unsafe extern "system" fn visit(top: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let found = unsafe { &mut *(lparam.0 as *mut Found) };
        if child_of_class(top, w!("SHELLDLL_DefView")).is_some() {
            found.0 = unsafe { FindWindowExW(None, Some(top), w!("WorkerW"), PCWSTR::null()) }.ok();
        }
        true.into()
    }
    let mut found = Found(None);
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut found as *mut Found as isize));
    }
    found.0.map(|worker| Layer { layout: Layout::Classic, parent: worker, under: None })
}

/// Makes `hwnd` a child of the desktop layer covering `screen` (physical
/// screen pixels): never activated, no taskbar button, click-through.
pub fn attach(hwnd: HWND, screen: RECT) -> Result<Layer, String> {
    let layer = find().ok_or("no desktop layer (is Explorer running?)")?;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let style = (style & !(WS_POPUP.0 as isize) & !(WS_CAPTION.0 as isize) & !(WS_THICKFRAME.0 as isize))
            | WS_CHILD.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
        let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        ex &= !(WS_EX_APPWINDOW.0 as isize);
        ex |= (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0) as isize;
        if layer.layout == Layout::Raised {
            ex |= WS_EX_LAYERED.0 as isize;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        if layer.layout == Layout::Raised {
            // Fully opaque: only there for the surface it brings.
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA).map_err(|e| e.to_string())?;
        }
        SetParent(hwnd, Some(layer.parent)).map_err(|e| format!("SetParent: {e}"))?;
    }
    place(hwnd, &layer, screen)?;
    Ok(layer)
}

/// Puts `hwnd` over `screen` (physical screen pixels) in its parent's client
/// coordinates, right under the icons. A monitor left of or above the primary
/// has negative screen coordinates; the parent's client area starts at the
/// virtual screen's corner, so the mapping takes care of it.
pub fn place(hwnd: HWND, layer: &Layer, screen: RECT) -> Result<(), String> {
    let mut corners = [POINT { x: screen.left, y: screen.top }, POINT { x: screen.right, y: screen.bottom }];
    unsafe {
        MapWindowPoints(None, Some(layer.parent), &mut corners);
    }
    let (x, y) = (corners[0].x.min(corners[1].x), corners[0].y);
    let (w, h) = (screen.right - screen.left, screen.bottom - screen.top);
    // HWND_TOP is the null handle: on top of the parent's children.
    let after = layer.under.unwrap_or_default();
    unsafe {
        SetWindowPos(hwnd, Some(after), x, y, w, h, SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW)
            .map_err(|e| format!("SetWindowPos: {e}"))
    }
}

/// Takes `hwnd` out of the desktop layer, hidden, before it is destroyed.
pub fn detach(hwnd: HWND) {
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = SetParent(hwnd, None);
        }
    }
}

/// Repaints the desktop layer, so nothing of a wallpaper window that was just
/// removed stays on screen over the picture.
pub fn refresh() {
    let Some(layer) = find() else { return };
    let flags = RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN | RDW_UPDATENOW;
    unsafe {
        let _ = RedrawWindow(Some(layer.parent), None, None, flags);
        if layer.layout == Layout::Raised {
            if let Some(worker) = child_of_class(layer.parent, w!("WorkerW")) {
                let _ = RedrawWindow(Some(worker), None, None, flags);
            }
        }
    }
}

// ── Checking where a window is ───────────────────────────────────────────────

/// Class names from `hwnd`'s parent up to the top-level window.
pub fn parent_chain(hwnd: HWND) -> Vec<String> {
    let desktop = unsafe { windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow() };
    let mut out = Vec::new();
    let mut cur = hwnd;
    for _ in 0..16 {
        let parent = unsafe { GetAncestor(cur, GA_PARENT) };
        if parent.is_invalid() || parent == desktop {
            break;
        }
        out.push(class_of(parent));
        cur = parent;
    }
    out
}

/// For each level from the desktop down to `hwnd`: its place among its
/// siblings, 0 being the top of the z-order.
fn z_path(hwnd: HWND) -> Vec<usize> {
    let desktop = unsafe { windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow() };
    let mut chain = vec![hwnd];
    let mut cur = hwnd;
    for _ in 0..16 {
        let parent = unsafe { GetAncestor(cur, GA_PARENT) };
        if parent.is_invalid() || parent == desktop {
            break;
        }
        chain.push(parent);
        cur = parent;
    }
    chain.reverse();
    chain
        .iter()
        .map(|&w| {
            let mut index = 0;
            let mut sib = unsafe { GetWindow(w, GW_HWNDFIRST) }.ok();
            while let Some(s) = sib {
                if s == w {
                    break;
                }
                index += 1;
                sib = unsafe { GetWindow(s, GW_HWNDNEXT) }.ok();
            }
            index
        })
        .collect()
}

/// Whether `a` is drawn over `b`: the first level where their paths part
/// decides (a lower index is higher in the z-order).
pub fn above(a: HWND, b: HWND) -> bool {
    z_path_above(&z_path(a), &z_path(b))
}

pub fn z_path_above(a: &[usize], b: &[usize]) -> bool {
    for (x, y) in a.iter().zip(b) {
        if x != y {
            return x < y;
        }
    }
    // One holds the other: a child is drawn over its parent.
    a.len() > b.len()
}

/// The icons' window, wherever the layout put it.
pub fn def_view() -> Option<HWND> {
    let progman = unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.ok()?;
    if let Some(v) = child_of_class(progman, w!("SHELLDLL_DefView")) {
        return Some(v);
    }
    let mut worker = unsafe { FindWindowExW(None, None, w!("WorkerW"), PCWSTR::null()) }.ok();
    while let Some(w) = worker {
        if let Some(v) = child_of_class(w, w!("SHELLDLL_DefView")) {
            return Some(v);
        }
        worker = unsafe { FindWindowExW(None, Some(w), w!("WorkerW"), PCWSTR::null()) }.ok();
    }
    None
}

/// One line for the log: the parent chain, whether the icons are drawn over
/// the window, and its rectangle on screen.
pub fn describe(hwnd: HWND) -> String {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
    }
    let icons_above = def_view().map(|v| above(v, hwnd));
    format!(
        "parents [{}], icons above: {}, rect ({},{})-({},{})",
        parent_chain(hwnd).join(" < "),
        match icons_above {
            Some(true) => "yes",
            Some(false) => "NO",
            None => "no icons window",
        },
        rect.left,
        rect.top,
        rect.right,
        rect.bottom
    )
}

#[cfg(test)]
mod tests {
    use super::z_path_above;

    #[test]
    fn the_first_level_where_two_windows_part_decides_which_is_above() {
        // Siblings under Progman: the icons (0) over the wallpaper window (1).
        assert!(z_path_above(&[197, 0], &[197, 1]));
        assert!(!z_path_above(&[197, 1], &[197, 0]));
        // Classic: the icons' WorkerW is a top-level window in front of ours.
        assert!(z_path_above(&[83, 0], &[84, 0]));
        // A child is drawn over its parent.
        assert!(z_path_above(&[197, 0], &[197]));
        assert!(!z_path_above(&[197], &[197, 0]));
    }
}
