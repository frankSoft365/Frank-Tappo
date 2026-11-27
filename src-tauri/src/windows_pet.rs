use tauri::{LogicalPosition, PhysicalPosition, PhysicalRect, PhysicalSize};

use super::clamp_pet_position;

#[cfg(target_os = "windows")]
pub use native::{apply_canvas, install, is_active, set_visible_position, start, visible_position, visible_size};

fn clip_origin(size: PhysicalSize<u32>, host: PhysicalSize<u32>) -> PhysicalPosition<i32> {
    PhysicalPosition::new(((host.width - size.width) / 2) as i32, ((host.height - size.height) / 2) as i32)
}

#[derive(Clone, Copy)]
pub struct CanvasFrame {
    pub position: PhysicalPosition<i32>,
    pub size: PhysicalSize<u32>,
    pub offset: LogicalPosition<f64>,
}

pub fn enclose_canvas(rect: PhysicalRect<f64, f64>, factor: f64) -> CanvasFrame {
    let x = (rect.position.x + 1e-9).floor();
    let y = (rect.position.y + 1e-9).floor();
    CanvasFrame {
        position: PhysicalPosition::new(x as i32, y as i32),
        size: PhysicalSize::new(
            ((rect.position.x + rect.size.width - 1e-9).ceil() - x) as u32,
            ((rect.position.y + rect.size.height - 1e-9).ceil() - y) as u32,
        ),
        offset: LogicalPosition::new(
            (rect.position.x - x) / factor,
            (rect.position.y - y) / factor,
        ),
    }
}

#[derive(Clone, Copy)]
struct DragAnchor {
    fraction: PhysicalPosition<f64>,
}

impl DragAnchor {
    fn new(offset: PhysicalPosition<f64>, size: PhysicalSize<u32>) -> Self {
        Self {
            fraction: PhysicalPosition::new(
                offset.x / size.width as f64,
                offset.y / size.height as f64,
            ),
        }
    }

    fn constrain(
        self,
        cursor: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
        area: PhysicalRect<i32, u32>,
        margin: i32,
    ) -> PhysicalPosition<i32> {
        let target = PhysicalPosition::new(
            (cursor.x as f64 - self.fraction.x * size.width as f64).round() as i32,
            (cursor.y as f64 - self.fraction.y * size.height as f64).round() as i32,
        );
        clamp_pet_position(target, size, area, margin)
    }
}

fn pack_screen_point(x: i32, y: i32) -> isize {
    ((x as u16 as u32) | ((y as u16 as u32) << 16)) as isize
}

#[cfg(target_os = "windows")]
mod native {
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use tauri::{LogicalPosition, WebviewWindow};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            CreateRectRgn, DeleteObject, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO,
            MonitorFromPoint, SetWindowRgn,
        },
        UI::{
            HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
            Input::KeyboardAndMouse::{GetAsyncKeyState, ReleaseCapture, VK_LBUTTON},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                GetCursorPos, GetWindowRect, HTCAPTION, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
                SendMessageW, SetWindowPos, WM_MOVING, WM_NCDESTROY, WM_NCLBUTTONDOWN,
            },
        },
    };

    use super::{
        CanvasFrame, DragAnchor, PhysicalPosition, PhysicalRect, PhysicalSize, pack_screen_point,
    };
    use crate::{PET_BASE_HEIGHT, PET_BASE_WIDTH, PET_MARGIN};

    const SUBCLASS_ID: usize = 0x46545044;
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static VISIBLE_SIZE: AtomicU64 = AtomicU64::new(0);
    thread_local! {
        static ANCHOR: Cell<Option<DragAnchor>> = const { Cell::new(None) };
    }

    struct MoveSession;

    impl Drop for MoveSession {
        fn drop(&mut self) {
            ANCHOR.set(None);
            ACTIVE.store(false, Ordering::Relaxed);
        }
    }

    pub fn is_active() -> bool {
        ACTIVE.load(Ordering::Relaxed)
    }

    pub fn visible_size(window: &WebviewWindow) -> tauri::Result<PhysicalSize<u32>> {
        let packed = VISIBLE_SIZE.load(Ordering::Relaxed);
        if packed == 0 {
            window.outer_size()
        } else {
            Ok(PhysicalSize::new(packed as u32, (packed >> 32) as u32))
        }
    }

    pub fn apply_canvas(window: &WebviewWindow, frame: CanvasFrame) -> Result<(), String> {
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0;
        let factor = unsafe { GetDpiForWindow(hwnd) } as f64 / 96.0;
        let host_width = (PET_BASE_WIDTH * factor).round() as i32;
        let host_height = (PET_BASE_HEIGHT * factor).round() as i32;
        let mut current = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut current) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let packed = frame.size.width as u64 | ((frame.size.height as u64) << 32);
        if packed != VISIBLE_SIZE.load(Ordering::Relaxed) {
            let region =
                unsafe { CreateRectRgn(0, 0, frame.size.width as i32, frame.size.height as i32) };
            if region.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            // SetWindowRgn transfers ownership on success and clips painting as well as hit testing.
            if unsafe { SetWindowRgn(hwnd, region, 1) } == 0 {
                unsafe { DeleteObject(region) };
                return Err(std::io::Error::last_os_error().to_string());
            }
            VISIBLE_SIZE.store(packed, Ordering::Relaxed);
        }
        let fixed_host = current.right - current.left == host_width
            && current.bottom - current.top == host_height;
        if current.left != frame.position.x || current.top != frame.position.y || !fixed_host {
            let flags = SWP_NOACTIVATE | SWP_NOZORDER | if fixed_host { SWP_NOSIZE } else { 0 };
            if unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    frame.position.x,
                    frame.position.y,
                    host_width,
                    host_height,
                    flags,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        Ok(())
    }

    pub fn install(window: &WebviewWindow) -> tauri::Result<()> {
        let hwnd = window.hwnd()?.0;
        if unsafe { SetWindowSubclass(hwnd, Some(window_proc), SUBCLASS_ID, 0) } == 0 {
            return Err(std::io::Error::other("failed to install native drag bounds").into());
        }
        Ok(())
    }

    pub fn start(window: &WebviewWindow, offset: LogicalPosition<f64>) -> Result<bool, String> {
        if is_active() || unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } >= 0 {
            return Ok(false);
        }
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0;
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let size = visible_size(window).map_err(|error| error.to_string())?;
        let factor = unsafe { GetDpiForWindow(hwnd) } as f64 / 96.0;
        let offset =
            PhysicalPosition::new((offset.x * factor).round(), (offset.y * factor).round());
        let point = pack_screen_point(rect.left + offset.x as i32, rect.top + offset.y as i32);
        ANCHOR.set(Some(DragAnchor::new(offset, size)));
        ACTIVE.store(true, Ordering::Relaxed);
        let _session = MoveSession;
        // This synchronous native move loop owns mouse capture until release or cancellation.
        unsafe {
            ReleaseCapture();
            SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as WPARAM, point);
        }
        Ok(true)
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        subclass_id: usize,
        _data: usize,
    ) -> LRESULT {
        if message == WM_NCDESTROY {
            ANCHOR.set(None);
            VISIBLE_SIZE.store(0, Ordering::Relaxed);
            unsafe { RemoveWindowSubclass(hwnd, Some(window_proc), subclass_id) };
        }
        let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        if message == WM_MOVING && lparam != 0 {
            let rect = unsafe { &mut *(lparam as *mut RECT) };
            if constrain_rect(hwnd, rect) {
                return 1;
            }
        }
        result
    }

    fn constrain_rect(hwnd: HWND, rect: &mut RECT) -> bool {
        let Some(anchor) = ANCHOR.get() else {
            return false;
        };
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) } == 0 {
            return false;
        }
        let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return false;
        }
        let mut dpi_x = 0;
        let mut dpi_y = 0;
        let dpi = if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }
            >= 0
        {
            dpi_x
        } else {
            unsafe { GetDpiForWindow(hwnd) }
        };
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return false;
        }
        let area = PhysicalRect {
            position: PhysicalPosition::new(info.rcWork.left, info.rcWork.top),
            size: PhysicalSize::new(
                (info.rcWork.right - info.rcWork.left) as u32,
                (info.rcWork.bottom - info.rcWork.top) as u32,
            ),
        };
        let packed = VISIBLE_SIZE.load(Ordering::Relaxed);
        let visible = PhysicalSize::new(packed as u32, (packed >> 32) as u32);
        // The fixed host's invisible area must not constrain dragging or change the grab point.
        let bounded = anchor.constrain(
            PhysicalPosition::new(cursor.x, cursor.y),
            visible,
            area,
            (PET_MARGIN * dpi as f64 / 96.0).round() as i32,
        );
        rect.left = bounded.x;
        rect.top = bounded.y;
        rect.right = bounded.x + width;
        rect.bottom = bounded.y + height;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_canvas_transform_preserves_fractional_centers_at_every_scale() {
        for factor in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0] {
            for origin in [(0, 0), (-2560, -1440), (3840, 240)] {
                let area = PhysicalRect {
                    position: PhysicalPosition::from(origin),
                    size: PhysicalSize::new(6000, 4000),
                };
                let center =
                    PhysicalPosition::new(origin.0 as f64 + 1000.37, origin.1 as f64 + 800.73);
                let mut rect = PhysicalRect {
                    position: PhysicalPosition::new(
                        center.x - 78.4 * factor,
                        center.y - 78.4 * factor,
                    ),
                    size: PhysicalSize::new(156.8 * factor, 156.8 * factor),
                };
                let mut previous = None;
                for _ in 0..5 {
                    for percent in (60..=150).chain((60..=150).rev()) {
                        let side = 392.0 * percent as f64 / 250.0 * factor;
                        let (next, anchor) = crate::resized_canvas(
                            rect,
                            PhysicalSize::new(side, side),
                            area,
                            (24.0 * factor).round() as i32,
                            factor,
                            previous,
                        );
                        let frame = enclose_canvas(next, factor);
                        let rendered = PhysicalPosition::new(
                            frame.position.x as f64 + frame.offset.x * factor + side / 2.0,
                            frame.position.y as f64 + frame.offset.y * factor + side / 2.0,
                        );
                        assert!((rendered.x - center.x).abs() < 1e-9);
                        assert!((rendered.y - center.y).abs() < 1e-9);
                        assert!(frame.size.width as f64 >= side - 1e-9);
                        assert!((frame.size.width as f64) < side + 2.0);
                        assert!(frame.offset.x >= -1e-9 && frame.offset.x * factor < 1.0 + 1e-9);
                        rect = next;
                        previous = Some(anchor);
                    }
                }
            }
        }
    }

    #[test]
    fn native_clip_region_encloses_fractional_canvas_without_crossing_boundaries() {
        for factor in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let area = PhysicalRect {
                position: PhysicalPosition::new(-2560, -1440),
                size: PhysicalSize::new(2560, 1440),
            };
            let margin = (24.0_f64 * factor).round() as i32;
            for percent in 60..=150 {
                let side = 392.0 * percent as f64 / 250.0 * factor;
                for (x, y) in [
                    (area.position.x + margin, area.position.y + margin),
                    (-margin, 0),
                ] {
                    let rect = PhysicalRect {
                        position: PhysicalPosition::new(
                            if x < -1000 { x as f64 } else { x as f64 - side },
                            if y < -1000 { y as f64 } else { y as f64 - side },
                        ),
                        size: PhysicalSize::new(side, side),
                    };
                    let frame = enclose_canvas(rect, factor);
                    assert!(frame.position.x >= area.position.x + margin);
                    assert!(frame.position.y >= area.position.y + margin);
                    assert!(frame.position.x + frame.size.width as i32 <= -margin);
                    assert!(frame.position.y + frame.size.height as i32 <= 0);
                    assert!(
                        (frame.position.x as f64 + frame.offset.x * factor - rect.position.x).abs()
                            < 1e-9
                    );
                    assert!(
                        (frame.position.y as f64 + frame.offset.y * factor - rect.position.y).abs()
                            < 1e-9
                    );
                }
            }
        }
    }

    #[test]
    fn mouse_message_coordinates_preserve_negative_desktop_positions() {
        for x in [-32768, -3840, -1, 0, 1920, 32767] {
            for y in [-32768, -2160, -1, 0, 1080, 32767] {
                let packed = pack_screen_point(x, y);
                assert_eq!(packed as u16 as i16 as i32, x);
                assert_eq!((packed >> 16) as u16 as i16 as i32, y);
            }
        }
    }

    #[test]
    fn boundary_overshoot_and_reversal_preserve_the_original_grab_point() {
        for factor in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0] {
            for origin in [(0, 0), (3840, 240), (-2560, -1440)] {
                let area = PhysicalRect {
                    position: PhysicalPosition::from(origin),
                    size: PhysicalSize::new(2560, 1440),
                };
                let margin = (24.0_f64 * factor).round() as i32;
                for scale in [0.24, 0.4, 0.6] {
                    let side = (392.0_f64 * scale * factor).round() as u32;
                    let size = PhysicalSize::new(side, side);
                    let min = PhysicalPosition::new(origin.0 + margin, origin.1 + margin);
                    let max = PhysicalPosition::new(
                        origin.0 + 2560 - margin - side as i32,
                        origin.1 + 1440 - side as i32,
                    );
                    for (x, y) in [(1, 1), (side / 2, side / 2), (side - 1, side - 1)] {
                        let offset = PhysicalPosition::new(x as f64, y as f64);
                        let anchor = DragAnchor::new(offset, size);
                        for direction in [
                            (-1, 0),
                            (1, 0),
                            (0, -1),
                            (0, 1),
                            (-1, -1),
                            (-1, 1),
                            (1, -1),
                            (1, 1),
                        ] {
                            let edge = PhysicalPosition::new(
                                if direction.0 < 0 {
                                    min.x
                                } else if direction.0 > 0 {
                                    max.x
                                } else {
                                    origin.0 + 700
                                },
                                if direction.1 < 0 {
                                    min.y
                                } else if direction.1 > 0 {
                                    max.y
                                } else {
                                    origin.1 + 400
                                },
                            );
                            for _ in 0..3 {
                                for distance in (-40..=160).chain((-40..=160).rev()) {
                                    let target = PhysicalPosition::new(
                                        edge.x + direction.0 * distance,
                                        edge.y + direction.1 * distance,
                                    );
                                    let cursor = PhysicalPosition::new(
                                        target.x + x as i32,
                                        target.y + y as i32,
                                    );
                                    let actual = anchor.constrain(cursor, size, area, margin);
                                    let expected = if distance >= 0 { edge } else { target };
                                    assert_eq!(
                                        actual, expected,
                                        "direction={direction:?} distance={distance}"
                                    );
                                    if distance <= 0 {
                                        assert_eq!(
                                            (cursor.x - actual.x, cursor.y - actual.y),
                                            (x as i32, y as i32)
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn reversing_inside_the_blocked_zone_does_not_move_the_window() {
        let size = PhysicalSize::new(157, 157);
        let area = PhysicalRect {
            position: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(1920, 1080),
        };
        let anchor = DragAnchor::new(PhysicalPosition::new(10.0, 80.0), size);
        for x in [100, 34, 25, 10, 0, 10, 25, 33, 34, 35, 100] {
            let actual = anchor.constrain(PhysicalPosition::new(x, 500), size, area, 24);
            assert_eq!(actual, PhysicalPosition::new((x - 10).max(24), 420));
        }
    }

    #[test]
    fn resized_rect_preserves_relative_grab_point_without_rebasing() {
        let initial_size = PhysicalSize::new(157, 157);
        let anchor = DragAnchor::new(PhysicalPosition::new(31.0, 121.0), initial_size);
        for (origin, factor) in [
            ((-2560, -1440), 1.0),
            ((0, 0), 1.5),
            ((3840, 240), 2.0),
            ((0, 0), 1.0),
        ] {
            let area = PhysicalRect {
                position: PhysicalPosition::from(origin),
                size: PhysicalSize::new(2560, 1440),
            };
            let size = PhysicalSize::new(
                (157.0_f64 * factor).round() as u32,
                (157.0_f64 * factor).round() as u32,
            );
            let cursor = PhysicalPosition::new(origin.0 + 1000, origin.1 + 800);
            let position = anchor.constrain(cursor, size, area, (24.0_f64 * factor).round() as i32);
            assert!(
                ((cursor.x - position.x) as f64 - 31.0 * size.width as f64 / 157.0).abs() <= 0.5
            );
            assert!(
                ((cursor.y - position.y) as f64 - 121.0 * size.height as f64 / 157.0).abs() <= 0.5
            );
        }
    }
}
