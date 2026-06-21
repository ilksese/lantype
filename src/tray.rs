use std::sync::atomic::{AtomicBool, Ordering};

use log::info;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

pub struct PrivacyState {
    pub enabled: AtomicBool,
}

pub fn setup_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show_item = MenuItemBuilder::with_id("show", "显示窗口").build(app)?;
    let privacy_item = MenuItemBuilder::with_id("privacy", "隐私模式: 开启").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .item(&privacy_item)
        .item(&quit_item)
        .build()?;

    TrayIconBuilder::with_id("main")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "privacy" => {
                if let Some(state) = app.try_state::<PrivacyState>() {
                    let new_val = !state.enabled.load(Ordering::Relaxed);
                    state.enabled.store(new_val, Ordering::Relaxed);
                    let label = if new_val { "隐私模式: 关闭" } else { "隐私模式: 开启" };
                    let _ = app.tray_by_id("main").map(|t| t.set_menu(Some({
                        let show_item = MenuItemBuilder::with_id("show", "显示窗口").build(app).unwrap();
                        let privacy_item = MenuItemBuilder::with_id("privacy", label).build(app).unwrap();
                        let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app).unwrap();
                        MenuBuilder::new(app)
                            .item(&show_item)
                            .item(&privacy_item)
                            .item(&quit_item)
                            .build().unwrap()
                    })));
                    info!("Privacy mode: {}", if new_val { "disabled" } else { "enabled" });
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}