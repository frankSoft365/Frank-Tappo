mod input;
mod permissions;
mod settings;

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent, Wry,
};

static LISTENER_STARTED: AtomicBool = AtomicBool::new(false);

const PET_BASE_WIDTH: f64 = 300.0;
const PET_BASE_HEIGHT: f64 = 360.0;
const PET_MARGIN: f64 = 24.0;

#[derive(Default)]
struct AppState {
    click_through: AtomicBool,
}

struct SettingsState(Mutex<settings::Settings>);

#[derive(Default)]
struct LatencySamples(Mutex<Vec<u64>>);

#[derive(Serialize)]
struct PermissionStatus {
    accessibility: bool,
    input_monitoring: bool,
}

fn permission_status() -> PermissionStatus {
    PermissionStatus {
        accessibility: permissions::is_trusted(),
        input_monitoring: permissions::listen_access(),
    }
}

fn log_permission_status(status: &PermissionStatus) {
    static LAST: AtomicU8 = AtomicU8::new(255);
    let code = status.accessibility as u8 | ((status.input_monitoring as u8) << 1);
    if LAST.swap(code, Ordering::Relaxed) != code {
        eprintln!(
            "[permissions] accessibility={} input_monitoring={}",
            status.accessibility, status.input_monitoring
        );
    }
}

fn ensure_input_listener(app: &AppHandle) {
    if !permissions::input_ready() || LISTENER_STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!("[input] starting global listener");
    input::spawn_listener(app.clone());
}

#[tauri::command]
fn check_permissions(app: AppHandle) -> PermissionStatus {
    let status = permission_status();
    log_permission_status(&status);
    ensure_input_listener(&app);
    status
}

#[tauri::command]
fn request_input_monitoring(app: AppHandle) -> bool {
    let granted = permissions::request_listen_access();
    if granted {
        ensure_input_listener(&app);
    } else {
        permissions::open_input_monitoring_settings();
    }
    log_permission_status(&permission_status());
    granted
}

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
fn get_settings(app: AppHandle) -> settings::SettingsView {
    let state = app.state::<SettingsState>();
    let guard = state.0.lock().unwrap();
    settings::view(&guard)
}

fn apply_pet_scale(app: &AppHandle, scale: f64) {
    let scale = scale.clamp(settings::SCALE_MIN, settings::SCALE_MAX);
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.set_size(LogicalSize::new(
        PET_BASE_WIDTH * scale,
        PET_BASE_HEIGHT * scale,
    ));
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let factor = monitor.scale_factor();
    let width = (PET_BASE_WIDTH * scale * factor).round() as i32;
    let height = (PET_BASE_HEIGHT * scale * factor).round() as i32;
    let margin = (PET_MARGIN * factor).round() as i32;
    let area = monitor.work_area();
    let x = area.position.x + area.size.width as i32 - width - margin;
    let y = area.position.y + area.size.height as i32 - height - margin;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

#[tauri::command]
fn preview_scale(app: AppHandle, scale: f64) {
    apply_pet_scale(&app, scale);
}

#[tauri::command]
fn set_scale(app: AppHandle, scale: f64) -> Result<settings::SettingsView, String> {
    let scale = scale.clamp(settings::SCALE_MIN, settings::SCALE_MAX);
    let state = app.state::<SettingsState>();
    let mut guard = state.0.lock().unwrap();
    guard.scale = scale;
    settings::save(&app, &guard)?;
    let view = settings::view(&guard);
    drop(guard);

    apply_pet_scale(&app, scale);
    let _ = app.emit("settings:changed", view.clone());
    Ok(view)
}

#[tauri::command]
fn set_language(app: AppHandle, language: String) -> Result<settings::SettingsView, String> {
    if !matches!(language.as_str(), "system" | "zh" | "en") {
        return Err(format!("unsupported language: {language}"));
    }
    let state = app.state::<SettingsState>();
    let mut guard = state.0.lock().unwrap();
    guard.language = language;
    settings::save(&app, &guard)?;
    let view = settings::view(&guard);
    drop(guard);

    refresh_tray(&app);
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.set_title(&settings_window_title(&view.resolved));
    }
    let _ = app.emit("settings:changed", view.clone());
    Ok(view)
}

fn set_click_through_enabled(app: &AppHandle, enabled: bool) {
    let state = app.state::<AppState>();
    state.click_through.store(enabled, Ordering::Relaxed);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_ignore_cursor_events(enabled);
    }
    let _ = app.emit("ui:click-through", enabled);
    refresh_tray(app);
}

fn resolved_language(app: &AppHandle) -> &'static str {
    let state = app.state::<SettingsState>();
    let guard = state.0.lock().unwrap();
    settings::resolve(&guard.language)
}

fn tray_labels(language: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match language {
        "zh" => ("点击穿透", "显示/隐藏", "设置…", "退出"),
        _ => ("Click-through", "Show/Hide", "Settings…", "Quit"),
    }
}

fn settings_window_title(language: &str) -> String {
    match language {
        "zh" => "设置",
        _ => "Settings",
    }
    .to_string()
}

fn build_tray_menu(app: &AppHandle, language: &str) -> tauri::Result<Menu<Wry>> {
    let (click_through, show_hide, settings_label, quit) = tray_labels(language);
    let checked = app.state::<AppState>().click_through.load(Ordering::Relaxed);

    let click_through_item =
        CheckMenuItem::with_id(app, "click_through", click_through, true, checked, None::<&str>)?;
    let show_hide_item =
        MenuItem::with_id(app, "show_hide", show_hide, true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", settings_label, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", quit, true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &click_through_item,
            &show_hide_item,
            &settings_item,
            &separator,
            &quit_item,
        ],
    )
}

fn refresh_tray(app: &AppHandle) {
    let language = resolved_language(app);
    if let Some(tray) = app.tray_by_id("main-tray") {
        if let Ok(menu) = build_tray_menu(app, language) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

fn build_settings_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let language = resolved_language(app);
    let window = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title(settings_window_title(language))
        .inner_size(420.0, 520.0)
        .resizable(false)
        .visible(false)
        .center()
        .build()?;
    let handle = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = handle.hide();
        }
    });
    Ok(window)
}

fn open_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    // Building a webview inside the tray menu callback blocks the first click,
    // so the window is normally pre-created at startup; this is a fallback.
    match build_settings_window(app) {
        Ok(window) => {
            let _ = window.show();
            let _ = window.set_focus();
        }
        Err(error) => eprintln!("[settings] failed to open window: {error}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .manage(LatencySamples::default())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            eprintln!("[app] exe={:?}", std::env::current_exe().ok());
            log_permission_status(&permission_status());

            #[cfg(target_os = "macos")]
            if !permissions::listen_access() {
                permissions::request_listen_access();
            }

            let settings = settings::load(app.handle());
            let initial_scale = settings.scale;
            app.manage(SettingsState(Mutex::new(settings)));

            let language = resolved_language(app.handle());
            let menu = build_tray_menu(app.handle(), language)?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "click_through" => {
                        let enabled = !app
                            .state::<AppState>()
                            .click_through
                            .load(Ordering::Relaxed);
                        set_click_through_enabled(app, enabled);
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
                    "settings" => open_settings_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            apply_pet_scale(app.handle(), initial_scale);

            if let Err(error) = build_settings_window(app.handle()) {
                eprintln!("[settings] failed to preload window: {error}");
            }

            ensure_input_listener(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_permissions,
            request_input_monitoring,
            report_latency,
            get_settings,
            set_language,
            preview_scale,
            set_scale
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
