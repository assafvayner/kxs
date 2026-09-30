//! kxs TUI library surface — the binary in `main.rs` is thin so the runtime
//! stays unit-testable.

pub mod app;
pub mod chrome;
pub mod clipboard;
pub mod cmd;
pub mod config;
pub mod ctx;
pub mod msg;
pub mod runtime;
pub mod select;
pub mod sessions;
pub mod suspend;
pub mod table;
pub mod terminal;
pub mod theme;
pub mod view;
pub mod views;

pub use app::App;
pub use ctx::AppCtx;
pub use view::{Hint, View, ViewId};
