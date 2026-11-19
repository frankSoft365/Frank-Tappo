// Global input listener.
//
// Privacy invariant: the raw key value never leaves this module. The frontend
// only receives which half of the keyboard was used. No keycodes, characters
// or input text are stored, logged or transmitted.

use std::time::{SystemTime, UNIX_EPOCH};

use rdev::{Button, EventType, Key};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

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
