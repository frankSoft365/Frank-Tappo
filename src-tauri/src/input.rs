// Global input listener.
//
// Privacy invariant: the raw key value never leaves this module. The frontend
// only receives which half of the keyboard was used. No keycodes, characters
// or input text are stored, logged or transmitted.
//
// macOS: a listen-only CGEventTap reads only the virtual keycode. rdev's
// macOS backend converts keycodes to strings via TSM on the tap thread, which
// trips a main-queue assertion (SIGTRAP) on macOS 15 — and reads input-source
// data we never want. Windows keeps rdev (WH_KEYBOARD_LL).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::AppHandle;

#[derive(Clone, Serialize)]
struct KeyInput {
    side: &'static str,
    ts: u64,
}

#[derive(Clone, Serialize)]
struct MouseInput {
    button: &'static str,
    ts: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn spawn_listener(app: AppHandle) {
    platform::spawn(app);
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicPtr, Ordering};

    use tauri::{AppHandle, Emitter};

    use super::{now_ms, KeyInput, MouseInput};

    type CFTypeRef = *const c_void;
    type CGEventRef = *const c_void;
    type CFMachPortRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFStringRef = *const c_void;

    const SESSION_EVENT_TAP: u32 = 1;
    const HEAD_INSERT_EVENT_TAP: u32 = 0;
    const OPTION_LISTEN_ONLY: u32 = 1;

    const EVENT_LEFT_MOUSE_DOWN: u32 = 1;
    const EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
    const EVENT_KEY_DOWN: u32 = 10;
    const EVENT_SCROLL_WHEEL: u32 = 22;
    const EVENT_OTHER_MOUSE_DOWN: u32 = 25;
    const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;

    const FIELD_KEYCODE: u32 = 9;
    const FIELD_BUTTON_NUMBER: u32 = 3;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: extern "C" fn(
                proxy: *mut c_void,
                event_type: u32,
                event: CGEventRef,
                user_info: *mut c_void,
            ) -> CGEventRef,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopCommonModes: CFStringRef;
        fn CFMachPortCreateRunLoopSource(
            allocator: CFTypeRef,
            port: CFMachPortRef,
            order: isize,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(run_loop: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopRun();
        fn CFRelease(cf: CFTypeRef);
    }

    struct TapContext {
        app: AppHandle,
        tap: AtomicPtr<c_void>,
    }

    pub fn spawn(app: AppHandle) {
        let _ = std::thread::Builder::new()
            .name("global-input".into())
            .spawn(move || {
                let context = Box::into_raw(Box::new(TapContext {
                    app,
                    tap: AtomicPtr::new(ptr::null_mut()),
                }));

                let mask = (1u64 << EVENT_KEY_DOWN)
                    | (1u64 << EVENT_LEFT_MOUSE_DOWN)
                    | (1u64 << EVENT_RIGHT_MOUSE_DOWN)
                    | (1u64 << EVENT_OTHER_MOUSE_DOWN)
                    | (1u64 << EVENT_SCROLL_WHEEL);

                let tap = unsafe {
                    CGEventTapCreate(
                        SESSION_EVENT_TAP,
                        HEAD_INSERT_EVENT_TAP,
                        OPTION_LISTEN_ONLY,
                        mask,
                        tap_callback,
                        context as *mut c_void,
                    )
                };
                if tap.is_null() {
                    eprintln!("[input] CGEventTapCreate failed; input monitoring unavailable");
                    return;
                }

                unsafe {
                    (*context).tap.store(tap, Ordering::Relaxed);
                    let source = CFMachPortCreateRunLoopSource(ptr::null(), tap, 0);
                    CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
                    CFRelease(source as CFTypeRef);
                    CGEventTapEnable(tap, true);
                }
                eprintln!("[input] CGEventTap listening");
                unsafe { CFRunLoopRun() };
            });
    }

    extern "C" fn tap_callback(
        _proxy: *mut c_void,
        event_type: u32,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef {
        unsafe {
            let context = &*(user_info as *const TapContext);
            match event_type {
                EVENT_KEY_DOWN => {
                    let code = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
                    let _ = context.app.emit(
                        "input:key",
                        KeyInput {
                            side: key_side(code),
                            ts: now_ms(),
                        },
                    );
                }
                EVENT_LEFT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_DOWN | EVENT_OTHER_MOUSE_DOWN => {
                    let button = CGEventGetIntegerValueField(event, FIELD_BUTTON_NUMBER);
                    let button = match button {
                        1 => "right",
                        2 => "middle",
                        0 => "left",
                        _ => "other",
                    };
                    let _ = context.app.emit(
                        "input:mouse",
                        MouseInput {
                            button,
                            ts: now_ms(),
                        },
                    );
                }
                EVENT_SCROLL_WHEEL => {
                    let _ = context.app.emit("input:wheel", ());
                }
                TAP_DISABLED_BY_TIMEOUT | TAP_DISABLED_BY_USER => {
                    let tap = context.tap.load(Ordering::Relaxed);
                    if !tap.is_null() {
                        CGEventTapEnable(tap, true);
                    }
                }
                _ => {}
            }
        }
        event
    }

    // ANSI virtual keycodes. Only the physical position matters: left-hand
    // keys map to the left paw, everything else to the right paw.
    fn key_side(code: u16) -> &'static str {
        match code {
            0x00 | 0x01 | 0x02 | 0x03 | 0x05 // A S D F G
            | 0x06 | 0x07 | 0x08 | 0x09 | 0x0B // Z X C V B
            | 0x0C | 0x0D | 0x0E | 0x0F | 0x11 // Q W E R T
            | 0x12 | 0x13 | 0x14 | 0x15 | 0x17 // 1 2 3 4 5
            | 0x30 | 0x32 | 0x35 | 0x37 | 0x38 | 0x39 | 0x3A | 0x3B // Tab ` Esc Cmd Shift Caps Opt Ctrl
            | 0x63 | 0x76 | 0x78 | 0x7A // F3 F4 F2 F1
            => "left",
            _ => "right",
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use rdev::{Button, EventType, Key};
    use tauri::{AppHandle, Emitter};

    use super::{now_ms, KeyInput, MouseInput};

    pub fn spawn(app: AppHandle) {
        let _ = std::thread::Builder::new()
            .name("global-input".into())
            .spawn(move || {
                let result = rdev::listen(move |event| match event.event_type {
                    EventType::KeyPress(key) => {
                        let _ = app.emit(
                            "input:key",
                            KeyInput {
                                side: key_side(key),
                                ts: now_ms(),
                            },
                        );
                    }
                    EventType::ButtonPress(button) => {
                        let _ = app.emit(
                            "input:mouse",
                            MouseInput {
                                button: button_name(button),
                                ts: now_ms(),
                            },
                        );
                    }
                    EventType::Wheel { .. } => {
                        let _ = app.emit("input:wheel", ());
                    }
                    _ => {}
                });

                if let Err(error) = result {
                    eprintln!("[input] global listener stopped: {error:?}");
                }
            });
    }

    fn button_name(button: Button) -> &'static str {
        match button {
            Button::Left => "left",
            Button::Right => "right",
            Button::Middle => "middle",
            _ => "other",
        }
    }

    fn key_side(key: Key) -> &'static str {
        use Key::*;
        match key {
            KeyQ | KeyW | KeyE | KeyR | KeyT
            | KeyA | KeyS | KeyD | KeyF | KeyG
            | KeyZ | KeyX | KeyC | KeyV | KeyB
            | Num1 | Num2 | Num3 | Num4 | Num5
            | BackQuote | Tab | CapsLock | Escape
            | ShiftLeft | ControlLeft | MetaLeft | Alt
            | F1 | F2 | F3 | F4 => "left",
            _ => "right",
        }
    }
}
