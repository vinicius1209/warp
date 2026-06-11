//! Backend router for the `elements` module. The module path is ungated and
//! stable; its contents are backend-routed. M8 adds the `tui` arm
//! (`elements/tui/`) when the TUI element library is absorbed into this crate.
#[cfg(not(feature = "tui"))]
mod gui;

#[cfg(not(feature = "tui"))]
pub use gui::*;
