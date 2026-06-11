//! The GUI [`Backend`] instantiation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::{Backend, GuiBackend};
use crate::presenter::PositionCache;
use crate::{Element, Presenter, WindowId};

impl Backend for GuiBackend {
    type RenderOutput = Box<dyn Element>;
    type Presenter = GuiPresenterState;
}

/// Presentation state for the GUI backend, stored on `AppContextImpl<GuiBackend>`
/// as `B::Presenter`. Holds the GUI presenter collection plus the position
/// cache, so the generic core holds only an opaque `B::Presenter` while GUI
/// method signatures that touch this state are unchanged.
///
/// The backend-neutral window-invalidation bookkeeping
/// (`window_invalidations` / `invalidation_callbacks`) lives directly on
/// [`AppContextImpl<B>`](crate::AppContextImpl) so both backends share it.
#[derive(Default)]
pub struct GuiPresenterState {
    pub(crate) presenters: HashMap<WindowId, Rc<RefCell<Presenter>>>,
    pub(crate) last_frame_position_cache: HashMap<WindowId, PositionCache>,
}
