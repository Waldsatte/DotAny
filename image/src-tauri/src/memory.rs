//! How much memory a large picture could still take right now: Image's own question, so it isn't in the family's
//! platform layer. A port to another system adds its answer here.

#[cfg(windows)]
pub fn available() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut s: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    s.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut s) } == 0 {
        return u64::MAX;
    }
    s.ullAvailPhys
}

/// The Mac: its memory times the share of it the system counts as free (`kern.memorystatus_level`, what
/// `memory_pressure` calls the system-wide memory free percentage: it counts what can be given back without paging out),
/// as the hub's converter asks it (`shared/convert`).
#[cfg(target_os = "macos")]
pub fn available() -> u64 {
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const std::ffi::c_char,
            old: *mut std::ffi::c_void,
            old_len: *mut usize,
            new: *mut std::ffi::c_void,
            new_len: usize,
        ) -> i32;
    }
    fn sysctl<T: Default>(name: &std::ffi::CStr) -> Option<T> {
        let mut v = T::default();
        let mut len = std::mem::size_of::<T>();
        // SAFETY: a value of the size the name's value has, and its length
        let r = unsafe { sysctlbyname(name.as_ptr(), &mut v as *mut T as *mut _, &mut len, std::ptr::null_mut(), 0) };
        (r == 0 && len == std::mem::size_of::<T>()).then_some(v)
    }
    match (sysctl::<u64>(c"hw.memsize"), sysctl::<i32>(c"kern.memorystatus_level")) {
        (Some(total), Some(pct)) if (0..=100).contains(&pct) => total / 100 * pct as u64,
        _ => u64::MAX,
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn available() -> u64 {
    u64::MAX
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn the_mac_says_how_much_is_free() {
        let free = super::available();
        // some memory is free, and never more than any Mac has
        assert!(free > 64 << 20 && free < 1 << 40, "{free}");
    }
}
