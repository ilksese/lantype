use std::sync::Arc;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use tokio::sync::Mutex;

pub struct KeyboardInjector {
    enigo: Arc<Mutex<Enigo>>,
}

impl KeyboardInjector {
    pub fn new() -> Self {
        let enigo = Enigo::new(&Settings::default()).expect("enigo init");
        Self {
            enigo: Arc::new(Mutex::new(enigo)),
        }
    }

    pub async fn type_text(&self, text: &str) -> Result<(), String> {
        let mut enigo = self.enigo.lock().await;
        enigo.text(text).map_err(|e| format!("enigo text: {e}"))?;
        Ok(())
    }

    pub async fn delete_chars(&self, count: u32) -> Result<(), String> {
        let mut enigo = self.enigo.lock().await;
        for _ in 0..count {
            enigo
                .key(Key::Backspace, Direction::Click)
                .map_err(|e| format!("enigo backspace: {e}"))?;
        }
        Ok(())
    }
}