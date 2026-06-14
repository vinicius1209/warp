//! Pure restore reconciliation: re-resolve each active mission's live tab
//! group after a session restore.
//!
//! Session restore mints fresh `TabGroupId`s (see `read_app_state` in
//! `persistence/sqlite.rs`), so a mission's persisted `group_id` never matches
//! a live group after a restart. The stable `slug` is mirrored onto the tab
//! group (`TabGroup::mission_slug`), so the live group can be re-found by slug.
//!
//! This module holds the matching logic as a free function so it can be unit
//! tested without a live `Workspace`/`ModelContext`.

use std::collections::HashMap;

use crate::workspace::tab_group::TabGroupId;

/// Computes the `slug -> live group id` rebindings for restore reconciliation.
///
/// `mission_slugs` is the set of active missions' stable slugs; `groups` is the
/// live tab groups, each carrying the `mission_slug` it was created for (the
/// fresh `TabGroupId` minted on restore plus the slug persisted on the group).
/// For every mission slug that matches exactly one live group, the result maps
/// that slug to the group's current id. Slugs with zero matching groups are
/// omitted (the mission keeps its stale `group_id` and is lazily rebound later
/// via `regroup_resumed_mission_tab`); when several groups claim the same slug
/// the first wins, mirroring `find_by_group`'s tolerance of duplicates.
pub fn reconcile_mission_groups<'a>(
    mission_slugs: impl IntoIterator<Item = &'a str>,
    groups: impl IntoIterator<Item = (TabGroupId, Option<&'a str>)>,
) -> HashMap<String, TabGroupId> {
    // Index live groups by the slug they were created for, keeping the first
    // group per slug so the result is deterministic across HashMap iteration.
    let mut group_by_slug: HashMap<&str, TabGroupId> = HashMap::new();
    for (group_id, mission_slug) in groups {
        if let Some(slug) = mission_slug {
            group_by_slug.entry(slug).or_insert(group_id);
        }
    }
    mission_slugs
        .into_iter()
        .filter_map(|slug| {
            group_by_slug
                .get(slug)
                .map(|group_id| (slug.to_string(), *group_id))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rebinds_slug_to_fresh_group_id() {
        // Simulates a session restore: the mission's old group id is gone and a
        // fresh one was minted for the group carrying the same slug.
        let fresh = TabGroupId::new();
        let map = reconcile_mission_groups(
            ["spec-driven-20260612-120000"],
            [(fresh, Some("spec-driven-20260612-120000"))],
        );
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("spec-driven-20260612-120000"), Some(&fresh));
    }

    #[test]
    fn test_unmatched_mission_is_omitted() {
        let other = TabGroupId::new();
        let map = reconcile_mission_groups(
            ["solo-20260612-130000"],
            [
                (other, Some("spec-driven-20260612-120000")),
                (TabGroupId::new(), None),
            ],
        );
        assert!(map.is_empty());
    }

    #[test]
    fn test_two_missions_bind_to_their_own_groups() {
        // The original CRITICAL bug: two same-template missions must not share a
        // group. Distinct slugs keep them apart even with identical group names.
        let group_a = TabGroupId::new();
        let group_b = TabGroupId::new();
        let map = reconcile_mission_groups(
            ["spec-driven-20260612-120000", "spec-driven-20260612-150000"],
            [
                (group_a, Some("spec-driven-20260612-120000")),
                (group_b, Some("spec-driven-20260612-150000")),
            ],
        );
        assert_eq!(map.get("spec-driven-20260612-120000"), Some(&group_a));
        assert_eq!(map.get("spec-driven-20260612-150000"), Some(&group_b));
    }

    #[test]
    fn test_groups_without_slug_are_ignored() {
        let mission_group = TabGroupId::new();
        let map = reconcile_mission_groups(
            ["solo-20260612-130000"],
            [
                (TabGroupId::new(), None),
                (mission_group, Some("solo-20260612-130000")),
                (TabGroupId::new(), None),
            ],
        );
        assert_eq!(map.get("solo-20260612-130000"), Some(&mission_group));
    }

    #[test]
    fn test_duplicate_slug_groups_take_the_first() {
        let first = TabGroupId::new();
        let second = TabGroupId::new();
        let map = reconcile_mission_groups(
            ["solo-20260612-130000"],
            [
                (first, Some("solo-20260612-130000")),
                (second, Some("solo-20260612-130000")),
            ],
        );
        assert_eq!(map.get("solo-20260612-130000"), Some(&first));
    }
}
