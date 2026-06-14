//! Tab group data model. Gated at runtime by `FeatureFlag::GroupedTabs`.

use uuid::Uuid;
use warpui::elements::DraggableState;

use crate::tab::SelectedTabColor;

/// Stable identity for a tab group.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TabGroupId(pub Uuid);

impl TabGroupId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TabGroupId {
    fn default() -> Self {
        Self::new()
    }
}

/// A named group of tabs in the vertical tabs panel.
/// Member tabs reference their group via `TabData::group_id`.
#[derive(Clone)]
pub struct TabGroup {
    pub id: TabGroupId,
    pub name: Option<String>,
    pub color: SelectedTabColor,
    pub collapsed: bool,
    pub draggable_state: DraggableState,
    /// True when this whole group is pinned to the front of the tab list.
    pub pinned: bool,
    /// Stable slug of the Cockpit mission hosted in this group, if any. The
    /// `id` is regenerated on every session restore, so this durable slug is
    /// what lets a restored mission re-find its live group.
    pub mission_slug: Option<String>,
}

impl TabGroup {
    /// Creates a new, untitled, expanded tab group with a fresh id.
    pub fn new() -> Self {
        Self {
            id: TabGroupId::new(),
            name: None,
            color: SelectedTabColor::default(),
            collapsed: false,
            draggable_state: Default::default(),
            pinned: false,
            mission_slug: None,
        }
    }
}

impl Default for TabGroup {
    fn default() -> Self {
        Self::new()
    }
}
