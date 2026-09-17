//! Keep long, user-started work (imports, Apple Photos sync) at full speed
//! while Laika's window is in the background. Without this macOS App Nap
//! throttles the process and preview generation slows to a crawl.

/// Holds a macOS "user-initiated activity" for as long as it lives.
pub(crate) struct KeepAwake {
    #[cfg(target_os = "macos")]
    token: Option<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
    >,
}

impl KeepAwake {
    pub(crate) fn begin(reason: &str) -> Self {
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
            let info = NSProcessInfo::processInfo();
            let token = info.beginActivityWithOptions_reason(
                NSActivityOptions::UserInitiated,
                &NSString::from_str(reason),
            );
            Self { token: Some(token) }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = reason;
            Self {}
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(token) = self.token.take() {
            // SAFETY: `token` came from `beginActivityWithOptions_reason` on
            // this process and is ended exactly once.
            unsafe { objc2_foundation::NSProcessInfo::processInfo().endActivity(&token) };
        }
    }
}

/// Run the calling thread at user-initiated QoS (performance cores, not
/// background scheduling) — for import workers.
pub(crate) fn user_initiated_thread() {
    laika_raw::user_initiated_thread();
}
