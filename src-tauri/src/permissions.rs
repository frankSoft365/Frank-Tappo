#[cfg(target_os = "macos")]
mod platform {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightListenEventAccess() -> u8;
        fn CGRequestListenEventAccess() -> u8;
    }

    pub fn is_trusted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }

    pub fn listen_access() -> bool {
        unsafe { CGPreflightListenEventAccess() != 0 }
    }

    pub fn request_listen_access() -> bool {
        unsafe { CGRequestListenEventAccess() != 0 }
    }

    pub fn ready() -> bool {
        listen_access()
    }

    fn open_pane(pane: &str) {
        let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
        let _ = std::process::Command::new("open").arg(url).spawn();
    }

    pub fn open_settings() {
        open_pane("Privacy_Accessibility");
    }

    pub fn open_input_monitoring_settings() {
        open_pane("Privacy_ListenEvent");
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn is_trusted() -> bool {
        true
    }

    pub fn listen_access() -> bool {
        true
    }

    pub fn request_listen_access() -> bool {
        true
    }

    pub fn ready() -> bool {
        true
    }

    pub fn open_settings() {}

    pub fn open_input_monitoring_settings() {}
}

pub use platform::{
    is_trusted, listen_access, open_input_monitoring_settings, open_settings, request_listen_access,
};

pub fn input_ready() -> bool {
    platform::ready()
}
