// Flow dictation on macOS: not yet. Watching the key and sending the paste
// need the Accessibility permission (a CGEventTap and CGEventPost), which
// comes with the macOS window work. Until then the key can't be watched, so
// dictation stays off, and the pill's own button is the only way in.

use std::sync::mpsc::Sender;
use std::time::Instant;

use super::logic::FlowKey;
use super::KeyEvent;

/// Nothing to stop: no key is watched.
pub struct KeyListener;

/// "no-display": the settings say the key can't be watched on this system.
pub fn keys_supported() -> Result<(), &'static str> {
    Err("no-display")
}

pub fn listen(_key: FlowKey, _tx: Sender<(KeyEvent, Instant)>) -> Result<KeyListener, String> {
    Err("dictation keys are not available on macOS yet".into())
}

pub fn wait_key_released(_key: FlowKey) {}

pub fn send_paste(_shift: bool) -> Result<(), String> {
    Err("pasting is not available on macOS yet".into())
}

pub fn foreground_app() -> Option<(String, String)> {
    None
}

pub fn running_apps() -> Vec<(String, String)> {
    Vec::new()
}
