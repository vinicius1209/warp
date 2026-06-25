//! Spec acceptance-criteria parsing for Cockpit Missions (SDD traceability).
//!
//! A Spec-Driven mission's Arquiteto stage writes a `spec.md` whose testable
//! acceptance criteria are expressed as a GitHub-style task list (`- [ ]` /
//! `- [x]`). This module parses those criteria so the rest of the app can
//! surface mission progress as "N/M criteria met" — in Mission Control, the
//! footer mission chip, and the between-stages gate.
//!
//! Parsing is intentionally forgiving and dependency-free: it scans lines for
//! task-list items and ignores everything else, so an unconventional spec never
//! breaks the UI (it just reports zero criteria). All file IO is non-fatal — a
//! missing or unreadable spec yields an empty list — mirroring the "failures are
//! logged, never fatal" stance of the rest of `missions`.

use std::path::Path;

/// The conventional spec filename a Spec-Driven mission writes, relative to the
/// mission dir. Kept here so callers don't hardcode it.
pub const SPEC_FILE_NAME: &str = "spec.md";

/// One acceptance criterion parsed from a spec's task list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Criterion {
    /// The criterion text, with the `- [ ] ` marker stripped and trimmed.
    pub text: String,
    /// Whether the box was checked (`- [x]`, case-insensitive).
    pub met: bool,
}

/// A roll-up of a spec's acceptance criteria.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CriteriaProgress {
    /// Number of criteria checked off.
    pub met: usize,
    /// Total number of criteria found.
    pub total: usize,
}

impl CriteriaProgress {
    /// `true` when there is at least one criterion and all are met. Used to
    /// decide whether a stage's spec is fully satisfied (an empty spec is never
    /// "complete", so it never short-circuits a gate).
    pub fn is_complete(self) -> bool {
        self.total > 0 && self.met == self.total
    }

    /// `true` when the spec declared no criteria at all. Distinguishes "0/0"
    /// (no checklist authored) from "0/5" (authored but none met), so callers
    /// can hide the progress chip rather than show a misleading "0/0".
    pub fn is_empty(self) -> bool {
        self.total == 0
    }
}

/// Parses GitHub-style task-list items from `markdown` into acceptance criteria,
/// in document order. A line is a criterion when, after optional leading
/// whitespace, it starts with a bullet marker (`-`, `*`, or `+`), a space, and a
/// `[ ]` / `[x]` / `[X]` checkbox. Everything else is ignored.
pub fn parse_criteria(markdown: &str) -> Vec<Criterion> {
    markdown.lines().filter_map(parse_criterion_line).collect()
}

fn parse_criterion_line(line: &str) -> Option<Criterion> {
    // Bullet marker (`-`, `*`, `+`) followed by at least one space. GitHub
    // requires the space, so `-[ ]` is intentionally not a task item.
    let rest = line.trim_start();
    let rest = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))?
        .trim_start();

    // Checkbox immediately after the bullet: "[ ]" (unmet) or "[x]"/"[X]" (met).
    let (met, after) = if let Some(after) = rest.strip_prefix("[ ]") {
        (false, after)
    } else if let Some(after) = rest.strip_prefix("[x]").or_else(|| rest.strip_prefix("[X]")) {
        (true, after)
    } else {
        return None;
    };

    Some(Criterion {
        text: after.trim().to_string(),
        met,
    })
}

/// Rolls a parsed criteria list up into a [`CriteriaProgress`].
pub fn summarize(criteria: &[Criterion]) -> CriteriaProgress {
    CriteriaProgress {
        met: criteria.iter().filter(|criterion| criterion.met).count(),
        total: criteria.len(),
    }
}

/// Reads and parses acceptance criteria from `<mission_dir>/spec.md`. A missing
/// spec yields an empty list silently (an absent spec is the normal state before
/// the Arquiteto stage runs); any other read error is logged and also yields an
/// empty list.
pub fn read_criteria(mission_dir: &Path) -> Vec<Criterion> {
    let path = mission_dir.join(SPEC_FILE_NAME);
    match std::fs::read_to_string(&path) {
        Ok(contents) => parse_criteria(&contents),
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                log::warn!("Failed to read mission spec {path:?}: {err:?}");
            }
            Vec::new()
        }
    }
}

/// Convenience: the [`CriteriaProgress`] for `<mission_dir>/spec.md`.
pub fn read_progress(mission_dir: &Path) -> CriteriaProgress {
    summarize(&read_criteria(mission_dir))
}

/// Reads `<mission_dir>/spec.md` as a human-readable preview, truncated to at
/// most `max_lines` lines (a trailing `… (truncated)` line is appended when the
/// spec is longer). Returns `None` when there is no non-empty spec to show, so
/// the gate can omit the preview rather than render an empty box.
pub fn read_spec_excerpt(mission_dir: &Path, max_lines: usize) -> Option<String> {
    let path = mission_dir.join(SPEC_FILE_NAME);
    let contents = std::fs::read_to_string(&path).ok()?;
    if contents.trim().is_empty() {
        return None;
    }
    let total = contents.lines().count();
    let mut preview: Vec<&str> = contents.lines().take(max_lines).collect();
    if total > max_lines {
        preview.push("… (truncated)");
    }
    Some(preview.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checked_and_unchecked_items() {
        let criteria = parse_criteria("- [ ] first\n- [x] second\n- [X] third");
        assert_eq!(
            criteria,
            vec![
                Criterion {
                    text: "first".to_string(),
                    met: false
                },
                Criterion {
                    text: "second".to_string(),
                    met: true
                },
                Criterion {
                    text: "third".to_string(),
                    met: true
                },
            ]
        );
    }

    #[test]
    fn accepts_all_bullet_markers_and_nesting() {
        // `-`, `*`, `+`, and indented (nested) items all count.
        let criteria = parse_criteria("- [ ] dash\n* [x] star\n+ [ ] plus\n    - [x] nested");
        assert_eq!(criteria.len(), 4);
        assert_eq!(summarize(&criteria), CriteriaProgress { met: 2, total: 4 });
    }

    #[test]
    fn ignores_non_task_lines() {
        // Plain bullets, prose, headings, and a checkbox with no bullet are all
        // ignored — only real task-list items are criteria.
        let criteria = parse_criteria(
            "# Objetivo\n\
             prose line\n\
             - just a bullet\n\
             [ ] no bullet marker\n\
             -[ ] no space after dash\n\
             - [ ] real one",
        );
        assert_eq!(
            criteria,
            vec![Criterion {
                text: "real one".to_string(),
                met: false
            }]
        );
    }

    #[test]
    fn keeps_empty_criterion_text() {
        // GitHub renders an empty checkbox; we keep it (text is just empty).
        let criteria = parse_criteria("- [ ] ");
        assert_eq!(criteria.len(), 1);
        assert_eq!(criteria[0].text, "");
    }

    #[test]
    fn empty_input_yields_no_criteria() {
        assert!(parse_criteria("").is_empty());
        assert_eq!(summarize(&[]), CriteriaProgress { met: 0, total: 0 });
    }

    #[test]
    fn progress_completion_and_emptiness() {
        assert!(!CriteriaProgress { met: 0, total: 0 }.is_complete());
        assert!(CriteriaProgress { met: 0, total: 0 }.is_empty());
        assert!(!CriteriaProgress { met: 2, total: 5 }.is_complete());
        assert!(!CriteriaProgress { met: 2, total: 5 }.is_empty());
        assert!(CriteriaProgress { met: 5, total: 5 }.is_complete());
    }

    #[test]
    fn read_spec_excerpt_truncates_and_handles_absence() {
        let dir = tempfile::tempdir().unwrap();
        // No spec -> None (gate omits the preview).
        assert!(read_spec_excerpt(dir.path(), 10).is_none());

        // Short spec -> returned verbatim, no truncation marker.
        std::fs::write(dir.path().join(SPEC_FILE_NAME), "line 1\nline 2\n").unwrap();
        let excerpt = read_spec_excerpt(dir.path(), 10).unwrap();
        assert_eq!(excerpt, "line 1\nline 2");
        assert!(!excerpt.contains("truncated"));

        // Long spec -> truncated to max_lines plus a marker line.
        let long = (1..=20)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(dir.path().join(SPEC_FILE_NAME), long).unwrap();
        let excerpt = read_spec_excerpt(dir.path(), 5).unwrap();
        assert_eq!(excerpt.lines().count(), 6);
        assert!(excerpt.contains("… (truncated)"));
    }

    #[test]
    fn read_spec_excerpt_treats_whitespace_only_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SPEC_FILE_NAME), "   \n\n").unwrap();
        assert!(read_spec_excerpt(dir.path(), 10).is_none());
    }

    #[test]
    fn read_progress_handles_missing_and_present_spec() {
        let dir = tempfile::tempdir().unwrap();
        // No spec.md yet -> empty, no panic.
        assert_eq!(read_progress(dir.path()), CriteriaProgress::default());

        std::fs::write(
            dir.path().join(SPEC_FILE_NAME),
            "# Critérios de aceite\n- [x] done\n- [ ] todo\n",
        )
        .unwrap();
        assert_eq!(read_progress(dir.path()), CriteriaProgress { met: 1, total: 2 });
    }
}
