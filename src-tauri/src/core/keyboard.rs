use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use tokio::sync::oneshot;

type CommandResult = Result<(), String>;
type Completion = oneshot::Sender<CommandResult>;

enum Command {
    TypeText {
        text: String,
        completion: Completion,
    },
    DeleteChars {
        count: u32,
        completion: Completion,
    },
    ApplyDiff {
        backspace: u32,
        text: String,
        completion: Completion,
    },
    PressChord {
        modifiers: Vec<Modifier>,
        key: Key,
        completion: Completion,
    },
    Recheck {
        completion: Completion,
    },
}

impl Command {
    fn finish(self, result: CommandResult) {
        let completion = match self {
            Command::TypeText { completion, .. }
            | Command::DeleteChars { completion, .. }
            | Command::ApplyDiff { completion, .. }
            | Command::PressChord { completion, .. }
            | Command::Recheck { completion } => completion,
        };
        let _ = completion.send(result);
    }
}

/// A keyboard modifier. `Control` maps to Command on macOS so that a
/// phone-side "Ctrl+V" behaves as users expect (⌘V = paste).
#[derive(Debug, Clone, Copy)]
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

fn parse_chord(modifiers: &[String], key: &str) -> Result<(Vec<Modifier>, Key), String> {
    let mut parsed_modifiers = Vec::with_capacity(modifiers.len());
    for modifier in modifiers {
        parsed_modifiers.push(
            Modifier::parse(modifier).ok_or_else(|| format!("unknown modifier: {modifier}"))?,
        );
    }
    let parsed_key = parse_key(key).ok_or_else(|| format!("unknown key: {key}"))?;
    Ok((parsed_modifiers, parsed_key))
}

fn format_initialization_error(error: impl Display) -> String {
    format!("keyboard unavailable: enigo initialization failed: {error}")
}

fn format_enigo_error(operation: &str, error: impl Display) -> String {
    format!("enigo {operation} error: {error}")
}

fn record_command_health(healthy: &AtomicBool, result: &CommandResult) {
    if result.is_err() {
        healthy.store(false, Ordering::Release);
    }
}

pub struct KeyboardInjector {
    tx: mpsc::Sender<Command>,
    healthy: Arc<AtomicBool>,
}

impl KeyboardInjector {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        let healthy = Arc::new(AtomicBool::new(false));
        let healthy_clone = healthy.clone();
        let (init_tx, init_rx) = mpsc::sync_channel(1);

        std::thread::spawn(move || {
            let mut enigo = match Enigo::new(&Settings::default()) {
                Ok(enigo) => {
                    healthy_clone.store(true, Ordering::Release);
                    let _ = init_tx.send(Ok(()));
                    Some(enigo)
                }
                Err(e) => {
                    let error = format_initialization_error(e);
                    log::error!("{error}");
                    healthy_clone.store(false, Ordering::Release);
                    let _ = init_tx.send(Err(error.clone()));
                    None
                }
            };

            #[cfg(target_os = "linux")]
            let mut clipboard_paster = ClipboardPaster::new();

            while let Ok(command) = rx.recv() {
                match command {
                    Command::TypeText { text, completion } => {
                        #[cfg(target_os = "linux")]
                        let result = enigo.as_mut().map_or_else(
                            || Err("keyboard unavailable: recheck permission".to_string()),
                            |enigo| type_text(enigo, &mut clipboard_paster, &text),
                        );

                        #[cfg(not(target_os = "linux"))]
                        let result = enigo.as_mut().map_or_else(
                            || Err("keyboard unavailable: recheck permission".to_string()),
                            |enigo| type_text(enigo, &text),
                        );

                        record_command_health(&healthy_clone, &result);
                        let _ = completion.send(result);
                    }
                    Command::DeleteChars { count, completion } => {
                        let result = enigo.as_mut().map_or_else(
                            || Err("keyboard unavailable: recheck permission".to_string()),
                            |enigo| delete_chars(enigo, count),
                        );
                        record_command_health(&healthy_clone, &result);
                        let _ = completion.send(result);
                    }
                    Command::ApplyDiff {
                        backspace,
                        text,
                        completion,
                    } => {
                        #[cfg(target_os = "linux")]
                        let result = enigo.as_mut().map_or_else(
                            || Err("keyboard unavailable: recheck permission".to_string()),
                            |enigo| apply_diff(enigo, &mut clipboard_paster, backspace, &text),
                        );

                        #[cfg(not(target_os = "linux"))]
                        let result = enigo.as_mut().map_or_else(
                            || Err("keyboard unavailable: recheck permission".to_string()),
                            |enigo| apply_diff(enigo, backspace, &text),
                        );

                        record_command_health(&healthy_clone, &result);
                        let _ = completion.send(result);
                    }
                    Command::PressChord {
                        modifiers,
                        key,
                        completion,
                    } => {
                        let result = enigo.as_mut().map_or_else(
                            || Err("keyboard unavailable: recheck permission".to_string()),
                            |enigo| press_chord(enigo, &modifiers, key),
                        );
                        record_command_health(&healthy_clone, &result);
                        let _ = completion.send(result);
                    }
                    Command::Recheck { completion } => {
                        let result = match Enigo::new(&Settings::default()) {
                            Ok(new_enigo) => {
                                enigo = Some(new_enigo);
                                healthy_clone.store(true, Ordering::Release);
                                Ok(())
                            }
                            Err(error) => {
                                healthy_clone.store(false, Ordering::Release);
                                Err(format_initialization_error(error))
                            }
                        };
                        let _ = completion.send(result);
                    }
                }
            }
            healthy_clone.store(false, Ordering::Release);
        });

        match init_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::warn!("{error}"),
            Err(_) => log::error!("keyboard injector initialization thread stopped"),
        }

        Self { tx, healthy }
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    pub async fn type_text(&self, text: String) -> Result<(), String> {
        self.execute("type_text", |completion| Command::TypeText {
            text,
            completion,
        })
        .await
    }

    pub async fn delete_chars(&self, count: u32) -> Result<(), String> {
        self.execute("delete_chars", |completion| Command::DeleteChars {
            count,
            completion,
        })
        .await
    }

    pub async fn apply_diff(&self, backspace: u32, text: String) -> Result<(), String> {
        self.execute("apply_diff", |completion| Command::ApplyDiff {
            backspace,
            text,
            completion,
        })
        .await
    }

    pub async fn press_chord(&self, modifiers: Vec<String>, key: String) -> Result<(), String> {
        let (modifiers, key) = parse_chord(&modifiers, &key)?;
        self.execute("press_chord", |completion| Command::PressChord {
            modifiers,
            key,
            completion,
        })
        .await
    }

    pub async fn recheck(&self) -> Result<(), String> {
        self.execute("recheck", |completion| Command::Recheck { completion })
            .await
    }

    async fn execute(
        &self,
        operation: &str,
        build: impl FnOnce(Completion) -> Command,
    ) -> Result<(), String> {
        let (completion, result) = oneshot::channel();
        if let Err(error) = self.tx.send(build(completion)) {
            self.healthy.store(false, Ordering::Release);
            return Err(format!("enigo channel: {error}"));
        }
        match result.await {
            Ok(result) => result,
            Err(_) => {
                self.healthy.store(false, Ordering::Release);
                Err(format!(
                    "enigo worker stopped before completing {operation}"
                ))
            }
        }
    }
}

impl Default for KeyboardInjector {
    fn default() -> Self {
        Self::new()
    }
}

fn delete_chars(enigo: &mut Enigo, count: u32) -> Result<(), String> {
    for _ in 0..count {
        enigo
            .key(Key::Backspace, Direction::Click)
            .map_err(|e| format_enigo_error("backspace", e))?;
    }
    Ok(())
}

fn press_chord(enigo: &mut Enigo, modifiers: &[Modifier], key: Key) -> Result<(), String> {
    let mut pressed = Vec::with_capacity(modifiers.len());
    let mut first_error = None;

    for modifier in modifiers {
        let enigo_key = modifier.enigo_key();
        match enigo.key(enigo_key, Direction::Press) {
            Ok(()) => pressed.push(enigo_key),
            Err(e) => {
                if first_error.is_none() {
                    first_error = Some(format_enigo_error("modifier press", e));
                }
            }
        }
    }

    if let Err(e) = enigo.key(key, Direction::Click) {
        if first_error.is_none() {
            first_error = Some(format_enigo_error("chord key", e));
        }
    }

    for enigo_key in pressed.into_iter().rev() {
        if let Err(e) = enigo.key(enigo_key, Direction::Release) {
            if first_error.is_none() {
                first_error = Some(format_enigo_error("modifier release", e));
            }
        }
    }

    first_error.map_or(Ok(()), Err)
}

#[cfg(not(target_os = "linux"))]
fn type_text(enigo: &mut Enigo, text: &str) -> Result<(), String> {
    if type_returns(enigo, text)? {
        return Ok(());
    }

    enigo
        .text(text)
        .map_err(|e| format_enigo_error("type_text", e))
}

#[cfg(target_os = "linux")]
fn type_text(
    enigo: &mut Enigo,
    clipboard_paster: &mut ClipboardPaster,
    text: &str,
) -> Result<(), String> {
    if type_returns(enigo, text)? {
        return Ok(());
    }

    let clipboard_error = if text.chars().any(|c| !c.is_ascii()) {
        match clipboard_paster.paste_text(enigo, text) {
            Ok(()) => return Ok(()),
            Err(error) => {
                log::warn!("clipboard paste fallback failed: {error}");
                Some(error)
            }
        }
    } else {
        None
    };

    enigo.text(text).map_err(|error| {
        let type_error = format_enigo_error("type_text", error);
        match clipboard_error {
            Some(clipboard_error) => {
                format!("{type_error}; clipboard fallback error: {clipboard_error}")
            }
            None => type_error,
        }
    })
}

#[cfg(not(target_os = "linux"))]
fn apply_diff(enigo: &mut Enigo, backspace: u32, text: &str) -> Result<(), String> {
    delete_chars(enigo, backspace)?;
    if !text.is_empty() {
        type_text(enigo, text)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_diff(
    enigo: &mut Enigo,
    clipboard_paster: &mut ClipboardPaster,
    backspace: u32,
    text: &str,
) -> Result<(), String> {
    delete_chars(enigo, backspace)?;
    if !text.is_empty() {
        type_text(enigo, clipboard_paster, text)?;
    }
    Ok(())
}

fn type_returns(enigo: &mut Enigo, text: &str) -> Result<bool, String> {
    if text.is_empty() || !text.chars().all(|c| c == '\n' || c == '\r') {
        return Ok(false);
    }

    let count = text.chars().count();
    for _ in 0..count {
        enigo
            .key(Key::Return, Direction::Click)
            .map_err(|e| format_enigo_error("return", e))?;
    }

    Ok(true)
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
        .map_err(|e| format_enigo_error("control press", e))?;
    enigo
        .key(Key::Shift, Direction::Press)
        .map_err(|e| format_enigo_error("shift press", e))?;

    let click_result = enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| format_enigo_error("v click", e));
    let shift_release_result = enigo
        .key(Key::Shift, Direction::Release)
        .map_err(|e| format_enigo_error("shift release", e));
    let release_result = enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| format_enigo_error("control release", e));

    click_result.and(shift_release_result).and(release_result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_key_returns_a_clear_error() {
        let modifiers = Vec::new();

        let error = parse_chord(&modifiers, "not-a-key").unwrap_err();

        assert_eq!(error, "unknown key: not-a-key");
    }

    #[test]
    fn permission_error_keeps_the_underlying_reason() {
        let error = format_initialization_error("permission denied");

        assert_eq!(
            error,
            "keyboard unavailable: enigo initialization failed: permission denied"
        );
    }

    #[test]
    fn runtime_error_marks_keyboard_unhealthy() {
        let healthy = AtomicBool::new(true);
        let result = Err("runtime failure".to_string());

        record_command_health(&healthy, &result);

        assert!(!healthy.load(Ordering::Acquire));
    }
}
