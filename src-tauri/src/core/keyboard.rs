use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

enum Command {
    TypeText(String),
    DeleteChars(u32),
    PressChord { modifiers: Vec<Modifier>, key: Key },
}

/// A keyboard modifier. `Control` maps to Command on macOS so that a
/// phone-side "Ctrl+V" behaves as users expect (⌘V = paste).
#[derive(Clone, Copy)]
enum Modifier {
    Control,
    Shift,
    Alt,
    Meta,
}

impl Modifier {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some(Modifier::Control),
            "shift" => Some(Modifier::Shift),
            "alt" | "option" => Some(Modifier::Alt),
            "meta" | "cmd" | "win" | "super" | "command" => Some(Modifier::Meta),
            _ => None,
        }
    }

    fn enigo_key(self) -> Key {
        match self {
            Modifier::Control => {
                if cfg!(target_os = "macos") {
                    Key::Meta
                } else {
                    Key::Control
                }
            }
            Modifier::Shift => Key::Shift,
            Modifier::Alt => Key::Alt,
            Modifier::Meta => Key::Meta,
        }
    }
}

const F_KEYS: [Key; 20] = [
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
    Key::F13,
    Key::F14,
    Key::F15,
    Key::F16,
    Key::F17,
    Key::F18,
    Key::F19,
    Key::F20,
];

/// Resolve a normalized key string (lowercase) into an enigo `Key`.
fn parse_key(s: &str) -> Option<Key> {
    let k = s.to_ascii_lowercase();
    match k.as_str() {
        "enter" | "return" => Some(Key::Return),
        "esc" | "escape" => Some(Key::Escape),
        "tab" => Some(Key::Tab),
        "space" => Some(Key::Space),
        "backspace" => Some(Key::Backspace),
        "delete" => Some(Key::Delete),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "up" => Some(Key::UpArrow),
        "down" => Some(Key::DownArrow),
        "left" => Some(Key::LeftArrow),
        "right" => Some(Key::RightArrow),
        _ => {
            if let Some(num) = k.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
                if (1..=20).contains(&num) {
                    return Some(F_KEYS[(num - 1) as usize]);
                }
                return None;
            }
            let mut chars = k.chars();
            let c = chars.next()?;
            if chars.next().is_none() {
                return Some(Key::Unicode(c));
            }
            None
        }
    }
}

pub struct KeyboardInjector {
    tx: mpsc::Sender<Command>,
    healthy: Arc<AtomicBool>,
}

impl KeyboardInjector {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        let healthy = Arc::new(AtomicBool::new(true));
        let healthy_clone = healthy.clone();

        std::thread::spawn(move || {
            let mut enigo = match Enigo::new(&Settings::default()) {
                Ok(e) => e,
                Err(e) => {
                    log::error!("enigo init failed: {e}");
                    healthy_clone.store(false, Ordering::Relaxed);
                    return;
                }
            };

            #[cfg(target_os = "linux")]
            let mut clipboard_paster = ClipboardPaster::new();

            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Command::TypeText(text) => {
                        #[cfg(target_os = "linux")]
                        type_text(&mut enigo, &mut clipboard_paster, &text);

                        #[cfg(not(target_os = "linux"))]
                        type_text(&mut enigo, &text);
                    }
                    Command::DeleteChars(count) => {
                        for _ in 0..count {
                            if let Err(e) = enigo.key(Key::Backspace, Direction::Click) {
                                log::error!("enigo backspace error: {e}");
                                break;
                            }
                        }
                    }
                    Command::PressChord { modifiers, key } => {
                        for m in &modifiers {
                            if let Err(e) = enigo.key(m.enigo_key(), Direction::Press) {
                                log::error!("enigo modifier press error: {e}");
                            }
                        }
                        if let Err(e) = enigo.key(key, Direction::Click) {
                            log::error!("enigo chord key error: {e}");
                        }
                        for m in modifiers.iter().rev() {
                            if let Err(e) = enigo.key(m.enigo_key(), Direction::Release) {
                                log::error!("enigo modifier release error: {e}");
                            }
                        }
                    }
                }
            }
        });

        Self { tx, healthy }
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    pub async fn type_text(&self, text: String) -> Result<(), String> {
        self.tx
            .send(Command::TypeText(text))
            .map_err(|e| format!("enigo channel: {e}"))
    }

    pub async fn delete_chars(&self, count: u32) -> Result<(), String> {
        self.tx
            .send(Command::DeleteChars(count))
            .map_err(|e| format!("enigo channel: {e}"))
    }

    pub async fn press_chord(&self, modifiers: Vec<String>, key: String) -> Result<(), String> {
        let mut mods = Vec::with_capacity(modifiers.len());
        for m in modifiers {
            mods.push(Modifier::parse(&m).ok_or_else(|| format!("unknown modifier: {m}"))?);
        }
        let key = parse_key(&key).ok_or_else(|| format!("unknown key: {key}"))?;
        self.tx
            .send(Command::PressChord {
                modifiers: mods,
                key,
            })
            .map_err(|e| format!("enigo channel: {e}"))
    }
}

#[cfg(not(target_os = "linux"))]
fn type_text(enigo: &mut Enigo, text: &str) {
    if type_returns(enigo, text) {
        return;
    }

    if let Err(e) = enigo.text(text) {
        log::error!("enigo type_text error: {e}");
    }
}

#[cfg(target_os = "linux")]
fn type_text(enigo: &mut Enigo, clipboard_paster: &mut ClipboardPaster, text: &str) {
    if type_returns(enigo, text) {
        return;
    }

    if text.chars().any(|c| !c.is_ascii()) {
        match clipboard_paster.paste_text(enigo, text) {
            Ok(()) => return,
            Err(e) => log::warn!("clipboard paste fallback failed: {e}"),
        }
    }

    if let Err(e) = enigo.text(text) {
        log::error!("enigo type_text error: {e}");
    }
}

fn type_returns(enigo: &mut Enigo, text: &str) -> bool {
    if text.is_empty() || !text.chars().all(|c| c == '\n' || c == '\r') {
        return false;
    }

    let count = text.chars().filter(|&c| c == '\n' || c == '\r').count();
    for _ in 0..count {
        if let Err(e) = enigo.key(Key::Return, Direction::Click) {
            log::error!("enigo return error: {e}");
            break;
        }
    }

    true
}

#[cfg(target_os = "linux")]
struct ClipboardPaster {
    clipboard: Option<arboard::Clipboard>,
}

#[cfg(target_os = "linux")]
impl ClipboardPaster {
    fn new() -> Self {
        Self { clipboard: None }
    }

    fn paste_text(&mut self, enigo: &mut Enigo, text: &str) -> Result<(), String> {
        if self.clipboard.is_none() {
            self.clipboard =
                Some(arboard::Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?);
        }

        let clipboard = self
            .clipboard
            .as_mut()
            .ok_or_else(|| "clipboard unavailable".to_string())?;

        clipboard
            .set_text(text.to_string())
            .map_err(|e| format!("clipboard set: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(80));

        press_paste_shortcut(enigo)?;
        std::thread::sleep(std::time::Duration::from_millis(120));

        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn press_paste_shortcut(enigo: &mut Enigo) -> Result<(), String> {
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| format!("control press: {e}"))?;
    enigo
        .key(Key::Shift, Direction::Press)
        .map_err(|e| format!("shift press: {e}"))?;

    let click_result = enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| format!("v click: {e}"));
    let shift_release_result = enigo
        .key(Key::Shift, Direction::Release)
        .map_err(|e| format!("shift release: {e}"));
    let release_result = enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| format!("control release: {e}"));

    click_result.and(shift_release_result).and(release_result)
}
