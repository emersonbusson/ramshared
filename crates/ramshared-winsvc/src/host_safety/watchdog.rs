//! Windows memory pressure watchdog thread.
//!
//! Monitors host physical memory availability using `GlobalMemoryStatusEx` and
//! triggers auto-release of VRAM swap when available memory drops below a configured threshold.

#[cfg(windows)]
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
#[cfg(windows)]
use std::mem;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Trait defining a control interface to release VRAM swap.
pub trait VramReleaseControl: Send + Sync {
    /// Triggers the release of VRAM swap.
    fn release_vram(&self);
}

/// Background watchdog that monitors host memory pressure.
pub struct MemoryWatchdog {
    stop_flag: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MemoryWatchdog {
    /// Starts the memory pressure watchdog thread.
    ///
    /// Evaluates memory threshold periodically, triggering a release when available memory falls below `threshold_bytes`.
    pub fn start<C: VramReleaseControl + 'static>(
        #[allow(unused_variables)]
        threshold_bytes: u64,
        poll_interval: Duration,
        #[allow(unused_variables)]
        control: Arc<C>,
    ) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop_flag);

        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                #[cfg(windows)]
                {
                    if let Some(avail) = available_physical_memory() {
                        if avail < threshold_bytes {
                            control.release_vram();
                        }
                    }
                }
                thread::sleep(poll_interval);
            }
        });

        Self {
            stop_flag,
            thread: Some(thread),
        }
    }

    /// Stops the memory pressure watchdog thread.
    pub fn stop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.stop_flag.store(true, Ordering::Release);
            let _ = thread.join();
        }
    }
}

impl Drop for MemoryWatchdog {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(windows)]
fn available_physical_memory() -> Option<u64> {
    // SAFETY: mem_status is properly sized according to Windows API contract.
    let mut mem_status: MEMORYSTATUSEX = unsafe { mem::zeroed() };
    mem_status.dwLength = mem::size_of::<MEMORYSTATUSEX>() as u32;

    // SAFETY: passing a valid initialized structure pointer to the OS.
    let success = unsafe { GlobalMemoryStatusEx(&mut mem_status) };
    if success != 0 {
        Some(mem_status.ullAvailPhys)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct MockControl {
        calls: AtomicUsize,
    }

    impl VramReleaseControl for MockControl {
        fn release_vram(&self) {
            self.calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn watchdog_starts_and_stops_cleanly() {
        let control = Arc::new(MockControl {
            calls: AtomicUsize::new(0),
        });
        let mut watchdog = MemoryWatchdog::start(1024, Duration::from_millis(10), control.clone());
        thread::sleep(Duration::from_millis(50));
        watchdog.stop();
    }
}
