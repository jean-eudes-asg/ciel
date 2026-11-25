// The windows of an app, above its icon while the pointer is on it, as the
// taskbar shows them: live pictures DWM draws (DwmRegisterThumbnail) into a
// window of the dock's own, with their titles. A click brings one forward;
// its × or a middle click closes it (the window is asked, as its own × does).
//
// It lives on the dock's watch thread (win.rs), from where it is asked to
// show and to go; nothing here runs while it is hidden. It never takes the
// keyboard, and it is left out of screen shares as Ciel's windows are.

use std::cell::RefCell;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmSetWindowAttribute, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_SOURCECLIENTAREAONLY,
    DWM_TNP_VISIBLE,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    FillRgn, InvalidateRect, SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HFONT,
    HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowRect, GetWindowTextW, IsWindow,
    KillTimer, LoadCursorW, PostMessageW, RegisterClassW, SetTimer, SetWindowDisplayAffinity, SetWindowPos,
    ShowWindow, HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WDA_EXCLUDEFROMCAPTURE,
    WDA_NONE, WM_CLOSE, WM_ERASEBKGND, WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// The pointer left: the preview goes this long after, unless it comes back.
const HIDE_TIMER: usize = 1;
const HIDE_MS: u32 = 300;
/// Sizes in logical pixels: a picture's room, the space around, the title's line.
const THUMB_W: f64 = 208.0;
const THUMB_H: f64 = 124.0;
const PAD: f64 = 8.0;
const TITLE_H: f64 = 26.0;
const CLOSE: f64 = 22.0;

struct Card {
    window: isize,
    title: String,
    thumb: isize,
    rect: RECT,
    close: RECT,
}

/// Where it was asked to show, kept to lay it out again when a window closes.
#[derive(Clone, Copy)]
struct Place {
    center_x: i32,
    bottom: i32,
    screen: RECT,
    scale: f64,
}

struct Preview {
    hwnd: HWND,
    for_id: Option<String>,
    cards: Vec<Card>,
    place: Option<Place>,
    light: bool,
    font: Option<HFONT>,
    hot: Option<usize>,
    hot_close: bool,
    tracking: bool,
}

thread_local! {
    static PREVIEW: RefCell<Option<Preview>> = const { RefCell::new(None) };
}

/// Its window, hidden: made with the watch, gone with it.
pub fn create(instance: HINSTANCE) {
    unsafe {
        let class = w!("CielDockPreview");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class,
            w!("Ciel"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        ) else {
            crate::log::line("dock: no window previews");
            return;
        };
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as *const _,
            std::mem::size_of_val(&round) as u32,
        );
        PREVIEW.with(|p| {
            *p.borrow_mut() = Some(Preview {
                hwnd,
                for_id: None,
                cards: Vec::new(),
                place: None,
                light: false,
                font: None,
                hot: None,
                hot_close: false,
                tracking: false,
            })
        });
    }
}

pub fn destroy() {
    hide();
    let gone = PREVIEW.with(|p| p.borrow_mut().take());
    if let Some(p) = gone {
        unsafe {
            if let Some(font) = p.font {
                let _ = DeleteObject(HGDIOBJ(font.0));
            }
            let _ = DestroyWindow(p.hwnd);
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

/// Monochrome, as the dock: black or white, a grey for the one under the pointer.
fn colors(light: bool) -> (COLORREF, COLORREF, COLORREF, COLORREF) {
    if light {
        (rgb(255, 255, 255), rgb(235, 235, 237), rgb(17, 18, 20), rgb(224, 224, 226))
    } else {
        (rgb(0, 0, 0), rgb(32, 32, 34), rgb(245, 246, 248), rgb(38, 38, 40))
    }
}

fn title_of(hwnd: isize) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetWindowTextW(HWND(hwnd as *mut _), &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

/// Shows `windows` (an app's, `id`), centred on `center_x` above `bottom`.
/// The same app's same windows already showing: only kept.
pub fn show(id: &str, windows: &[isize], center_x: i32, bottom: i32, screen: RECT, scale: f64, light: bool) {
    let windows: Vec<isize> =
        windows.iter().copied().filter(|w| unsafe { IsWindow(Some(HWND(*w as *mut _))) }.as_bool()).collect();
    if windows.is_empty() {
        hide();
        return;
    }
    let same = PREVIEW.with(|p| {
        let p = p.borrow();
        let Some(p) = p.as_ref() else { return true };
        unsafe {
            let _ = KillTimer(Some(p.hwnd), HIDE_TIMER);
        }
        p.for_id.as_deref() == Some(id) && p.light == light && p.cards.iter().map(|c| c.window).eq(windows.iter().copied())
    });
    if same {
        return;
    }
    let Some(hwnd) = PREVIEW.with(|p| p.borrow().as_ref().map(|p| p.hwnd)) else { return };
    release_thumbs();
    let mut cards = Vec::new();
    for window in windows {
        let Ok(thumb) = (unsafe { DwmRegisterThumbnail(hwnd, HWND(window as *mut _)) }) else { continue };
        cards.push(Card { window, title: title_of(window), thumb, rect: RECT::default(), close: RECT::default() });
    }
    if cards.is_empty() {
        hide();
        return;
    }
    let font = unsafe {
        CreateFontW(
            -(12.0 * scale).round() as i32,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI Variable Text"),
        )
    };
    PREVIEW.with(|p| {
        if let Some(p) = p.borrow_mut().as_mut() {
            if let Some(old) = p.font.replace(font) {
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(old.0));
                }
            }
            p.for_id = Some(id.to_string());
            p.cards = cards;
            p.light = light;
            p.hot = None;
            p.hot_close = false;
            p.place = Some(Place { center_x, bottom, screen, scale });
        }
    });
    layout();
}

/// The cards side by side, as many as the screen holds, and the window around them.
fn layout() {
    let laid = PREVIEW.with(|p| {
        let mut p = p.borrow_mut();
        let p = p.as_mut()?;
        let place = p.place?;
        let s = place.scale;
        let px = |v: f64| (v * s).round() as i32;
        let (card_w, card_h) = (px(THUMB_W + 2.0 * PAD), px(TITLE_H + THUMB_H + 2.0 * PAD));
        let gap = px(PAD);
        let room = (place.screen.right - place.screen.left - 2 * gap).max(card_w + 2 * gap);
        let fits = ((room - gap) / (card_w + gap)).max(1) as usize;
        // More windows than the screen holds: the ones nearest the front.
        for card in p.cards.drain(fits.min(p.cards.len())..).collect::<Vec<_>>() {
            unsafe {
                let _ = DwmUnregisterThumbnail(card.thumb);
            }
        }
        let n = p.cards.len() as i32;
        let width = n * card_w + (n + 1) * gap;
        let height = card_h + 2 * gap;
        let x = (place.center_x - width / 2).clamp(place.screen.left + gap, (place.screen.right - gap - width).max(place.screen.left));
        let y = place.bottom - height;
        for (i, card) in p.cards.iter_mut().enumerate() {
            let left = gap + i as i32 * (card_w + gap);
            card.rect = RECT { left, top: gap, right: left + card_w, bottom: gap + card_h };
            let c = px(CLOSE);
            let inset = px(PAD) / 2 + px(2.0);
            card.close = RECT { left: card.rect.right - inset - c, top: card.rect.top + inset, right: card.rect.right - inset, bottom: card.rect.top + inset + c };
            // The picture, its shape kept, in its room under the title.
            let room = RECT {
                left: card.rect.left + px(PAD),
                top: card.rect.top + px(PAD + TITLE_H),
                right: card.rect.right - px(PAD),
                bottom: card.rect.bottom - px(PAD),
            };
            let (rw, rh) = ((room.right - room.left) as f64, (room.bottom - room.top) as f64);
            let (sw, sh) = match unsafe { DwmQueryThumbnailSourceSize(card.thumb) } {
                Ok(size) if size.cx > 0 && size.cy > 0 => (size.cx as f64, size.cy as f64),
                _ => (rw, rh),
            };
            let fit = (rw / sw).min(rh / sh).min(1.0);
            let (dw, dh) = ((sw * fit).round() as i32, (sh * fit).round() as i32);
            let dx = room.left + ((room.right - room.left) - dw) / 2;
            let dy = room.top + ((room.bottom - room.top) - dh) / 2;
            let props = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
                rcDestination: RECT { left: dx, top: dy, right: dx + dw, bottom: dy + dh },
                opacity: 255,
                fVisible: true.into(),
                fSourceClientAreaOnly: false.into(),
                ..Default::default()
            };
            unsafe {
                let _ = DwmUpdateThumbnailProperties(card.thumb, &props);
            }
        }
        Some((p.hwnd, x, y, width, height, p.light))
    });
    let Some((hwnd, x, y, width, height, light)) = laid else { return };
    unsafe {
        let line = colors(light).3;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &line as *const _ as *const _, std::mem::size_of::<COLORREF>() as u32);
        let _ = SetWindowDisplayAffinity(hwnd, if crate::capture::hidden() { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE });
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

fn release_thumbs() {
    PREVIEW.with(|p| {
        if let Some(p) = p.borrow_mut().as_mut() {
            for card in p.cards.drain(..) {
                unsafe {
                    let _ = DwmUnregisterThumbnail(card.thumb);
                }
            }
        }
    });
}

/// The pointer left the app's icon: the preview goes in a moment, unless the
/// pointer is on it by then.
pub fn leave() {
    PREVIEW.with(|p| {
        if let Some(p) = p.borrow().as_ref() {
            if p.for_id.is_some() {
                unsafe {
                    let _ = SetTimer(Some(p.hwnd), HIDE_TIMER, HIDE_MS, None);
                }
            }
        }
    });
}

pub fn hide() {
    let hwnd = PREVIEW.with(|p| {
        let mut p = p.borrow_mut();
        let p = p.as_mut()?;
        p.for_id = None;
        p.place = None;
        p.hot = None;
        Some(p.hwnd)
    });
    release_thumbs();
    if let Some(hwnd) = hwnd {
        unsafe {
            let _ = KillTimer(Some(hwnd), HIDE_TIMER);
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

fn inside(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// The card under (x, y), and whether on its ×.
fn hit(p: &Preview, x: i32, y: i32) -> Option<(usize, bool)> {
    p.cards.iter().position(|c| inside(&c.rect, x, y)).map(|i| (i, inside(&p.cards[i].close, x, y)))
}

/// Asks a window to close, and lays out the others.
fn close(i: usize) {
    let thumb = PREVIEW.with(|p| {
        let mut p = p.borrow_mut();
        let p = p.as_mut()?;
        (i < p.cards.len()).then(|| {
            let card = p.cards.remove(i);
            p.hot = None;
            unsafe {
                let _ = PostMessageW(Some(HWND(card.window as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            card.thumb
        })
    });
    if let Some(thumb) = thumb {
        unsafe {
            let _ = DwmUnregisterThumbnail(thumb);
        }
    }
    if PREVIEW.with(|p| p.borrow().as_ref().is_none_or(|p| p.cards.is_empty())) {
        hide();
    } else {
        layout();
    }
}

fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    PREVIEW.with(|p| {
        let Ok(p) = p.try_borrow() else { return };
        let Some(p) = p.as_ref() else { return };
        let (bg, hot_bg, ink, _) = colors(p.light);
        let scale = p.place.map_or(1.0, |pl| pl.scale);
        let px = |v: f64| (v * scale).round() as i32;
        unsafe {
            let mut all = RECT::default();
            let _ = GetWindowRect(hwnd, &mut all);
            let whole = RECT { left: 0, top: 0, right: all.right - all.left, bottom: all.bottom - all.top };
            let brush = CreateSolidBrush(bg);
            FillRect(hdc, &whole, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, ink);
            let old = p.font.map(|f| SelectObject(hdc, HGDIOBJ(f.0)));
            for (i, card) in p.cards.iter().enumerate() {
                let hot = p.hot == Some(i);
                if hot {
                    let r = card.rect;
                    let rgn = CreateRoundRectRgn(r.left, r.top, r.right + 1, r.bottom + 1, px(16.0), px(16.0));
                    let brush = CreateSolidBrush(hot_bg);
                    let _ = FillRgn(hdc, rgn, brush);
                    let _ = DeleteObject(HGDIOBJ(brush.0));
                    let _ = DeleteObject(HGDIOBJ(rgn.0));
                }
                let mut title: Vec<u16> = card.title.encode_utf16().collect();
                let mut line = RECT {
                    left: card.rect.left + px(PAD + 2.0),
                    top: card.rect.top + px(PAD / 2.0),
                    right: card.rect.right - px(PAD + CLOSE + 2.0),
                    bottom: card.rect.top + px(PAD / 2.0 + TITLE_H),
                };
                DrawTextW(hdc, &mut title, &mut line, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
                if hot {
                    let mut cross: Vec<u16> = "✕".encode_utf16().collect();
                    let mut r = card.close;
                    if p.hot_close {
                        let rgn = CreateRoundRectRgn(r.left, r.top, r.right + 1, r.bottom + 1, px(8.0), px(8.0));
                        let brush = CreateSolidBrush(colors(p.light).3);
                        let _ = FillRgn(hdc, rgn, brush);
                        let _ = DeleteObject(HGDIOBJ(brush.0));
                        let _ = DeleteObject(HGDIOBJ(rgn.0));
                    }
                    DrawTextW(hdc, &mut cross, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
                }
            }
            if let Some(old) = old {
                SelectObject(hdc, old);
            }
        }
    });
    unsafe {
        let _ = EndPaint(hwnd, &ps);
    }
}

fn point(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32)
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let (x, y) = point(lparam);
            let changed = PREVIEW.with(|p| {
                let Ok(mut p) = p.try_borrow_mut() else { return false };
                let Some(p) = p.as_mut() else { return false };
                unsafe {
                    let _ = KillTimer(Some(hwnd), HIDE_TIMER);
                }
                if !p.tracking {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    p.tracking = unsafe { TrackMouseEvent(&mut track) }.is_ok();
                }
                let (hot, on_close) = hit(p, x, y).map_or((None, false), |(i, c)| (Some(i), c));
                let changed = hot != p.hot || on_close != p.hot_close;
                p.hot = hot;
                p.hot_close = on_close;
                changed
            });
            if changed {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            PREVIEW.with(|p| {
                if let Ok(mut p) = p.try_borrow_mut() {
                    if let Some(p) = p.as_mut() {
                        p.tracking = false;
                        p.hot = None;
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
                let _ = SetTimer(Some(hwnd), HIDE_TIMER, HIDE_MS, None);
            }
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == HIDE_TIMER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), HIDE_TIMER);
            }
            let mut cursor = POINT::default();
            let mut rect = RECT::default();
            let over = unsafe { GetCursorPos(&mut cursor).is_ok() && GetWindowRect(hwnd, &mut rect).is_ok() }
                && inside(&rect, cursor.x, cursor.y);
            if !over {
                hide();
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP | WM_MBUTTONUP => {
            let (x, y) = point(lparam);
            let target = PREVIEW.with(|p| {
                let p = p.try_borrow().ok()?;
                let p = p.as_ref()?;
                hit(p, x, y).map(|(i, on_close)| (i, on_close, p.cards[i].window))
            });
            if let Some((i, on_close, window)) = target {
                if on_close || msg == WM_MBUTTONUP {
                    close(i);
                } else {
                    hide();
                    crate::platform::bring_forward(window);
                }
            }
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
