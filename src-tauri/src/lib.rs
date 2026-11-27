mod input;
mod permissions;
mod settings;
#[cfg(any(target_os = "windows", test))]
mod windows_pet;

use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use serde::Serialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Monitor, PhysicalPosition,
    PhysicalRect, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent, Wry,
};

static LISTENER_STARTED: AtomicBool = AtomicBool::new(false);

const PET_BASE_WIDTH: f64 = 392.0;
const PET_BASE_HEIGHT: f64 = 392.0;
const PET_MARGIN: f64 = 24.0;

#[derive(Default)]
struct AppState {
    click_through: AtomicBool,
    drag: Mutex<Option<PetDrag>>,
    scale_anchor: Mutex<Option<ScaleAnchor>>,
    canvas: Mutex<Option<CanvasGeometry>>,
    windows_canvas: Mutex<Option<WindowsCanvasView>>,
}

#[derive(Clone, Copy, Serialize)]
struct WindowsCanvasView {
    scale: f64,
    offset: LogicalPosition<f64>,
    revision: u64,
}

#[derive(Clone, Copy)]
struct CanvasGeometry {
    scale: f64,
    // Read by the Windows drag anchoring only; macOS positions come from `scale` alone.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    offset: LogicalPosition<f64>,
}

#[derive(Clone, Copy)]
struct PetDrag {
    offset: LogicalPosition<f64>,
}

impl PetDrag {
    fn target(
        self,
        cursor: LogicalPosition<f64>,
        factor: f64,
    ) -> (PhysicalPosition<i32>, PhysicalPosition<f64>) {
        let cursor = cursor.to_physical::<f64>(factor);
        let position = PhysicalPosition::new(
            (cursor.x - self.offset.x * factor).round() as i32,
            (cursor.y - self.offset.y * factor).round() as i32,
        );
        (position, cursor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ScaleAlignment {
    Start,
    Center,
    End,
}

impl ScaleAlignment {
    fn ratio(self) -> f64 {
        match self {
            Self::Start => 0.0,
            Self::Center => 0.5,
            Self::End => 1.0,
        }
    }
}

#[derive(Clone, Copy)]
struct ScaleAnchor {
    point: PhysicalPosition<f64>,
    alignment: (ScaleAlignment, ScaleAlignment),
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    factor: f64,
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

#[cfg(any(not(target_os = "macos"), test))]
fn resized_anchor(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    new_size: PhysicalSize<u32>,
    area: PhysicalRect<i32, u32>,
    margin: i32,
    factor: f64,
    previous: Option<ScaleAnchor>,
) -> ScaleAnchor {
    use ScaleAlignment::{Center, End, Start};

    let previous = previous.filter(|anchor| {
        anchor.position == position && anchor.size == size && anchor.factor == factor
    });
    let left = area.position.x + margin;
    let right = area.position.x + area.size.width as i32 - margin;
    let top = area.position.y + margin;
    let bottom = area.position.y + area.size.height as i32;
    let alignment = if new_size == size {
        previous.map_or((Center, Center), |anchor| anchor.alignment)
    } else {
        (
            if new_size.width > size.width && (position.x - left).abs() <= 1 {
                Start
            } else if new_size.width > size.width
                && (position.x + size.width as i32 - right).abs() <= 1
            {
                End
            } else {
                Center
            },
            if (position.y + size.height as i32 - bottom).abs() <= 1 {
                End
            } else if new_size.height > size.height && (position.y - top).abs() <= 1 {
                Start
            } else {
                Center
            },
        )
    };
    let mut point = PhysicalPosition::new(
        position.x as f64 + size.width as f64 * alignment.0.ratio(),
        position.y as f64 + size.height as f64 * alignment.1.ratio(),
    );
    if let Some(anchor) = previous {
        // Preserve subpixel precision only while that axis keeps the same anchor policy.
        if anchor.alignment.0 == alignment.0 {
            point.x = anchor.point.x;
        }
        if anchor.alignment.1 == alignment.1 {
            point.y = anchor.point.y;
        }
    }
    let requested = PhysicalPosition::new(
        (point.x - new_size.width as f64 * alignment.0.ratio()).round() as i32,
        (point.y - new_size.height as f64 * alignment.1.ratio()).round() as i32,
    );
    let bounded = clamp_pet_position(requested, new_size, area, margin);
    if bounded.x != requested.x {
        point.x = bounded.x as f64 + new_size.width as f64 * alignment.0.ratio();
    }
    if bounded.y != requested.y {
        point.y = bounded.y as f64 + new_size.height as f64 * alignment.1.ratio();
    }
    ScaleAnchor {
        point,
        alignment,
        position: bounded,
        size: new_size,
        factor,
    }
}

fn clamp_pet_position(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    area: PhysicalRect<i32, u32>,
    margin: i32,
) -> PhysicalPosition<i32> {
    let min_x = area.position.x + margin;
    let min_y = area.position.y + margin;
    let max_x = (area.position.x + area.size.width as i32 - margin - size.width as i32).max(min_x);
    let max_y = (area.position.y + area.size.height as i32 - size.height as i32).max(min_y);
    PhysicalPosition::new(
        position.x.clamp(min_x, max_x),
        position.y.clamp(min_y, max_y),
    )
}

fn monitor_rect(
    monitor: &Monitor,
    factor: f64,
    rect: PhysicalRect<i32, u32>,
) -> PhysicalRect<i32, u32> {
    let ratio = if cfg!(target_os = "macos") {
        factor / monitor.scale_factor()
    } else {
        1.0
    };
    PhysicalRect {
        position: PhysicalPosition::new(
            (rect.position.x as f64 * ratio).round() as i32,
            (rect.position.y as f64 * ratio).round() as i32,
        ),
        size: PhysicalSize::new(
            (rect.size.width as f64 * ratio).round() as u32,
            (rect.size.height as f64 * ratio).round() as u32,
        ),
    }
}

fn pet_work_area(
    window: &WebviewWindow,
    factor: f64,
    monitor_point: PhysicalPosition<f64>,
) -> Result<(PhysicalRect<i32, u32>, i32), String> {
    let monitor = window
        .available_monitors()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|monitor| {
            let rect = monitor_rect(
                monitor,
                factor,
                PhysicalRect {
                    position: *monitor.position(),
                    size: *monitor.size(),
                },
            );
            monitor_point.x >= rect.position.x as f64
                && monitor_point.x < rect.position.x as f64 + rect.size.width as f64
                && monitor_point.y >= rect.position.y as f64
                && monitor_point.y < rect.position.y as f64 + rect.size.height as f64
        });
    let monitor = match monitor {
        Some(monitor) => monitor,
        None => window
            .current_monitor()
            .map_err(|error| error.to_string())?
            .ok_or("no monitor available")?,
    };
    let area = monitor_rect(&monitor, factor, *monitor.work_area());
    let margin_factor = if cfg!(target_os = "macos") {
        factor
    } else {
        monitor.scale_factor()
    };
    Ok((area, (PET_MARGIN * margin_factor).round() as i32))
}

fn constrained_pet_position(
    window: &WebviewWindow,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    factor: f64,
    monitor_point: PhysicalPosition<f64>,
) -> Result<PhysicalPosition<i32>, String> {
    let (area, margin) = pet_work_area(window, factor, monitor_point)?;
    Ok(clamp_pet_position(position, size, area, margin))
}

fn pet_window_size(window: &WebviewWindow) -> tauri::Result<PhysicalSize<u32>> {
    #[cfg(target_os = "windows")]
    {
        windows_pet::visible_size(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        window.outer_size()
    }
}

fn keep_pet_on_screen(window: &WebviewWindow) -> Result<(), String> {
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let size = pet_window_size(window).map_err(|error| error.to_string())?;
    let factor = window.scale_factor().map_err(|error| error.to_string())?;
    let center = PhysicalPosition::new(
        position.x as f64 + size.width as f64 / 2.0,
        position.y as f64 + size.height as f64 / 2.0,
    );
    let bounded = constrained_pet_position(window, position, size, factor, center)?;
    if bounded != position {
        window
            .set_position(bounded)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn uses_native_pet_drag() -> bool {
    cfg!(target_os = "windows")
}

#[tauri::command]
async fn start_pet_drag(window: WebviewWindow, offset: LogicalPosition<f64>) -> Result<(), String> {
    if !offset.x.is_finite() || !offset.y.is_finite() {
        return Err("invalid drag offset".into());
    }
    #[cfg(target_os = "windows")]
    {
        let app = window.app_handle().clone();
        let (sender, mut receiver) = tauri::async_runtime::channel(1);
        app.run_on_main_thread(move || {
            let result = (|| {
                let state = window.state::<AppState>();
                if state.click_through.load(Ordering::Relaxed)
                    || !window.is_visible().map_err(|error| error.to_string())?
                {
                    return Ok(());
                }
                *state.scale_anchor.lock().unwrap() = None;
                if windows_pet::start(&window, offset)? {
                    let scale = state.canvas.lock().unwrap().map(|canvas| canvas.scale);
                    if let Some(scale) = scale {
                        apply_pet_scale_on_main_thread(window.app_handle(), scale)?;
                    }
                    keep_pet_on_screen(&window)?;
                }
                Ok(())
            })();
            let _ = sender.try_send(result);
        })
        .map_err(|error| error.to_string())?;
        receiver.recv().await.ok_or("native drag did not finish")?
    }
    #[cfg(not(target_os = "windows"))]
    {
        let state = window.state::<AppState>();
        // The web view is pinned to the window frame's origin, not to the inset canvas rect,
        // so a CSS point is `scale` window points measured from the frame corner. Adding
        // CanvasGeometry.offset (the sub-point rounding of enclosing_frame) made every click
        // land ~1pt left of the cursor, and absolute positioning accumulated that drift.
        let offset = state.canvas.lock().unwrap().map_or(offset, |canvas| {
            LogicalPosition::new(offset.x * canvas.scale, offset.y * canvas.scale)
        });
        *state.drag.lock().unwrap() = Some(PetDrag { offset });
        *state.scale_anchor.lock().unwrap() = None;
        Ok(())
    }
}

#[tauri::command]
fn drag_pet(window: WebviewWindow, cursor: LogicalPosition<f64>) -> Result<(), String> {
    let drag = *window.state::<AppState>().drag.lock().unwrap();
    let Some(drag) = drag else {
        return Ok(());
    };
    let factor = window.scale_factor().map_err(|error| error.to_string())?;
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let (position, cursor) = drag.target(cursor, factor);
    let position = constrained_pet_position(&window, position, size, factor, cursor)?;
    window
        .set_position(position)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn end_pet_drag(window: WebviewWindow, cursor: Option<LogicalPosition<f64>>) -> Result<(), String> {
    let result = match cursor {
        Some(cursor) => drag_pet(window.clone(), cursor),
        None => Ok(()),
    };
    *window.state::<AppState>().drag.lock().unwrap() = None;
    result
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn resized_canvas(
    rect: PhysicalRect<f64, f64>,
    new_size: PhysicalSize<f64>,
    area: PhysicalRect<i32, u32>,
    margin: i32,
    factor: f64,
    previous: Option<ScaleAnchor>,
) -> (PhysicalRect<f64, f64>, ScaleAnchor) {
    use ScaleAlignment::{Center, End, Start};

    let left = (area.position.x + margin) as f64;
    let right = (area.position.x + area.size.width as i32 - margin) as f64;
    let top = (area.position.y + margin) as f64;
    let bottom = (area.position.y + area.size.height as i32) as f64;
    let same_size = (new_size.width - rect.size.width).abs() < 1e-7
        && (new_size.height - rect.size.height).abs() < 1e-7;
    // The enclosing native window can extend up to one logical point beyond the canvas.
    let tolerance = factor + 1e-7;
    let alignment = if same_size {
        previous.map_or((Center, Center), |anchor| anchor.alignment)
    } else {
        (
            if new_size.width > rect.size.width && (rect.position.x - left).abs() <= tolerance {
                Start
            } else if new_size.width > rect.size.width
                && (rect.position.x + rect.size.width - right).abs() <= tolerance
            {
                End
            } else {
                Center
            },
            if (rect.position.y + rect.size.height - bottom).abs() <= tolerance {
                End
            } else if new_size.height > rect.size.height
                && (rect.position.y - top).abs() <= tolerance
            {
                Start
            } else {
                Center
            },
        )
    };
    let mut point = PhysicalPosition::new(
        rect.position.x + rect.size.width * alignment.0.ratio(),
        rect.position.y + rect.size.height * alignment.1.ratio(),
    );
    if let Some(previous) = previous {
        if previous.alignment.0 == alignment.0 {
            point.x = previous.point.x;
        }
        if previous.alignment.1 == alignment.1 {
            point.y = previous.point.y;
        }
    }
    let requested = PhysicalPosition::new(
        point.x - new_size.width * alignment.0.ratio(),
        point.y - new_size.height * alignment.1.ratio(),
    );
    let position = PhysicalPosition::new(
        requested.x.clamp(left, (right - new_size.width).max(left)),
        requested.y.clamp(top, (bottom - new_size.height).max(top)),
    );
    if position.x != requested.x {
        point.x = position.x + new_size.width * alignment.0.ratio();
    }
    if position.y != requested.y {
        point.y = position.y + new_size.height * alignment.1.ratio();
    }
    (
        PhysicalRect {
            position,
            size: new_size,
        },
        ScaleAnchor {
            point,
            alignment,
            position: position.cast(),
            size: new_size.cast(),
            factor,
        },
    )
}

#[cfg(target_os = "macos")]
fn macos_pet_frame(
    rect: PhysicalRect<f64, f64>,
    factor: f64,
    desktop_height: f64,
) -> objc2_foundation::NSRect {
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let position = rect.position.to_logical::<f64>(factor);
    let size = rect.size.to_logical::<f64>(factor);
    NSRect::new(
        NSPoint::new(position.x, desktop_height - position.y - size.height),
        NSSize::new(size.width, size.height),
    )
}

#[cfg(target_os = "macos")]
fn enclosing_frame(rect: objc2_foundation::NSRect) -> objc2_foundation::NSRect {
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let x = (rect.origin.x + 1e-9).floor();
    let y = (rect.origin.y + 1e-9).floor();
    NSRect::new(
        NSPoint::new(x, y),
        NSSize::new(
            (rect.origin.x + rect.size.width - 1e-9).ceil() - x,
            (rect.origin.y + rect.size.height - 1e-9).ceil() - y,
        ),
    )
}

#[cfg(target_os = "macos")]
fn prepare_pet_canvas(app: &AppHandle) -> tauri::Result<()> {
    use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    app.get_webview_window("main")
        .unwrap()
        .with_webview(|webview| {
            // Wry owns this view; only its native host scales, never its CSS viewport.
            let view = unsafe { &*webview.inner().cast::<NSView>() };
            view.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
            view.setFrame(NSRect::new(
                NSPoint::ZERO,
                NSSize::new(PET_BASE_WIDTH, PET_BASE_HEIGHT),
            ));
            if let Some(host) = unsafe { view.superview() } {
                host.setWantsLayer(true);
            }
        })
}

#[tauri::command]
fn get_pet_canvas_scale(app: AppHandle) -> Option<f64> {
    app.state::<AppState>()
        .canvas
        .lock()
        .unwrap()
        .map(|canvas| canvas.scale)
}

#[tauri::command]
fn get_windows_pet_canvas(app: AppHandle) -> Option<WindowsCanvasView> {
    *app.state::<AppState>().windows_canvas.lock().unwrap()
}

fn apply_pet_scale(app: &AppHandle, scale: f64) -> Result<(), String> {
    let handle = app.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = sender.send(apply_pet_scale_on_main_thread(&handle, scale));
    })
    .map_err(|error| error.to_string())?;
    receiver.recv().map_err(|error| error.to_string())?
}

fn apply_pet_scale_on_main_thread(app: &AppHandle, scale: f64) -> Result<(), String> {
    let scale = settings::normalize_scale(scale);
    let window = app
        .get_webview_window("main")
        .ok_or("pet window not found")?;
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let size = pet_window_size(&window).map_err(|error| error.to_string())?;
    let factor = window.scale_factor().map_err(|error| error.to_string())?;
    let previous = *app.state::<AppState>().scale_anchor.lock().unwrap();
    let center = PhysicalPosition::new(
        position.x as f64 + size.width as f64 / 2.0,
        position.y as f64 + size.height as f64 / 2.0,
    );
    let (area, margin) = pet_work_area(&window, factor, center)?;
    #[cfg(target_os = "macos")]
    let mut anchor = {
        use objc2_app_kit::NSWindow;
        use objc2_core_graphics::{CGDisplayPixelsHigh, CGMainDisplayID};
        use objc2_foundation::{NSPoint, NSRect, NSSize};

        let pointer = window.ns_window().map_err(|error| error.to_string())?;
        // Tauri owns the NSWindow, and the complete resize runs on the main thread.
        let native =
            unsafe { pointer.cast::<NSWindow>().as_ref() }.ok_or("pet window unavailable")?;
        let host = native.contentView().ok_or("pet content view unavailable")?;
        let frame = native.frame();
        let bounds = host.bounds();
        let current_scale = frame.size.width / bounds.size.width;
        let desktop_height = CGDisplayPixelsHigh(CGMainDisplayID()) as f64;
        let rect = PhysicalRect {
            position: PhysicalPosition::new(
                (frame.origin.x - bounds.origin.x * current_scale) * factor,
                (desktop_height - frame.origin.y
                    + (bounds.origin.y - PET_BASE_HEIGHT) * current_scale)
                    * factor,
            ),
            size: LogicalSize::new(
                PET_BASE_WIDTH * current_scale,
                PET_BASE_HEIGHT * current_scale,
            )
            .to_physical(factor),
        };
        let previous = previous.filter(|anchor| {
            anchor.position == position && anchor.size == size && anchor.factor == factor
        });
        let (rect, anchor) = resized_canvas(
            rect,
            LogicalSize::new(PET_BASE_WIDTH * scale, PET_BASE_HEIGHT * scale).to_physical(factor),
            area,
            margin,
            factor,
            previous,
        );
        let canvas = macos_pet_frame(rect, factor, desktop_height);
        let target = enclosing_frame(canvas);
        if native.frame() != target {
            native.setFrame_display(target, false);
        }
        let frame = native.frame();
        host.setBounds(NSRect::new(
            NSPoint::new(
                (frame.origin.x - canvas.origin.x) / scale,
                (frame.origin.y - canvas.origin.y) / scale,
            ),
            NSSize::new(frame.size.width / scale, frame.size.height / scale),
        ));
        native.displayIfNeeded();
        *app.state::<AppState>().canvas.lock().unwrap() = Some(CanvasGeometry {
            scale,
            offset: LogicalPosition::new(
                canvas.origin.x - frame.origin.x,
                frame.origin.y + frame.size.height - canvas.origin.y - canvas.size.height,
            ),
        });
        window
            .emit("pet:canvas-scale", scale)
            .map_err(|error| error.to_string())?;
        anchor
    };
    #[cfg(target_os = "windows")]
    let mut anchor = {
        let state = app.state::<AppState>();
        let canvas = *state.canvas.lock().unwrap();
        let rect = canvas.map_or(
            PhysicalRect {
                position: position.cast(),
                size: size.cast(),
            },
            |canvas| PhysicalRect {
                position: PhysicalPosition::new(
                    position.x as f64 + canvas.offset.x * factor,
                    position.y as f64 + canvas.offset.y * factor,
                ),
                size: LogicalSize::new(
                    PET_BASE_WIDTH * canvas.scale,
                    PET_BASE_HEIGHT * canvas.scale,
                )
                .to_physical(factor),
            },
        );
        let previous = previous.filter(|anchor| {
            anchor.position == position && anchor.size == size && anchor.factor == factor
        });
        let (rect, anchor) = resized_canvas(
            rect,
            LogicalSize::new(PET_BASE_WIDTH * scale, PET_BASE_HEIGHT * scale).to_physical(factor),
            area,
            margin,
            factor,
            previous,
        );
        let frame = windows_pet::enclose_canvas(rect, factor);
        windows_pet::apply_canvas(&window, frame)?;
        *state.canvas.lock().unwrap() = Some(CanvasGeometry {
            scale,
            offset: frame.offset,
        });
        let view = {
            let mut current = state.windows_canvas.lock().unwrap();
            let view = WindowsCanvasView {
                scale,
                offset: frame.offset,
                revision: current.map_or(1, |view| view.revision + 1),
            };
            *current = Some(view);
            view
        };
        window
            .emit("pet:windows-canvas", view)
            .map_err(|error| error.to_string())?;
        anchor
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut anchor = {
        window
            .set_size(LogicalSize::new(
                PET_BASE_WIDTH * scale,
                PET_BASE_HEIGHT * scale,
            ))
            .map_err(|error| error.to_string())?;
        let new_size = window.outer_size().map_err(|error| error.to_string())?;
        let anchor = resized_anchor(position, size, new_size, area, margin, factor, previous);
        window
            .set_position(anchor.position)
            .map_err(|error| error.to_string())?;
        anchor
    };
    anchor.position = window.outer_position().map_err(|error| error.to_string())?;
    anchor.size = pet_window_size(&window).map_err(|error| error.to_string())?;
    let actual_factor = window.scale_factor().map_err(|error| error.to_string())?;
    *app.state::<AppState>().scale_anchor.lock().unwrap() =
        (actual_factor == factor).then_some(anchor);
    Ok(())
}

fn position_pet_at_startup(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(size) = pet_window_size(&window) else {
        return;
    };
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let factor = monitor.scale_factor();
    let margin = (PET_MARGIN * factor).round() as i32;
    let area = monitor.work_area();
    let x = area.position.x + area.size.width as i32 - size.width as i32 - margin;
    let y = area.position.y + area.size.height as i32 - size.height as i32;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

#[tauri::command]
fn preview_scale(app: AppHandle, scale: f64) -> Result<(), String> {
    apply_pet_scale(&app, scale)
}

#[tauri::command]
fn set_scale(app: AppHandle, scale: f64) -> Result<settings::SettingsView, String> {
    let scale = settings::normalize_scale(scale);
    let state = app.state::<SettingsState>();
    let mut guard = state.0.lock().unwrap();
    guard.scale = scale;
    settings::save(&app, &guard)?;
    let view = settings::view(&guard);
    drop(guard);

    apply_pet_scale(&app, scale)?;
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

#[tauri::command]
fn set_skin(app: AppHandle, skin: String) -> Result<settings::SettingsView, String> {
    let state = app.state::<SettingsState>();
    let mut guard = state.0.lock().unwrap();
    guard.skin = skin;
    settings::save(&app, &guard)?;
    let view = settings::view(&guard);
    drop(guard);

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

fn tray_labels(
    language: &str,
    visible: bool,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    match language {
        "zh" => (
            "点击穿透",
            if visible {
                "隐藏桌宠"
            } else {
                "显示桌宠"
            },
            "设置…",
            "角色…",
            "退出",
        ),
        _ => (
            "Click-through",
            if visible { "Hide pet" } else { "Show pet" },
            "Settings…",
            "Characters…",
            "Quit",
        ),
    }
}

fn settings_window_title(language: &str) -> String {
    match language {
        "zh" => "更多",
        _ => "More",
    }
    .to_string()
}

fn build_tray_menu(app: &AppHandle, language: &str) -> tauri::Result<Menu<Wry>> {
    let visible = match app.get_webview_window("main") {
        Some(window) => window.is_visible()?,
        None => false,
    };
    let (click_through, show_hide, settings_label, characters_label, quit) =
        tray_labels(language, visible);
    let checked = app
        .state::<AppState>()
        .click_through
        .load(Ordering::Relaxed);

    let click_through_item = CheckMenuItem::with_id(
        app,
        "click_through",
        click_through,
        true,
        checked,
        None::<&str>,
    )?;
    let show_hide_item = MenuItem::with_id(app, "show_hide", show_hide, true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", settings_label, true, None::<&str>)?;
    let characters_item =
        MenuItem::with_id(app, "characters", characters_label, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", quit, true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &click_through_item,
            &show_hide_item,
            &settings_item,
            &characters_item,
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

fn log_file_path(app: &AppHandle) -> tauri::Result<std::path::PathBuf> {
    Ok(app.path().app_log_dir()?.join("frank-tappo.log"))
}

fn append_log_line(app: &AppHandle, line: &str) {
    let Ok(path) = log_file_path(app) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{line}");
    }
}

#[tauri::command]
fn reveal_log_file(app: AppHandle) -> Result<(), String> {
    let path = log_file_path(&app).map_err(|error| error.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    if !path.exists() {
        std::fs::File::create(&path).map_err(|error| error.to_string())?;
    }
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .status()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn open_author_email() -> Result<(), String> {
    let url = "mailto:Frankzhen2025@outlook.com";
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    command
        .arg(url)
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn build_settings_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let language = resolved_language(app);
    let window =
        WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
            .title(settings_window_title(language))
            .inner_size(880.0, 640.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
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

fn open_settings_window(app: &AppHandle, page: Option<&str>) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(page) = page {
            let _ = window.emit("settings:navigate", page);
        }
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

#[tauri::command]
async fn focus_pet(app: AppHandle) -> Result<(), String> {
    if app
        .state::<AppState>()
        .click_through
        .load(Ordering::Relaxed)
    {
        return Ok(());
    }
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().map_err(|error| error.to_string())?
            && !window.is_focused().map_err(|error| error.to_string())?
        {
            window.set_focus().map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn toggle_settings_window(app: AppHandle) -> Result<(), String> {
    let window = match app.get_webview_window("settings") {
        Some(window) => window,
        None => build_settings_window(&app).map_err(|error| error.to_string())?,
    };
    let visible = window.is_visible().map_err(|error| error.to_string())?;
    let minimized = window.is_minimized().map_err(|error| error.to_string())?;
    if visible && !minimized {
        window.hide().map_err(|error| error.to_string())
    } else {
        window.unminimize().map_err(|error| error.to_string())?;
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())
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
            append_log_line(
                app.handle(),
                &format!("Frank Tappo {} — session start", env!("CARGO_PKG_VERSION")),
            );
            log_permission_status(&permission_status());

            #[cfg(target_os = "macos")]
            if !permissions::listen_access() {
                permissions::request_listen_access();
            }

            input::init_counter(app.handle())?;
            let settings = settings::load(app.handle());
            let initial_scale = settings.scale;
            app.manage(SettingsState(Mutex::new(settings)));

            let language = resolved_language(app.handle());
            let menu = build_tray_menu(app.handle(), language)?;
            TrayIconBuilder::with_id("main-tray")
                .icon(tauri::include_image!("icons/tray-original.png"))
                .icon_as_template(false)
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
                            let result = window.is_visible().and_then(|visible| {
                                if visible {
                                    window.hide()
                                } else {
                                    window.show()
                                }
                            });
                            match result {
                                Ok(()) => refresh_tray(app),
                                Err(error) => {
                                    eprintln!("[window] failed to toggle pet visibility: {error}")
                                }
                            }
                        }
                    }
                    "settings" => open_settings_window(app, Some("general")),
                    "characters" => open_settings_window(app, Some("characters")),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            #[cfg(target_os = "macos")]
            prepare_pet_canvas(app.handle())?;
            apply_pet_scale(app.handle(), initial_scale)?;
            position_pet_at_startup(app.handle());
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "windows")]
                windows_pet::install(&window)?;
                let handle = window.clone();
                window.on_window_event(move |event| {
                    #[cfg(target_os = "windows")]
                    if windows_pet::is_active() {
                        return;
                    }
                    #[cfg(target_os = "windows")]
                    if matches!(event, WindowEvent::ScaleFactorChanged { .. }) {
                        let scale = handle
                            .state::<AppState>()
                            .canvas
                            .lock()
                            .unwrap()
                            .map(|canvas| canvas.scale);
                        if let Some(scale) = scale {
                            if let Err(error) =
                                apply_pet_scale_on_main_thread(handle.app_handle(), scale)
                            {
                                eprintln!("[window] failed to update pet DPI: {error}");
                            }
                        }
                    }
                    if matches!(
                        event,
                        WindowEvent::Moved(_)
                            | WindowEvent::Resized(_)
                            | WindowEvent::ScaleFactorChanged { .. }
                    ) {
                        if let Err(error) = keep_pet_on_screen(&handle) {
                            eprintln!("[window] failed to constrain pet: {error}");
                        }
                    }
                });
            }

            if let Err(error) = build_settings_window(app.handle()) {
                eprintln!("[settings] failed to preload window: {error}");
            }

            input::spawn_gamepad_listener(app.handle().clone());
            ensure_input_listener(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_permissions,
            input::get_input_count,
            request_input_monitoring,
            report_latency,
            get_settings,
            get_pet_canvas_scale,
            get_windows_pet_canvas,
            set_language,
            set_skin,
            preview_scale,
            set_scale,
            uses_native_pet_drag,
            start_pet_drag,
            drag_pet,
            end_pet_drag,
            focus_pet,
            toggle_settings_window,
            reveal_log_file,
            open_author_email,
            quit_app
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                input::shutdown_counter(app);
            }
        });
}

#[cfg(test)]
mod tray_tests {
    use super::{settings_window_title, tray_labels};

    #[test]
    fn visibility_item_describes_the_next_action_in_each_language() {
        for (language, visible, expected) in [
            ("zh", true, ("点击穿透", "隐藏桌宠", "设置…", "角色…", "退出")),
            ("zh", false, ("点击穿透", "显示桌宠", "设置…", "角色…", "退出")),
            (
                "en",
                true,
                ("Click-through", "Hide pet", "Settings…", "Characters…", "Quit"),
            ),
            (
                "en",
                false,
                ("Click-through", "Show pet", "Settings…", "Characters…", "Quit"),
            ),
        ] {
            assert_eq!(tray_labels(language, visible), expected);
        }
    }

    #[test]
    fn more_window_title_follows_language() {
        assert_eq!(settings_window_title("zh"), "更多");
        assert_eq!(settings_window_title("en"), "More");
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    #[test]
    fn native_move_rectangles_are_constrained_without_accumulating_delta() {
        for factor in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0] {
            for origin in [(0, 0), (3840, 240), (-2560, -1440)] {
                let area = PhysicalRect {
                    position: PhysicalPosition::from(origin),
                    size: PhysicalSize::new(2560, 1440),
                };
                let margin = (PET_MARGIN * factor).round() as i32;
                for scale in [0.24, 0.4, 0.6] {
                    let size = LogicalSize::new(PET_BASE_WIDTH * scale, PET_BASE_HEIGHT * scale)
                        .to_physical::<u32>(factor);
                    let min = PhysicalPosition::new(origin.0 + margin, origin.1 + margin);
                    let max = PhysicalPosition::new(
                        origin.0 + 2560 - margin - size.width as i32,
                        origin.1 + 1440 - size.height as i32,
                    );
                    for step in 0..1000 {
                        let target = if step % 2 == 0 {
                            PhysicalPosition::new(origin.0 - 10000, origin.1 - 10000)
                        } else {
                            PhysicalPosition::new(origin.0 + 10000, origin.1 + 10000)
                        };
                        assert_eq!(
                            clamp_pet_position(target, size, area, margin),
                            if step % 2 == 0 { min } else { max }
                        );
                    }
                    for target in [
                        min,
                        max,
                        PhysicalPosition::new(origin.0 + 600, origin.1 + 400),
                    ] {
                        assert_eq!(clamp_pet_position(target, size, area, margin), target);
                    }
                }
            }
        }
    }

    #[test]
    fn macos_drag_maps_client_points_to_the_frame_origin() {
        // The web view is anchored to the window frame, so the grab distance is client * scale
        // with no canvas inset: a click that never moves the cursor must reproduce the position.
        for factor in [1.0, 2.0] {
            for scale in [0.3, 0.4, 0.8] {
                for (client_x, client_y) in [(100.25, 130.75), (0.0, 391.0), (346.4, 46.2)] {
                    let offset = LogicalPosition::new(client_x * scale, client_y * scale);
                    let origin = LogicalPosition::new(900.4, 500.6);
                    let cursor = LogicalPosition::new(origin.x + offset.x, origin.y + offset.y);
                    let (position, _) = PetDrag { offset }.target(cursor, factor);
                    assert_eq!(
                        position,
                        PhysicalPosition::new(
                            (origin.x * factor).round() as i32,
                            (origin.y * factor).round() as i32,
                        )
                    );
                }
            }
        }
    }

    #[test]
    fn scaling_keeps_a_fixed_fractional_center() {
        let area = PhysicalRect {
            position: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(1440, 900),
        };
        let mut position = PhysicalPosition::new(500, 350);
        let mut size = PhysicalSize::new(201, 201);
        let center = PhysicalPosition::new(600.5, 450.5);
        let mut previous = None;
        for _ in 0..10 {
            for percent in (20..=60).chain((20..=60).rev()) {
                let side = (PET_BASE_WIDTH * percent as f64 / 100.0).round() as u32;
                let anchor = resized_anchor(
                    position,
                    size,
                    PhysicalSize::new(side, side),
                    area,
                    24,
                    1.0,
                    previous,
                );
                position = anchor.position;
                size = anchor.size;
                previous = Some(anchor);
                assert!((position.x as f64 + side as f64 / 2.0 - center.x).abs() <= 0.5);
                assert!((position.y as f64 + side as f64 / 2.0 - center.y).abs() <= 0.5);
            }
        }
    }

    #[test]
    fn native_position_quantization_and_same_size_saves_do_not_accumulate_drift() {
        for factor in [1.0, 2.0] {
            let area = PhysicalRect {
                position: PhysicalPosition::new(0, 0),
                size: LogicalSize::new(1440.0, 900.0).to_physical(factor),
            };
            let mut position = LogicalPosition::new(500.0, 350.0).to_physical(factor);
            let mut size = LogicalSize::new(157.0, 157.0).to_physical(factor);
            let center = PhysicalPosition::new(578.5 * factor, 428.5 * factor);
            let mut previous = None;
            for _ in 0..20 {
                for tenth in (200..=600).chain((200..=600).rev()) {
                    let side = (PET_BASE_WIDTH * tenth as f64 / 1000.0).ceil();
                    for save in [false, true] {
                        let mut anchor = resized_anchor(
                            position,
                            size,
                            LogicalSize::new(side, side).to_physical(factor),
                            area,
                            (24.0 * factor) as i32,
                            factor,
                            previous,
                        );
                        let logical = anchor.position.to_logical::<f64>(factor);
                        anchor.position =
                            LogicalPosition::new(logical.x.round(), logical.y.round())
                                .to_physical(factor);
                        if save {
                            assert_eq!(anchor.position, position);
                        }
                        position = anchor.position;
                        size = anchor.size;
                        previous = Some(anchor);
                        assert!(
                            (position.x as f64 + size.width as f64 / 2.0 - center.x).abs()
                                <= factor / 2.0
                        );
                        assert!(
                            (position.y as f64 + size.height as f64 / 2.0 - center.y).abs()
                                <= factor / 2.0
                        );
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_frame_conversion_preserves_size_and_signed_monitor_coordinates() {
        use objc2_foundation::{NSPoint, NSRect, NSSize};

        for (position, factor, expected) in [
            ((200, 400), 1.0, (200.0, 182.0, 400.0)),
            ((200, 400), 2.0, (100.0, 582.0, 200.0)),
            ((-2800, -1600), 2.0, (-1400.0, 1582.0, 200.0)),
            ((-2800, 2400), 2.0, (-1400.0, -418.0, 200.0)),
            ((201, 401), 2.0, (100.5, 581.5, 200.0)),
        ] {
            assert_eq!(
                macos_pet_frame(
                    PhysicalRect {
                        position: PhysicalPosition::<i32>::from(position).cast(),
                        size: PhysicalSize::new(400.0, 400.0),
                    },
                    factor,
                    982.0
                ),
                NSRect::new(
                    NSPoint::new(expected.0, expected.1),
                    NSSize::new(expected.2, expected.2)
                ),
            );
        }
    }

    #[test]
    fn fractional_canvas_scaling_has_no_center_wobble() {
        for factor in [1.0, 2.0] {
            let area = PhysicalRect {
                position: PhysicalPosition::new(-2000, -1000),
                size: PhysicalSize::new(6000, 4000),
            };
            let mut rect = PhysicalRect {
                position: PhysicalPosition::new(500.3 * factor, 350.7 * factor),
                size: PhysicalSize::new(156.8 * factor, 156.8 * factor),
            };
            let center = PhysicalPosition::new(
                rect.position.x + rect.size.width / 2.0,
                rect.position.y + rect.size.height / 2.0,
            );
            let mut previous = None;
            for _ in 0..10 {
                for tenth in (200..=600).chain((200..=600).rev()) {
                    let side = PET_BASE_WIDTH * tenth as f64 / 1000.0 * factor;
                    let (next, anchor) = resized_canvas(
                        rect,
                        PhysicalSize::new(side, side),
                        area,
                        24,
                        factor,
                        previous,
                    );
                    assert!((next.position.x + side / 2.0 - center.x).abs() < 1e-9);
                    assert!((next.position.y + side / 2.0 - center.y).abs() < 1e-9);
                    let (saved, _) =
                        resized_canvas(next, next.size, area, 24, factor, Some(anchor));
                    assert_eq!(saved.position, next.position);
                    assert_eq!(saved.size, next.size);
                    rect = next;
                    previous = Some(anchor);
                }
            }
        }
    }

    #[test]
    fn fractional_canvas_edges_keep_directional_rules() {
        for factor in [1.0, 2.0] {
            for origin in [(0, 0), (-1920, -300)] {
                let area = PhysicalRect {
                    position: PhysicalPosition::from(origin),
                    size: PhysicalSize::new(1440, 900),
                };
                let margin = (24.0 * factor) as i32;
                let left = (area.position.x + margin) as f64;
                let right = (area.position.x + area.size.width as i32 - margin) as f64;
                let top = (area.position.y + margin) as f64;
                let bottom = (area.position.y + area.size.height as i32) as f64;
                for x in [left, left + 400.0, right - 78.4 * factor] {
                    for y in [top, top + 300.0, bottom - 78.4 * factor] {
                        let rect = PhysicalRect {
                            position: PhysicalPosition::new(x, y),
                            size: PhysicalSize::new(78.4 * factor, 78.4 * factor),
                        };
                        let (grown, anchor) = resized_canvas(
                            rect,
                            PhysicalSize::new(235.2 * factor, 235.2 * factor),
                            area,
                            margin,
                            factor,
                            None,
                        );
                        if (x - left).abs() < 1e-7 {
                            assert!((grown.position.x - left).abs() < 1e-7);
                        }
                        if (x + rect.size.width - right).abs() < 1e-7 {
                            assert!((grown.position.x + grown.size.width - right).abs() < 1e-7);
                        }
                        if (y - top).abs() < 1e-7 {
                            assert!((grown.position.y - top).abs() < 1e-7);
                        }
                        let (shrunk, _) =
                            resized_canvas(grown, rect.size, area, margin, factor, Some(anchor));
                        assert!(
                            (shrunk.position.x + shrunk.size.width / 2.0
                                - grown.position.x
                                - grown.size.width / 2.0)
                                .abs()
                                < 1e-7
                        );
                        if (y + rect.size.height - bottom).abs() < 1e-7 {
                            assert!((shrunk.position.y + shrunk.size.height - bottom).abs() < 1e-7);
                        } else {
                            assert!(
                                (shrunk.position.y + shrunk.size.height / 2.0
                                    - grown.position.y
                                    - grown.size.height / 2.0)
                                    .abs()
                                    < 1e-7
                            );
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn enclosing_native_frame_does_not_quantize_the_canvas() {
        use objc2_foundation::{NSPoint, NSRect, NSSize};
        for x in [-1400.81, -0.01, 0.0, 400.13] {
            for tenth in 200..=600 {
                let scale = tenth as f64 / 1000.0;
                let canvas = NSRect::new(
                    NSPoint::new(x, x - 300.67),
                    NSSize::new(392.0 * scale, 392.0 * scale),
                );
                let frame = enclosing_frame(canvas);
                assert!(frame.origin.x <= canvas.origin.x + 1e-8);
                assert!(frame.origin.y <= canvas.origin.y + 1e-8);
                assert!(
                    frame.origin.x + frame.size.width >= canvas.origin.x + canvas.size.width - 1e-8
                );
                assert!(
                    frame.origin.y + frame.size.height
                        >= canvas.origin.y + canvas.size.height - 1e-8
                );
                let bounds_x = (frame.origin.x - canvas.origin.x) / scale;
                assert!((frame.origin.x - bounds_x * scale - x).abs() < 1e-9);
                assert_eq!(frame.size.width.fract(), 0.0);
                assert_eq!(frame.size.height.fract(), 0.0);
            }
        }
    }

    #[test]
    fn edge_transitions_preserve_the_other_axis_fractional_anchor() {
        let area = PhysicalRect {
            position: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(1000, 800),
        };
        for (initial, expected) in [((24, 350), (125, 351)), ((350, 699), (351, 701))] {
            let grown = resized_anchor(
                PhysicalPosition::from(initial),
                PhysicalSize::new(101, 101),
                PhysicalSize::new(300, 300),
                area,
                24,
                1.0,
                None,
            );
            let shrunk = resized_anchor(
                grown.position,
                grown.size,
                PhysicalSize::new(99, 99),
                area,
                24,
                1.0,
                Some(grown),
            );
            assert_eq!(shrunk.position, PhysicalPosition::from(expected));
        }
    }

    #[test]
    fn screen_edges_reserve_top_and_side_margins_only() {
        let area = PhysicalRect {
            position: PhysicalPosition::new(0, 40),
            size: PhysicalSize::new(1440, 860),
        };
        let size = PhysicalSize::new(200, 200);
        for (requested, expected) in [
            ((-100, -100), (24, 64)),
            ((2000, -100), (1216, 64)),
            ((-100, 2000), (24, 700)),
            ((2000, 2000), (1216, 700)),
            ((600, 400), (600, 400)),
        ] {
            assert_eq!(
                clamp_pet_position(PhysicalPosition::from(requested), size, area, 24),
                PhysicalPosition::from(expected),
            );
        }
    }

    #[test]
    fn screen_bounds_support_negative_monitor_origins() {
        let area = PhysicalRect {
            position: PhysicalPosition::new(-1920, -300),
            size: PhysicalSize::new(1920, 1200),
        };
        let size = PhysicalSize::new(200, 200);
        assert_eq!(
            clamp_pet_position(PhysicalPosition::new(-3000, -1000), size, area, 24),
            PhysicalPosition::new(-1896, -276),
        );
        assert_eq!(
            clamp_pet_position(PhysicalPosition::new(3000, 1000), size, area, 24),
            PhysicalPosition::new(-224, 700),
        );
    }

    #[test]
    fn margins_are_logical_pixels_at_each_display_scale() {
        for factor in [1.0, 1.25, 1.5, 2.0] {
            let area = PhysicalRect {
                position: PhysicalPosition::new(0, 0),
                size: LogicalSize::new(1440.0, 900.0).to_physical(factor),
            };
            let size = LogicalSize::new(200.0, 200.0).to_physical(factor);
            let margin = (PET_MARGIN * factor).round() as i32;
            let top_left =
                clamp_pet_position(PhysicalPosition::new(-1000, -1000), size, area, margin);
            let bottom_right =
                clamp_pet_position(PhysicalPosition::new(10000, 10000), size, area, margin);
            assert_eq!(top_left.x, margin);
            assert_eq!(top_left.y, margin);
            assert_eq!(
                area.size.width as i32 - bottom_right.x - size.width as i32,
                margin
            );
            assert_eq!(
                area.size.height as i32 - bottom_right.y - size.height as i32,
                0
            );
        }
    }

    #[test]
    fn edges_and_corners_follow_directional_anchors() {
        for factor in [1.0, 1.25, 1.5, 2.0] {
            for (origin_x, origin_y) in [(0.0, 0.0), (-1920.0, -300.0)] {
                let area = PhysicalRect {
                    position: LogicalPosition::new(origin_x, origin_y).to_physical(factor),
                    size: LogicalSize::new(1000.0, 800.0).to_physical(factor),
                };
                let margin = (24.0 * factor).round() as i32;
                let physical = |(x, y): (i32, i32)| {
                    LogicalPosition::new(origin_x + x as f64, origin_y + y as f64)
                        .to_physical::<i32>(factor)
                };
                for (initial, grown, shrunk) in [
                    ((24, 352), (24, 252), (124, 352)),
                    ((876, 352), (676, 252), (776, 352)),
                    ((452, 24), (352, 24), (452, 124)),
                    ((452, 700), (352, 500), (452, 700)),
                    ((24, 24), (24, 24), (124, 124)),
                    ((876, 24), (676, 24), (776, 124)),
                    ((24, 700), (24, 500), (124, 700)),
                    ((876, 700), (676, 500), (776, 700)),
                ] {
                    let mut position = physical(initial);
                    let mut size = LogicalSize::new(100.0, 100.0).to_physical(factor);
                    let mut previous = None;
                    for _ in 0..20 {
                        for (side, expected) in [
                            (300.0, grown),
                            (300.0, grown),
                            (100.0, shrunk),
                            (100.0, shrunk),
                        ] {
                            let anchor = resized_anchor(
                                position,
                                size,
                                LogicalSize::new(side, side).to_physical(factor),
                                area,
                                margin,
                                factor,
                                previous,
                            );
                            assert_eq!(
                                anchor.position,
                                physical(expected),
                                "edge {initial:?}, size {side}, factor {factor}"
                            );
                            assert_eq!(
                                clamp_pet_position(anchor.position, anchor.size, area, margin),
                                anchor.position
                            );
                            position = anchor.position;
                            size = anchor.size;
                            previous = Some(anchor);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn clipping_rebases_the_center_but_bottom_stays_attached() {
        let area = PhysicalRect {
            position: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(1000, 800),
        };
        for (initial, expected) in [((30, 30), (124, 124)), ((450, 690), (450, 700))] {
            let grown = resized_anchor(
                PhysicalPosition::from(initial),
                PhysicalSize::new(100, 100),
                PhysicalSize::new(300, 300),
                area,
                24,
                1.0,
                None,
            );
            let shrunk = resized_anchor(
                grown.position,
                grown.size,
                PhysicalSize::new(100, 100),
                area,
                24,
                1.0,
                Some(grown),
            );
            assert_eq!(shrunk.position, PhysicalPosition::from(expected));
        }
    }

    #[test]
    fn moving_or_changing_dpi_invalidates_the_cached_anchor() {
        let area = PhysicalRect {
            position: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(1000, 800),
        };
        let previous = resized_anchor(
            PhysicalPosition::new(400, 300),
            PhysicalSize::new(100, 100),
            PhysicalSize::new(200, 200),
            area,
            24,
            1.0,
            None,
        );
        for (position, factor) in [
            (PhysicalPosition::new(500, 400), 1.0),
            (previous.position, 2.0),
        ] {
            let mut cached = previous;
            cached.point = PhysicalPosition::new(-5000.0, -5000.0);
            let anchor = resized_anchor(
                position,
                previous.size,
                PhysicalSize::new(100, 100),
                area,
                24,
                factor,
                Some(cached),
            );
            assert_eq!(
                anchor.position,
                PhysicalPosition::new(position.x + 50, position.y + 50)
            );
        }
    }

    #[test]
    fn undersized_work_area_does_not_invert_clamp_bounds() {
        let area = PhysicalRect {
            position: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(100, 100),
        };
        assert_eq!(
            clamp_pet_position(
                PhysicalPosition::new(-100, 1000),
                PhysicalSize::new(200, 200),
                area,
                24
            ),
            PhysicalPosition::new(24, 24),
        );
    }
}
