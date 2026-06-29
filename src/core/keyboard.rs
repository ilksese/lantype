use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

enum Command {
    TypeText(String),
    DeleteChars(u32),
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

            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Command::TypeText(text) => {
                        if let Err(e) = enigo.text(&text) {
                            log::error!("enigo type_text error: {e}");
                        }
                    }
                    Command::DeleteChars(count) => {
                        for _ in 0..count {
                            if let Err(e) = enigo.key(Key::Backspace, Direction::Click) {
                                log::error!("enigo backspace error: {e}");
                                break;
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
}