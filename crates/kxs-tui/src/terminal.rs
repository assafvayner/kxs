//! Terminal setup/teardown. Both a panic hook and a `Drop` guard restore the
//! terminal, so a panic never leaves the shell unusable.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

static ON_SCREEN: AtomicBool = AtomicBool::new(false);

pub fn enter() -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    ON_SCREEN.store(true, Ordering::SeqCst);
    Ok(())
}

/// Leave raw mode and the alternate screen. Safe to call repeatedly.
pub fn restore() {
    if ON_SCREEN.swap(false, Ordering::SeqCst) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = io::stdout().flush();
    }
}

/// Installs a panic hook that restores the terminal before the default hook
/// reports the panic.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        default_hook(info);
    }));
}

/// Restores the terminal when dropped, on every exit path through `main`.
pub struct RestoreGuard;

impl Drop for RestoreGuard {
    fn drop(&mut self) {
        restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_is_idempotent() {
        // Not on screen; both calls must be no-ops that leave the flag false.
        restore();
        restore();
        assert!(!ON_SCREEN.load(Ordering::SeqCst));
    }

    #[test]
    fn guard_drops_without_panic_when_not_on_screen() {
        let _g = RestoreGuard;
    }
}
