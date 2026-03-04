use std::sync::atomic::{AtomicBool, Ordering};

static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set(enabled: bool) {
    VERBOSE.store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// Emit a dimmed diagnostic line to stderr when `--verbose` is active.
///
/// Usage:  `vlog!("git fetch {} --prune", remote);`
#[macro_export]
macro_rules! vlog {
    ($($arg:tt)*) => {
        if $crate::verbose::is_enabled() {
            use colored::Colorize as _;
            eprintln!("{} {}", "▸".dimmed(), format!($($arg)*));
        }
    };
}
