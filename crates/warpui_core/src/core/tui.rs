//! TUI-backend-specific `App`/`AppContext` API.
//!
//! Since the view-trait unification, all view registration/access lives in the
//! shared `app.rs`; this module holds only the TUI halves of the per-backend
//! window plumbing.

use super::{AddWindowOptions, AppContext};
use crate::{EntityId, WindowId};

impl AppContext {
    /// Neutral (TUI) window creation: the backend-agnostic subset of
    /// [`Self::insert_window_internal`]'s GUI counterpart — window-id + bounds
    /// bookkeeping, root-view construction, and focus. No presenter, platform
    /// window, or scene/event callbacks (the TUI runtime owns those).
    pub(super) fn insert_window_internal<F>(
        &mut self,
        window_id: Option<WindowId>,
        add_window_options: AddWindowOptions,
        build_window_data: F,
    ) -> (WindowId, EntityId)
    where
        F: FnOnce(WindowId, &mut AppContext) -> EntityId,
    {
        let AddWindowOptions {
            window_bounds,
            anchor_new_windows_from_closed_position,
            ..
        } = add_window_options;

        let window_id = window_id.unwrap_or_else(WindowId::new);

        // Store the window bounds before creating the root view, in case it uses
        // this value.
        self.window_bounds.insert(window_id, window_bounds.bounds());
        self.next_window_bounds_map
            .insert(window_id, anchor_new_windows_from_closed_position);
        self.next_window_bounds = None;

        let root_view_id = build_window_data(window_id, self);
        self.focus(window_id, root_view_id);

        (window_id, root_view_id)
    }

    /// The TUI backend keeps no per-window presentation state in the core; the
    /// `warpui_tui` runtime owns the presenter. No-op counterpart of the GUI
    /// version.
    pub(super) fn drop_window_presentation(&mut self, _window_id: WindowId) {}
}
