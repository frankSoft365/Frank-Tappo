mod input;
mod permissions;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, PhysicalPosition};

#[derive(Default)]
struct AppState {
    click_through: AtomicBool,
}

#[derive(Default)]
struct LatencySamples(Mutex<Vec<u64>>);

#[tauri::command]
fn report_latency(ms: u64, samples: tauri::State<LatencySamples>) {
    let mut values = samples.0.lock().unwrap();
    values.push(ms);
    if values.len() % 25 == 0 {
        let mut sorted = values.clone();
        sorted.sort_unstable();
        eprintln!(
            "[latency] samples={} min={}ms p50={}ms p95={}ms max={}ms",
            sorted.len(),
            sorted[0],
            sorted[sorted.len() / 2],
            sorted[sorted.len() * 95 / 100],
            sorted[sorted.len() - 1]
        );
    }
}

#[tauri::command]
fn check_accessibility() -> bool {
    permissions::is_trusted()
}

#[tauri::command]
fn open_accessibility_settings() {
    permissions::open_settings();
}

fn set_click_through_enabled(app: &tauri::AppHandle, enabled: bool) {
    let state = app.state::<AppState>();
    state.click_through.store(enabled, Ordering::Relaxed);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_ignore_cursor_events(enabled);
    }
    let _ = app.emit("ui:click-through", enabled);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .manage(LatencySamples::default())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            #[cfg(target_os = "macos")]
            eprintln!(
                "[permissions] accessibility trusted: {}",
                permissions::is_trusted()
            );

            let click_through_item = CheckMenuItem::with_id(
                app,
                "click_through",
                "点击穿透 Click-through",
                true,
                false,
                None::<&str>,
            )?;
            let show_hide_item =
                MenuItem::with_id(app, "show_hide", "显示/隐藏 Show/Hide", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 Quit", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&click_through_item, &show_hide_item, &separator, &quit_item],
            )?;

            let click_through_for_menu = click_through_item.clone();
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "click_through" => {
                        let enabled = !app.state::<AppState>().click_through.load(Ordering::Relaxed);
                        set_click_through_enabled(app, enabled);
                        let _ = click_through_for_menu.set_checked(enabled);
                    }
                    "show_hide" => {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(true) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                            }
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                if let Ok(Some(monitor)) = window.primary_monitor() {
                    let margin = (24.0 * monitor.scale_factor()) as i32;
                    if let Ok(size) = window.outer_size() {
                        let x = monitor.size().width as i32 - size.width as i32 - margin;
                        let y = monitor.size().height as i32 - size.height as i32 - margin;
                        let _ = window.set_position(PhysicalPosition::new(x, y));
                    }
                }
            }

            input::spawn_listener(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_accessibility,
            open_accessibility_settings,
            report_latency
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
