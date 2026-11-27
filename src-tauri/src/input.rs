// macOS uses a listen-only CGEventTap to avoid rdev's key-to-text conversion.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

const SAVE_INTERVAL: Duration = Duration::from_secs(5);
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize)]
struct Input {
    ts: u64,
    total: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCount {
    total: u64,
}

struct CountState {
    total: u64,
    closed: bool,
}

struct Counter {
    path: PathBuf,
    count: Mutex<CountState>,
    // Always acquire this before taking a snapshot. Never hold count during disk I/O.
    saved_total: Mutex<u64>,
    wake: Condvar,
}

impl Counter {
    fn load(path: PathBuf) -> io::Result<Self> {
        let total = match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice::<StoredCount>(&bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                    .total
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // A dangling symlink is an unreadable existing file, not a fresh counter.
                match fs::symlink_metadata(&path) {
                    Err(missing) if missing.kind() == io::ErrorKind::NotFound => 0,
                    _ => return Err(error),
                }
            }
            Err(error) => return Err(error),
        };
        Ok(Self {
            path,
            count: Mutex::new(CountState {
                total,
                closed: false,
            }),
            saved_total: Mutex::new(total),
            wake: Condvar::new(),
        })
    }

    fn increment(&self) -> Option<u64> {
        let mut count = self.count.lock().unwrap();
        if count.closed {
            return None;
        }
        count.total = count.total.saturating_add(1);
        Some(count.total)
    }

    fn total(&self) -> u64 {
        self.count.lock().unwrap().total
    }

    fn save(&self) -> io::Result<bool> {
        let mut saved = self.saved_total.lock().unwrap();
        let total = self.total();
        if total == *saved {
            return Ok(false);
        }
        write_snapshot(&self.path, total)?;
        *saved = total;
        Ok(true)
    }

    fn close(&self) {
        self.count.lock().unwrap().closed = true;
        self.wake.notify_all();
    }

    fn shutdown(&self) -> io::Result<bool> {
        // Release count before waiting for a writer; the worker uses the reverse order.
        self.close();
        self.save()
    }

    fn persist_periodically(&self) {
        loop {
            let count = self.count.lock().unwrap();
            let (count, _) = self
                .wake
                .wait_timeout_while(count, SAVE_INTERVAL, |count| !count.closed)
                .unwrap();
            if count.closed {
                return;
            }
            drop(count);
            if let Err(error) = self.save() {
                eprintln!(
                    "[input] failed to save counter {}: {error}",
                    self.path.display()
                );
            }
        }
    }
}

fn write_snapshot(path: &Path, total: u64) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("counter has no directory"))?;
    fs::create_dir_all(parent)?;
    // Create a sibling exclusively: a leftover from a killed process is never truncated.
    let (temporary, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".counter-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        let bytes = serde_json::to_vec(&StoredCount { total }).map_err(io::Error::other)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn init_counter(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let counter = Arc::new(Counter::load(
        app.path().app_config_dir()?.join("counter.json"),
    )?);
    app.manage(Arc::clone(&counter));
    std::thread::Builder::new()
        .name("input-counter-save".into())
        .spawn(move || counter.persist_periodically())?;
    Ok(())
}

#[tauri::command]
pub fn get_input_count(app: AppHandle) -> String {
    app.state::<Arc<Counter>>().total().to_string()
}

pub fn shutdown_counter(app: &AppHandle) {
    if let Some(counter) = app.try_state::<Arc<Counter>>() {
        if let Err(error) = counter.shutdown() {
            eprintln!(
                "[input] failed to flush counter {}: {error}",
                counter.path.display()
            );
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn emit_activity(app: &AppHandle) {
    let counter = app.state::<Arc<Counter>>();
    if let Some(total) = counter.increment() {
        // Counting is authoritative even if no webview is visible or emission fails.
        let _ = app.emit(
            "input:activity",
            Input {
                ts: now_ms(),
                total: total.to_string(),
            },
        );
    }
}

pub fn spawn_listener(app: AppHandle) {
    platform::spawn(app);
}

fn is_gamepad_press(event: gilrs::EventType) -> bool {
    // gilrs already emits ButtonPressed for analog-to-button hysteresis crossings.
    matches!(event, gilrs::EventType::ButtonPressed(_, _))
}

pub fn spawn_gamepad_listener(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("gamepad-input".into())
        .spawn(move || {
            use std::collections::HashMap;

            use gilrs::{EventType, GilrsBuilder};

            let mut gamepads = match GilrsBuilder::new().with_force_feedback(false).build() {
                Ok(gamepads) => gamepads,
                Err(error) => {
                    eprintln!("[input] gamepad support unavailable: {error}");
                    return;
                }
            };
            // Discovery may report held controls as input before the initial state settles.
            let settle_time = Duration::from_millis(500);
            let mut ready_at: HashMap<_, _> = gamepads
                .gamepads()
                .map(|(id, _)| (id, Instant::now() + settle_time))
                .collect();
            eprintln!(
                "[input] gamepad listener ready ({} connected)",
                ready_at.len()
            );

            while let Some(event) = gamepads.next_event_blocking(None) {
                gamepads.inc();
                let now = Instant::now();
                match event.event {
                    EventType::Connected => {
                        ready_at.insert(event.id, now + settle_time);
                    }
                    EventType::Disconnected => {
                        ready_at.remove(&event.id);
                    }
                    activity
                        if is_gamepad_press(activity)
                            && ready_at.get(&event.id).is_some_and(|ready| now >= *ready) =>
                    {
                        emit_activity(&app);
                    }
                    _ => {}
                }
            }
            eprintln!("[input] gamepad listener stopped");
        });
}

#[cfg(any(target_os = "macos", test))]
mod modifiers {
    // IOLLEvent.h device-dependent left/right bits plus generic fallback flags.
    const FAMILIES: [(u64, u64); 4] = [
        (0x0002 | 0x0004, 0x0002_0000), // shift
        (0x0001 | 0x2000, 0x0004_0000), // control
        (0x0020 | 0x0040, 0x0008_0000), // option
        (0x0008 | 0x0010, 0x0010_0000), // command
    ];
    const CAPS_DOWN: u64 = 0x0080 | 0x0100_0000;
    const CAPS_TOGGLE: u64 = 0x0001_0000;
    const FN: u64 = 0x0080_0000;
    const RELEVANT: u64 = 0x207f | 0x001e_0000 | CAPS_DOWN | CAPS_TOGGLE | FN;

    pub(super) struct ModifierFlags(u64);

    impl ModifierFlags {
        pub(super) fn new(flags: u64) -> Self {
            Self(flags & RELEVANT)
        }

        pub(super) fn observe(&mut self, flags: u64) -> u32 {
            let flags = flags & RELEVANT;
            let previous = std::mem::replace(&mut self.0, flags);
            let mut presses = 0;
            for (sides, generic) in FAMILIES {
                let old_sides = previous & sides;
                let new_sides = flags & sides;
                if new_sides != 0 {
                    let added = (new_sides & !old_sides).count_ones();
                    // A generic-only initial snapshot already represents a held modifier.
                    presses +=
                        added.saturating_sub(u32::from(old_sides == 0 && previous & generic != 0));
                } else if flags & generic != 0 && previous & (sides | generic) == 0 {
                    presses += 1;
                }
            }
            presses += (flags & !previous & FN).count_ones();
            let was_caps_down = previous & CAPS_DOWN != 0;
            let caps_down = flags & CAPS_DOWN != 0;
            // Both stateless bits refer to one caps key; toggles must not recount its release.
            if (caps_down && !was_caps_down)
                || (!caps_down && !was_caps_down && (flags ^ previous) & CAPS_TOGGLE != 0)
            {
                presses += 1;
            }
            presses
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::collections::HashSet;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicPtr, Ordering};

    use tauri::AppHandle;

    use super::{emit_activity, modifiers::ModifierFlags};

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
    const EVENT_LEFT_MOUSE_UP: u32 = 2;
    const EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
    const EVENT_RIGHT_MOUSE_UP: u32 = 4;
    const EVENT_KEY_DOWN: u32 = 10;
    const EVENT_KEY_UP: u32 = 11;
    const EVENT_FLAGS_CHANGED: u32 = 12;
    const EVENT_OTHER_MOUSE_DOWN: u32 = 25;
    const EVENT_OTHER_MOUSE_UP: u32 = 26;
    const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;
    const MOUSE_BUTTON_NUMBER: u32 = 3;
    const KEYBOARD_AUTOREPEAT: u32 = 8;

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
        fn CGEventGetFlags(event: CGEventRef) -> u64;
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
        fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
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
        fn CFRunLoopAddSource(
            run_loop: CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: CFStringRef,
        );
        fn CFRunLoopRun();
        fn CFRelease(cf: CFTypeRef);
    }

    struct InputState {
        modifiers: ModifierFlags,
        mouse_buttons: HashSet<i64>,
    }

    impl InputState {
        fn snapshot() -> Self {
            Self {
                modifiers: ModifierFlags::new(unsafe { CGEventSourceFlagsState(1) }),
                mouse_buttons: (0..32)
                    .filter(|&button| unsafe { CGEventSourceButtonState(1, button) })
                    .map(i64::from)
                    .collect(),
            }
        }

        fn presses(&mut self, event_type: u32, event: CGEventRef) -> u32 {
            let modifier_presses = self.modifiers.observe(unsafe { CGEventGetFlags(event) });
            match event_type {
                EVENT_KEY_DOWN => u32::from(unsafe {
                    CGEventGetIntegerValueField(event, KEYBOARD_AUTOREPEAT) == 0
                }),
                EVENT_LEFT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_DOWN | EVENT_OTHER_MOUSE_DOWN => {
                    let button = unsafe { CGEventGetIntegerValueField(event, MOUSE_BUTTON_NUMBER) };
                    u32::from(self.mouse_buttons.insert(button))
                }
                EVENT_LEFT_MOUSE_UP | EVENT_RIGHT_MOUSE_UP | EVENT_OTHER_MOUSE_UP => {
                    let button = unsafe { CGEventGetIntegerValueField(event, MOUSE_BUTTON_NUMBER) };
                    self.mouse_buttons.remove(&button);
                    0
                }
                EVENT_FLAGS_CHANGED => modifier_presses,
                _ => 0,
            }
        }
    }

    struct TapContext {
        app: AppHandle,
        tap: AtomicPtr<c_void>,
        input: Mutex<InputState>,
    }

    pub fn spawn(app: AppHandle) {
        let _ = std::thread::Builder::new()
            .name("global-input".into())
            .spawn(move || {
                let context = Box::into_raw(Box::new(TapContext {
                    app,
                    tap: AtomicPtr::new(ptr::null_mut()),
                    input: Mutex::new(InputState::snapshot()),
                }));

                let mask = (1u64 << EVENT_KEY_DOWN)
                    | (1u64 << EVENT_KEY_UP)
                    | (1u64 << EVENT_FLAGS_CHANGED)
                    | (1u64 << EVENT_LEFT_MOUSE_DOWN)
                    | (1u64 << EVENT_LEFT_MOUSE_UP)
                    | (1u64 << EVENT_RIGHT_MOUSE_DOWN)
                    | (1u64 << EVENT_RIGHT_MOUSE_UP)
                    | (1u64 << EVENT_OTHER_MOUSE_DOWN)
                    | (1u64 << EVENT_OTHER_MOUSE_UP);

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
                    unsafe { drop(Box::from_raw(context)) };
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
            if matches!(event_type, TAP_DISABLED_BY_TIMEOUT | TAP_DISABLED_BY_USER) {
                // Events may have been missed while disabled; resynchronize without counting.
                *context.input.lock().unwrap() = InputState::snapshot();
                let tap = context.tap.load(Ordering::Relaxed);
                if !tap.is_null() {
                    CGEventTapEnable(tap, true);
                }
            } else if !event.is_null() {
                let presses = context.input.lock().unwrap().presses(event_type, event);
                for _ in 0..presses {
                    emit_activity(&context.app);
                }
            }
        }
        event
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[link(name = "ApplicationServices", kind = "framework")]
        unsafe extern "C" {
            fn CGEventCreate(source: CFTypeRef) -> CGEventRef;
            fn CGEventSetType(event: CGEventRef, event_type: u32);
            fn CGEventSetFlags(event: CGEventRef, flags: u64);
            fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
        }

        fn empty_state() -> InputState {
            InputState {
                modifiers: ModifierFlags::new(0),
                mouse_buttons: HashSet::new(),
            }
        }

        fn count_event(
            state: &mut InputState,
            event_type: u32,
            repeat: i64,
            button: i64,
            flags: u64,
        ) -> u32 {
            unsafe {
                let event = CGEventCreate(ptr::null());
                assert!(!event.is_null());
                CGEventSetType(event, event_type);
                CGEventSetFlags(event, flags);
                if matches!(event_type, EVENT_KEY_DOWN | EVENT_KEY_UP) {
                    CGEventSetIntegerValueField(event, KEYBOARD_AUTOREPEAT, repeat);
                } else if event_type != EVENT_FLAGS_CHANGED {
                    CGEventSetIntegerValueField(event, MOUSE_BUTTON_NUMBER, button);
                }
                let count = state.presses(event_type, event);
                CFRelease(event);
                count
            }
        }

        #[test]
        fn keyboard_autorepeat_is_ignored_until_a_new_press() {
            let mut state = empty_state();
            assert_eq!(count_event(&mut state, EVENT_KEY_DOWN, 0, 0, 0), 1);
            for repeat in [1, 2, 10] {
                for _ in 0..100 {
                    assert_eq!(count_event(&mut state, EVENT_KEY_DOWN, repeat, 0, 0), 0);
                }
            }
            assert_eq!(count_event(&mut state, EVENT_KEY_UP, 0, 0, 0), 0);
            assert_eq!(count_event(&mut state, EVENT_KEY_DOWN, 0, 0, 0), 1);
        }

        #[test]
        fn held_key_does_not_block_a_different_key_or_mouse() {
            let mut state = empty_state();
            assert_eq!(count_event(&mut state, EVENT_KEY_DOWN, 0, 0, 0), 1);
            assert_eq!(count_event(&mut state, EVENT_KEY_DOWN, 1, 0, 0), 0);
            assert_eq!(count_event(&mut state, EVENT_KEY_DOWN, 0, 0, 0), 1);
            assert_eq!(count_event(&mut state, EVENT_LEFT_MOUSE_DOWN, 0, 0, 0), 1);
            assert_eq!(count_event(&mut state, EVENT_KEY_UP, 0, 0, 0), 0);
            assert_eq!(count_event(&mut state, EVENT_KEY_DOWN, 1, 0, 0), 0);
        }

        #[test]
        fn each_mouse_button_counts_once_until_released() {
            let mut state = empty_state();
            for (down, up, button) in [
                (EVENT_LEFT_MOUSE_DOWN, EVENT_LEFT_MOUSE_UP, 0),
                (EVENT_RIGHT_MOUSE_DOWN, EVENT_RIGHT_MOUSE_UP, 1),
                (EVENT_OTHER_MOUSE_DOWN, EVENT_OTHER_MOUSE_UP, 2),
                (EVENT_OTHER_MOUSE_DOWN, EVENT_OTHER_MOUSE_UP, 3),
                (EVENT_OTHER_MOUSE_DOWN, EVENT_OTHER_MOUSE_UP, 4),
                (EVENT_OTHER_MOUSE_DOWN, EVENT_OTHER_MOUSE_UP, 31),
            ] {
                assert_eq!(count_event(&mut state, down, 0, button, 0), 1);
                for _ in 0..100 {
                    assert_eq!(count_event(&mut state, down, 0, button, 0), 0);
                }
                assert_eq!(count_event(&mut state, up, 0, button, 0), 0);
                assert_eq!(count_event(&mut state, down, 0, button, 0), 1);
                assert_eq!(count_event(&mut state, up, 0, button, 0), 0);
            }
            assert!(state.mouse_buttons.is_empty());
        }

        #[test]
        fn simultaneous_mouse_buttons_have_independent_release_states() {
            let mut state = empty_state();
            for button in [2, 3, 4] {
                assert_eq!(
                    count_event(&mut state, EVENT_OTHER_MOUSE_DOWN, 0, button, 0),
                    1
                );
            }
            assert_eq!(count_event(&mut state, EVENT_OTHER_MOUSE_UP, 0, 3, 0), 0);
            assert_eq!(count_event(&mut state, EVENT_OTHER_MOUSE_DOWN, 0, 2, 0), 0);
            assert_eq!(count_event(&mut state, EVENT_OTHER_MOUSE_DOWN, 0, 4, 0), 0);
            assert_eq!(count_event(&mut state, EVENT_OTHER_MOUSE_DOWN, 0, 3, 0), 1);
        }

        #[test]
        fn rapid_release_and_repress_has_no_debounce_loss() {
            let mut state = empty_state();
            let mut count = 0;
            for _ in 0..1000 {
                count += count_event(&mut state, EVENT_LEFT_MOUSE_DOWN, 0, 0, 0);
                count += count_event(&mut state, EVENT_LEFT_MOUSE_UP, 0, 0, 0);
            }
            assert_eq!(count, 1000);
        }

        #[test]
        fn held_modifier_and_resynchronized_buttons_do_not_recount() {
            let shift = 0x0002_0002;
            let mut state = InputState {
                modifiers: ModifierFlags::new(shift),
                mouse_buttons: HashSet::from([0]),
            };
            assert_eq!(count_event(&mut state, EVENT_FLAGS_CHANGED, 0, 0, shift), 0);
            assert_eq!(
                count_event(&mut state, EVENT_LEFT_MOUSE_DOWN, 0, 0, shift),
                0
            );
            assert_eq!(count_event(&mut state, EVENT_KEY_DOWN, 1, 0, shift), 0);
            assert_eq!(count_event(&mut state, EVENT_LEFT_MOUSE_UP, 0, 0, shift), 0);
            assert_eq!(
                count_event(&mut state, EVENT_LEFT_MOUSE_DOWN, 0, 0, shift),
                1
            );
            assert_eq!(count_event(&mut state, EVENT_FLAGS_CHANGED, 0, 0, 0), 0);
            assert_eq!(count_event(&mut state, EVENT_FLAGS_CHANGED, 0, 0, shift), 1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::collections::HashSet;

    use rdev::EventType;
    use tauri::AppHandle;

    use super::emit_activity;

    pub fn spawn(app: AppHandle) {
        let _ = std::thread::Builder::new()
            .name("global-input".into())
            .spawn(move || {
                let mut held_keys = HashSet::new();
                let mut held_buttons = HashSet::new();
                let result = rdev::listen(move |event| match event.event_type {
                    EventType::KeyPress(key) => {
                        if held_keys.insert(key) {
                            emit_activity(&app);
                        }
                    }
                    EventType::KeyRelease(key) => {
                        held_keys.remove(&key);
                    }
                    EventType::ButtonPress(button) => {
                        if held_buttons.insert(button) {
                            emit_activity(&app);
                        }
                    }
                    EventType::ButtonRelease(button) => {
                        held_buttons.remove(&button);
                    }
                    _ => {}
                });

                if let Err(error) = result {
                    eprintln!("[input] global listener stopped: {error:?}");
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Barrier, mpsc};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            loop {
                let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "frank-tappo-input-test-{}-{}-{id}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos(),
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create test directory: {error}"),
                }
            }
        }

        fn path(&self) -> PathBuf {
            self.0.join("counter.json")
        }

        fn counter(&self) -> Counter {
            Counter::load(self.path()).unwrap()
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            // Only remove the unique directory this fixture successfully created.
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn input_payload_contains_only_timestamp_and_decimal_total() {
        assert_eq!(
            serde_json::to_value(Input {
                ts: 123,
                total: "9007199254740993".into(),
            })
            .unwrap(),
            serde_json::json!({ "ts": 123, "total": "9007199254740993" })
        );
    }

    #[test]
    fn high_rate_presses_are_not_coalesced_and_do_not_write_from_increment() {
        let directory = TestDirectory::new();
        let counter = directory.counter();
        for expected in 1..=100_000 {
            assert_eq!(counter.increment(), Some(expected));
        }
        assert_eq!(counter.total(), 100_000);
        assert!(!directory.path().exists());
    }

    #[test]
    fn concurrent_presses_have_exact_unique_totals() {
        let directory = TestDirectory::new();
        let counter = Arc::new(directory.counter());
        let barrier = Arc::new(Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let counter = Arc::clone(&counter);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    (0..10_000)
                        .map(|_| counter.increment().unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut totals: Vec<_> = threads
            .into_iter()
            .flat_map(|thread| thread.join().unwrap())
            .collect();
        totals.sort_unstable();
        assert_eq!(totals, (1..=80_000).collect::<Vec<_>>());
        assert_eq!(counter.total(), 80_000);
        assert!(!directory.path().exists());
    }

    #[test]
    fn missing_counter_starts_at_zero_without_rewriting_unchanged_state() {
        let directory = TestDirectory::new();
        let counter = directory.counter();
        assert_eq!(counter.total(), 0);
        assert!(!counter.save().unwrap());
        assert!(!directory.path().exists());
    }

    #[test]
    fn persisted_roundtrip_preserves_values_above_javascript_safe_integer() {
        let directory = TestDirectory::new();
        let initial = (1u64 << 53) + 123;
        fs::write(directory.path(), format!("{{\"total\":{initial}}}")).unwrap();
        let counter = directory.counter();
        assert_eq!(counter.total(), initial);
        assert_eq!(counter.increment(), Some(initial + 1));
        assert!(counter.save().unwrap());
        assert_eq!(directory.counter().total(), initial + 1);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(directory.path()).unwrap())
                .unwrap(),
            serde_json::json!({ "total": initial + 1 }),
        );
    }

    #[test]
    fn unchanged_total_does_not_rewrite_existing_file() {
        let directory = TestDirectory::new();
        let original = b"{ \"total\": 41 }\n";
        fs::write(directory.path(), original).unwrap();
        let modified = fs::metadata(directory.path()).unwrap().modified().unwrap();
        let counter = directory.counter();
        assert!(!counter.save().unwrap());
        assert!(!counter.shutdown().unwrap());
        assert_eq!(fs::read(directory.path()).unwrap(), original);
        assert_eq!(
            fs::metadata(directory.path()).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn corrupt_counter_is_rejected_and_never_overwritten() {
        let directory = TestDirectory::new();
        for invalid in [
            "not json",
            "{}",
            "{\"total\":-1}",
            "{\"total\":1.5}",
            "{\"total\":\"12\"}",
            "{\"total\":18446744073709551616}",
            "{\"total\":5,\"history\":[]}",
            "{\"total\":1,\"total\":2}",
        ] {
            fs::write(directory.path(), invalid).unwrap();
            assert!(Counter::load(directory.path()).is_err(), "{invalid}");
            assert_eq!(fs::read_to_string(directory.path()).unwrap(), invalid);
            assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
        }
    }

    #[test]
    fn unreadable_existing_counter_is_not_treated_as_missing() {
        let directory = TestDirectory::new();
        fs::create_dir(directory.path()).unwrap();
        assert!(Counter::load(directory.path()).is_err());
        assert!(directory.path().is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_counter_symlink_is_not_treated_as_missing() {
        let directory = TestDirectory::new();
        let missing = directory.0.join("missing");
        std::os::unix::fs::symlink(&missing, directory.path()).unwrap();
        assert!(Counter::load(directory.path()).is_err());
        assert_eq!(fs::read_link(directory.path()).unwrap(), missing);
        assert!(!missing.exists());
    }

    #[test]
    fn counter_saturates_instead_of_wrapping() {
        let directory = TestDirectory::new();
        fs::write(directory.path(), format!("{{\"total\":{}}}", u64::MAX - 1)).unwrap();
        let counter = directory.counter();
        assert_eq!(counter.increment(), Some(u64::MAX));
        assert_eq!(counter.increment(), Some(u64::MAX));
        counter.shutdown().unwrap();
        assert_eq!(directory.counter().total(), u64::MAX);
    }

    #[test]
    fn snapshot_is_taken_only_after_acquiring_writer_lock() {
        let directory = TestDirectory::new();
        let counter = Arc::new(directory.counter());
        let mut count = counter.count.lock().unwrap();
        let writer = Arc::clone(&counter);
        let thread = std::thread::spawn(move || writer.save().unwrap());
        // While snapshot acquisition is blocked, save must already own the writer lock.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match counter.saved_total.try_lock() {
                Err(std::sync::TryLockError::WouldBlock) => break,
                Ok(guard) => drop(guard),
                Err(error) => panic!("writer mutex poisoned: {error}"),
            }
            assert!(
                Instant::now() < deadline,
                "save acquired locks in the wrong order"
            );
            std::thread::yield_now();
        }
        count.total = 17;
        drop(count);
        assert!(thread.join().unwrap());
        assert_eq!(directory.counter().total(), 17);
    }

    #[test]
    fn concurrent_saves_never_overwrite_a_newer_snapshot() {
        let directory = TestDirectory::new();
        let counter = Arc::new(directory.counter());
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let counter = Arc::clone(&counter);
                std::thread::spawn(move || {
                    for _ in 0..25 {
                        let accepted = counter.increment().unwrap();
                        counter.save().unwrap();
                        let saved = counter.saved_total.lock().unwrap();
                        let stored = Counter::load(counter.path.clone()).unwrap().total();
                        assert_eq!(stored, *saved);
                        assert!(stored >= accepted);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        counter.shutdown().unwrap();
        assert_eq!(directory.counter().total(), 100);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn failed_save_keeps_previous_file_and_remains_dirty_for_retry() {
        let directory = TestDirectory::new();
        let mut counter = directory.counter();
        counter.increment();
        counter.save().unwrap();
        let prior = fs::read(directory.path()).unwrap();
        counter.increment();
        // A regular file cannot also be a parent directory, reliably forcing I/O failure.
        counter.path = directory.path().join("counter.json");
        assert!(counter.save().is_err());
        assert_eq!(*counter.saved_total.lock().unwrap(), 1);
        assert_eq!(fs::read(directory.path()).unwrap(), prior);
        counter.path = directory.path();
        assert!(counter.save().unwrap());
        assert_eq!(directory.counter().total(), 2);
    }

    #[test]
    fn failed_rename_preserves_destination_and_removes_own_temporary_file() {
        let directory = TestDirectory::new();
        fs::create_dir(directory.path()).unwrap();
        let marker = directory.path().join("prior");
        fs::write(&marker, "preserve me").unwrap();
        assert!(write_snapshot(&directory.path(), 123).is_err());
        assert_eq!(fs::read_to_string(marker).unwrap(), "preserve me");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn shutdown_flushes_and_rejects_later_presses() {
        let directory = TestDirectory::new();
        let counter = directory.counter();
        assert_eq!(counter.increment(), Some(1));
        assert!(counter.shutdown().unwrap());
        for _ in 0..1000 {
            assert_eq!(counter.increment(), None);
        }
        assert_eq!(counter.total(), 1);
        assert_eq!(directory.counter().total(), 1);
        assert!(!counter.shutdown().unwrap());
    }

    #[test]
    fn shutdown_racing_with_inputs_and_saves_persists_every_accepted_press() {
        let directory = TestDirectory::new();
        let counter = Arc::new(directory.counter());
        counter.increment();
        let barrier = Arc::new(Barrier::new(5));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let counter = Arc::clone(&counter);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut accepted = 0;
                    for index in 0..1000 {
                        if counter.increment().is_some() {
                            accepted += 1;
                        }
                        if index % 100 == 0 {
                            counter.save().unwrap();
                        }
                    }
                    accepted
                })
            })
            .collect();
        barrier.wait();
        counter.shutdown().unwrap();
        let accepted: u64 = 1 + threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .sum::<u64>();
        assert_eq!(counter.total(), accepted);
        assert_eq!(directory.counter().total(), accepted);
        assert_eq!(counter.increment(), None);
    }

    #[test]
    fn shutdown_wakes_background_worker_without_waiting_five_seconds() {
        let directory = TestDirectory::new();
        let counter = Arc::new(directory.counter());
        let worker = Arc::clone(&counter);
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            worker.persist_periodically();
            sender.send(()).unwrap();
        });
        counter.shutdown().unwrap();
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn gamepad_counts_only_button_pressed_including_mapped_analog_buttons() {
        use gilrs::{Axis, Button, EventType};
        // to_nec supplies real gilrs Code values without discovering any hardware.
        for button in [Button::South, Button::LeftTrigger2, Button::DPadUp] {
            let code = button.to_nec().unwrap();
            assert!(is_gamepad_press(EventType::ButtonPressed(button, code)));
            for event in [
                EventType::ButtonReleased(button, code),
                EventType::ButtonRepeated(button, code),
                EventType::ButtonChanged(button, 0.0, code),
                EventType::ButtonChanged(button, 0.5, code),
                EventType::ButtonChanged(button, 1.0, code),
                EventType::AxisChanged(Axis::LeftStickX, -1.0, code),
                EventType::AxisChanged(Axis::LeftStickY, 1.0, code),
                EventType::Connected,
                EventType::Disconnected,
                EventType::Dropped,
                EventType::ForceFeedbackEffectCompleted,
            ] {
                assert!(!is_gamepad_press(event), "{event:?}");
            }
        }
    }

    #[test]
    fn gilrs_press_and_changed_pair_counts_once() {
        use gilrs::{Button, EventType};
        let button = Button::RightTrigger2;
        let code = button.to_nec().unwrap();
        let events = [
            EventType::ButtonChanged(button, 0.4, code),
            EventType::ButtonPressed(button, code),
            EventType::ButtonChanged(button, 1.0, code),
            EventType::ButtonChanged(button, 0.8, code),
            EventType::ButtonReleased(button, code),
            EventType::ButtonChanged(button, 0.0, code),
        ];
        assert_eq!(
            events
                .into_iter()
                .filter(|event| is_gamepad_press(*event))
                .count(),
            1
        );
    }

    #[test]
    fn modifiers_count_press_not_release_and_distinguish_both_sides() {
        use modifiers::ModifierFlags;
        for (left, right, generic) in [
            (0x2, 0x4, 0x20000),
            (0x1, 0x2000, 0x40000),
            (0x20, 0x40, 0x80000),
            (0x8, 0x10, 0x100000),
        ] {
            let mut flags = ModifierFlags::new(0);
            assert_eq!(flags.observe(left | generic), 1);
            assert_eq!(flags.observe(left | generic), 0);
            assert_eq!(flags.observe(left | right | generic), 1);
            assert_eq!(flags.observe(right | generic), 0);
            assert_eq!(flags.observe(0), 0);
            assert_eq!(flags.observe(left | right | generic), 2);
            assert_eq!(flags.observe(0), 0);
        }
    }

    #[test]
    fn modifier_combinations_and_fn_count_only_added_bits() {
        let mut flags = modifiers::ModifierFlags::new(0);
        let combination = 0x2 | 0x1 | 0x8 | 0x20000 | 0x40000 | 0x100000;
        assert_eq!(flags.observe(combination), 3);
        assert_eq!(flags.observe(combination | 0x800000), 1);
        assert_eq!(flags.observe(combination), 0);
        assert_eq!(flags.observe(0), 0);
        // Numeric-pad, help and noncoalesced flags are not physical modifier presses.
        assert_eq!(flags.observe(0x200000 | 0x400000 | 0x100), 0);
    }

    #[test]
    fn generic_modifier_fallback_does_not_double_count_sided_flags() {
        for generic in [0x20000, 0x40000, 0x80000, 0x100000] {
            let mut flags = modifiers::ModifierFlags::new(0);
            assert_eq!(flags.observe(generic), 1);
            assert_eq!(flags.observe(generic), 0);
            assert_eq!(flags.observe(0), 0);
        }
        let mut flags = modifiers::ModifierFlags::new(0x20000);
        assert_eq!(flags.observe(0x20000 | 0x2), 0);
        assert_eq!(flags.observe(0x20000 | 0x2 | 0x4), 1);
        assert_eq!(flags.observe(0x20000), 0);
        assert_eq!(flags.observe(0), 0);
    }

    #[test]
    fn initially_held_modifiers_and_caps_do_not_count_on_release() {
        let mut flags = modifiers::ModifierFlags::new(0x2 | 0x4 | 0x20000 | 0x800000);
        assert_eq!(flags.observe(0x4 | 0x20000), 0);
        assert_eq!(flags.observe(0), 0);
        let mut flags = modifiers::ModifierFlags::new(0x80 | 0x1000000 | 0x10000);
        assert_eq!(flags.observe(0x10000), 0);
        assert_eq!(flags.observe(0x80), 1); // Next actual press toggles caps off.
        assert_eq!(flags.observe(0), 0);
    }

    #[test]
    fn modifier_state_is_refreshed_by_other_observed_events() {
        let mut flags = modifiers::ModifierFlags::new(0);
        // The callback discards the modifier delta for a key/mouse event, but updates it.
        let _ = flags.observe(0x2 | 0x20000);
        assert_eq!(flags.observe(0x2 | 0x20000), 0);
        assert_eq!(flags.observe(0), 0);
    }

    #[test]
    fn caps_toggle_only_counts_each_press_in_either_direction() {
        let mut flags = modifiers::ModifierFlags::new(0);
        for _ in 0..3 {
            assert_eq!(flags.observe(0x10000), 1);
            assert_eq!(flags.observe(0x10000), 0);
            assert_eq!(flags.observe(0), 1);
            assert_eq!(flags.observe(0), 0);
        }
    }

    #[test]
    fn caps_stateless_bits_never_double_count_press_and_release() {
        for down in [0x80, 0x1000000, 0x80 | 0x1000000] {
            let mut flags = modifiers::ModifierFlags::new(0);
            assert_eq!(flags.observe(down | 0x10000), 1);
            assert_eq!(flags.observe(down | 0x10000), 0);
            assert_eq!(flags.observe(0x10000), 0);
            assert_eq!(flags.observe(down), 1);
            assert_eq!(flags.observe(0), 0);
            // Also tolerate a toggle reported after the stateless press, at release.
            assert_eq!(flags.observe(down), 1);
            assert_eq!(flags.observe(0x10000), 0);
        }
        let mut flags = modifiers::ModifierFlags::new(0x80);
        assert_eq!(flags.observe(0x1000000 | 0x10000), 0);
        assert_eq!(flags.observe(0x10000), 0);
    }
}
