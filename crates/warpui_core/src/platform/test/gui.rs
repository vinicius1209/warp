//! GUI-only pieces of the test platform delegate.

use super::FontDB;
use crate::platform;
use crate::text_layout::TextAlignment;

impl platform::TextLayoutSystem for FontDB {
    fn layout_line(
        &self,
        _text: &str,
        line_style: platform::LineStyle,
        _style_runs: &[(std::ops::Range<usize>, crate::text_layout::StyleAndFont)],
        _max_width: f32,
        _clip_config: crate::text_layout::ClipConfig,
    ) -> crate::text_layout::Line {
        crate::text_layout::Line::empty(line_style.font_size, line_style.line_height_ratio, 0)
    }

    fn layout_text(
        &self,
        _text: &str,
        line_style: platform::LineStyle,
        _style_runs: &[(std::ops::Range<usize>, crate::text_layout::StyleAndFont)],
        _max_width: f32,
        _max_height: f32,
        _alignment: TextAlignment,
        _first_line_head_indent: Option<f32>,
    ) -> crate::text_layout::TextFrame {
        crate::text_layout::TextFrame::empty(line_style.font_size, line_style.line_height_ratio)
    }
}
