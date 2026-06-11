//! GUI-backend platform items.

use std::ops::Range;

use super::LineStyle;
use crate::text_layout::{ClipConfig, Line, StyleAndFont, TextAlignment, TextFrame};

/// Trait that implements text layout. Implementors must be [`Send`] and
/// [`Sync`] so that text can be laid out in a background thread.
pub trait TextLayoutSystem: 'static + Send + Sync {
    /// Lays out a single line of text.
    fn layout_line(
        &self,
        text: &str,
        line_style: LineStyle,
        style_runs: &[(Range<usize>, StyleAndFont)],
        max_width: f32,
        clip_config: ClipConfig,
    ) -> Line;

    /// Lays out text into a series of lines that fit within the bounding box
    /// defined by `max_width` and `max_height`.
    #[allow(clippy::too_many_arguments)]
    fn layout_text(
        &self,
        text: &str,
        line_style: LineStyle,
        style_runs: &[(Range<usize>, StyleAndFont)],
        max_width: f32,
        max_height: f32,
        alignment: TextAlignment,
        first_line_head_indent: Option<f32>,
    ) -> TextFrame;
}
