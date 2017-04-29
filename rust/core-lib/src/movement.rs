// Copyright 2017 Google Inc. All rights reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Representation and calculation of movement within a view.

use std::cmp::max;

use selection::{Affinity, HorizPos, Selection, SelRegion};
use view::View;
use word_boundaries::WordCursor;
use xi_rope::rope::{LinesMetric, Rope};
use xi_rope::tree::Cursor;

/// The specification of a movement.
#[derive(Clone, Copy)]
pub enum Movement {
    /// Move to the left by one grapheme cluster.
    Left,
    /// Move to the right by one grapheme cluster.
    Right,
    /// Move to the left by one word.
    LeftWord,
    /// Move to the right by one word.
    RightWord,
    /// Move to left end of visible line.
    LeftOfLine,
    /// Move to right end of visible line.
    RightOfLine,
    /// Move up one visible line.
    Up,
    /// Move down one visible line.
    Down,
    /// Move up one viewport height.
    UpPage,
    /// Move down one viewport height.
    DownPage,
    /// Move to the start of the text line.
    StartOfParagraph,
    /// Move to the end of the text line.
    EndOfParagraph,
    /// Move to the start of the document.
    StartOfDocument,
    /// Move to the end of the document
    EndOfDocument,
}

/// Calculate a horizontal position in the view, based on the offset. Return
/// value has the same type as `region_movement` for convenience.
fn calc_horiz(view: &View, text: &Rope, offset: usize) -> (usize, Option<HorizPos>) {
    let (_line, col) = view.offset_to_line_col(text, offset);
    (offset, Some(col))
}

/// Compute movement based on vertical motion by the given number of lines.
///
/// Note: in non-exceptional cases, this function preserves the `horiz`
/// field of the selection region.
fn vertical_motion(r: &SelRegion, view: &View, text: &Rope, line_delta: isize,
    modify: bool) -> (usize, Option<HorizPos>)
{
    // The active point of the selection
    let active = if modify {
        r.end
    } else if line_delta < 0 {
        r.min()
    } else {
        r.max()
    };
    let col = if let Some(col) = r.horiz {
        col
    } else {
        view.offset_to_line_col(text, active).1
    };
    // This code is quite careful to avoid integer overflow.
    // TODO: write tests to verify
    let line = view.line_of_offset(text, active);
    if line_delta < 0 && (-line_delta as usize) > line {
        return (0, Some(col));
    }
    let line = if line_delta < 0 {
        line - (-line_delta as usize)
    } else {
        line.saturating_add(line_delta as usize)
    };
    let n_lines = view.line_of_offset(text, text.len());
    if line > n_lines {
        return (text.len(), Some(col));
    }
    let new_offset = view.line_col_to_offset(text, line, col);
    if new_offset == active {
        calc_horiz(view, text, new_offset)
    } else {
        (new_offset, Some(col))
    }
}

/// Computes the actual desired amount of scrolling (generally slightly
/// less than the height of the viewport, to allow overlap).
fn scroll_height(view: &View) -> isize {
    max(view.scroll_height() as isize - 2, 1)
}

/// Compute the result of movement on one selection region.

// Note: most of these calls to calc_horiz could be eliminated (just use
// None). That would cause the column to be calculated lazily on vertical
// motion, rather than eagerly.
fn region_movement(m: Movement, r: &SelRegion, view: &View, text: &Rope, modify: bool)
    -> (usize, Option<HorizPos>)
{
    match m {
        Movement::Left => {
            if r.is_caret() || modify {
                if let Some(offset) = text.prev_grapheme_offset(r.end) {
                    calc_horiz(view, text, offset)
                } else {
                    (0, r.horiz)
                }
            } else {
                calc_horiz(view, text, r.min())
            }
        }
        Movement::Right => {
            if r.is_caret() || modify {
                if let Some(offset) = text.next_grapheme_offset(r.end) {
                    calc_horiz(view, text, offset)
                } else {
                    (r.end, r.horiz)
                }
            } else {
                calc_horiz(view, text, r.max())
            }
        }
        Movement::LeftWord => {
            let mut word_cursor = WordCursor::new(text, r.end);
            let offset = word_cursor.prev_boundary().unwrap_or(0);
            calc_horiz(view, text, offset)
        }
        Movement::RightWord => {
            let mut word_cursor = WordCursor::new(text, r.end);
            let offset = word_cursor.next_boundary().unwrap_or_else(|| text.len());
            calc_horiz(view, text, offset)
        }
        Movement::LeftOfLine => {
            let line = view.line_of_offset(text, r.end);
            let offset = view.offset_of_line(text, line);
            calc_horiz(view, text, offset)
        }
        Movement::RightOfLine => {
            let line = view.line_of_offset(text, r.end);
            let mut offset = text.len();

            // calculate end of line
            let next_line_offset = view.offset_of_line(text, line + 1);
            if line < view.line_of_offset(text, offset) {
                if let Some(prev) = text.prev_grapheme_offset(next_line_offset) {
                    offset = prev;
                }
            }
            calc_horiz(view, text, offset)
        }
        Movement::Up => vertical_motion(r, view, text, -1, modify),
        Movement::Down => vertical_motion(r, view, text, 1, modify),
        Movement::StartOfParagraph => {
            // Note: TextEdit would start at modify ? r.end : r.min()
            let mut cursor = Cursor::new(&text, r.end);
            let offset = cursor.prev::<LinesMetric>().unwrap_or(0);
            calc_horiz(view, text, offset)
        }
        Movement::EndOfParagraph => {
            // Note: TextEdit would start at modify ? r.end : r.max()
            let mut offset = r.end;
            let mut cursor = Cursor::new(&text, offset);
            if let Some(next_para_offset) = cursor.next::<LinesMetric>() {
                if cursor.is_boundary::<LinesMetric>() {
                    if let Some(eol) = text.prev_grapheme_offset(next_para_offset) {
                        offset = eol;
                    }
                }
            }
            calc_horiz(view, text, offset)
        }
        Movement::UpPage => vertical_motion(r, view, text, -scroll_height(view), modify),
        Movement::DownPage => vertical_motion(r, view, text, scroll_height(view), modify),
        Movement::StartOfDocument => calc_horiz(view, text, 0),
        Movement::EndOfDocument => calc_horiz(view, text, text.len()),
    }
}

/// Compute a new selection by applying a movement to an existing selection.
///
/// In a multi-region selection, this function applies the movement to each
/// region in the selection, and returns the union of the results.
///
/// If `modify` is `true`, the selections are modified, otherwise the results
/// of individual region movements become carets.
pub fn selection_movement(m: Movement, s: &Selection, view: &View, text: &Rope,
    modify: bool) -> Selection
{
    let mut result = Selection::new();
    for r in s.iter() {
        let (offset, horiz) = region_movement(m, r, view, text, modify);
        let new_region = SelRegion {
            start: if modify { r.start } else { offset },
            end: offset,
            horiz: horiz,
            affinity: Affinity::default(),
        };
        result.add_region(new_region);
    }
    result
}
