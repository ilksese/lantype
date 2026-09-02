use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

pub fn setup_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show_item = MenuItemBuilder::with_id("show", "显示窗口").build(app)?;
    let pause_item = MenuItemBuilder::with_id("toggle-pause", "暂停/恢复接收").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .item(&pause_item)
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
            "toggle-pause" => {
                if let Some(state) = app.try_state::<crate::AppState>() {
                    let server = state.ws_server.clone();
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let enabled = !server.is_paused();
                        server.set_input_paused(enabled).await;
                        server.emit_state(&app).await;
                    });
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
