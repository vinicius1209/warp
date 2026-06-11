//! The TUI [`Backend`] instantiation, gated behind the `tui` feature.
//!
//! `TuiBackend` keeps its render output abstract (`Box<dyn Any>`) so
//! `warpui_core` never names a concrete TUI element type and therefore never
//! depends on the downstream `warpui_tui` crate. The concrete element/buffer
//! types are defined in `warpui_tui` and recovered by downcasting this erased
//! output.

use std::any::Any;

use super::Backend;

/// The TUI backend marker.
pub struct TuiBackend;

/// Neutral, empty presentation state for the TUI backend. It names no GUI type,
/// which is what lets the GUI render cluster (presenter/scene/text_layout/
/// rendering) be fully excluded under `--features tui`. The backend-neutral
/// window-invalidation bookkeeping lives directly on
/// [`AppContextImpl<B>`](crate::AppContextImpl); the real TUI presentation layer
/// is provided by `warpui_tui`'s `TuiRuntime`, not by this core loop.
#[derive(Default)]
pub struct TuiPresenterState;

impl Backend for TuiBackend {
    /// Abstract: the concrete TUI element/buffer type lives in `warpui_tui` and
    /// is recovered by downcasting. Keeping this `Box<dyn Any>` is what prevents
    /// a `warpui_core -> warpui_tui` dependency cycle.
    type RenderOutput = Box<dyn Any>;

    type Presenter = TuiPresenterState;
}
