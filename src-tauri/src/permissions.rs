#[cfg(target_os = "macos")]
mod platform {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    pub fn is_trusted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }

    pub fn open_settings() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn is_trusted() -> bool {
        true
    }

    pub fn open_settings() {}
}

pub use platform::{is_trusted, open_settings};
