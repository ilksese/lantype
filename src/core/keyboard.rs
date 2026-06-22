use std::sync::mpsc;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

enum Command {
    TypeText(String),
    DeleteChars(u32),
}

pub struct KeyboardInjector {
    tx: mpsc::Sender<Command>,
}

impl KeyboardInjector {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        std::thread::spawn(move || {
            let mut enigo = Enigo::new(&Settings::default()).expect("enigo init");
            while let Ok(cmd) = rx.recv() {
                let _result = match cmd {
                    Command::TypeText(text) => enigo.text(&text),
                    Command::DeleteChars(count) => {
                        for _ in 0..count {
                            if let Err(e) = enigo.key(Key::Backspace, Direction::Click) {
                                log::error!("enigo backspace error: {e}");
                                break;
                            }
                        }
                        Ok(())
                    }
                };
            }
        });
        Self { tx }
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