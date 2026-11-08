//! Asks the system to run a thread ahead of others or behind them: the sound thread must never
//! wait behind background work (aiming, particles), or the speakers run dry. Only Windows is
//! asked; elsewhere the threads keep the usual priority.

/// How soon a thread runs when several want to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// Keeps the speakers fed.
    High,
    /// Background work that may wait.
    Low,
}

/// Sets the calling thread's priority. Doing nothing is fine: it is only a request.
pub fn set_this_thread(priority: Priority) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL, THREAD_PRIORITY_BELOW_NORMAL,
        };
        let level = match priority {
            Priority::High => THREAD_PRIORITY_ABOVE_NORMAL,
            Priority::Low => THREAD_PRIORITY_BELOW_NORMAL,
        };
        // SAFETY: the pseudo handle of the calling thread is always valid, and a refused
        // request leaves the priority as it was.
        unsafe {
            SetThreadPriority(GetCurrentThread(), level);
        }
    }
    #[cfg(not(windows))]
    let _ = priority;
}
