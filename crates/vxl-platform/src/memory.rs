//! Process memory introspection, used by the RAM HUD and the Zero-Drag budget check.

/// Current working set of *this* process, in bytes.
///
/// This counts only the Rust core. WebView2 renderer and GPU processes are separate and not included.
/// Returns `None` if the OS call fails or the platform is unsupported.
pub fn working_set_bytes() -> Option<u64> {
    imp::working_set_bytes()
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    pub fn working_set_bytes() -> Option<u64> {
        let size = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
        // SAFETY: PROCESS_MEMORY_COUNTERS is a plain C struct of integers; all-zero is a valid value.
        let mut pmc: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        pmc.cb = size;
        // SAFETY: GetCurrentProcess returns a pseudo-handle that is always valid and needs no closing.
        // `pmc` is a properly sized, writable PROCESS_MEMORY_COUNTERS and `size` is its byte length.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, size) };
        (ok != 0).then_some(pmc.WorkingSetSize as u64)
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn working_set_bytes() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(windows)]
    fn reports_nonzero_working_set() {
        let ws = super::working_set_bytes();
        assert!(
            matches!(ws, Some(b) if b > 1024 * 1024),
            "unexpected working set: {ws:?}"
        );
    }
}
