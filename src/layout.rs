//! Persistent local pane trees. Display order, focus and ownership never depend on tab labels.
use crate::model::{Id, Split};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_LOCAL_PANES: usize = 5;
/// Minimum logical width of the SSH tools sidebar when no user preference exists.
pub const TOOL_MIN_WIDTH: f32 = 280.;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneLayout {
    Pane {
        pane: Id,
    },
    Split {
        id: Id,
        axis: Split,
        ratio: f32,
        first: Box<PaneLayout>,
        second: Box<PaneLayout>,
    },
}
impl PaneLayout {
    pub fn single(pane: Id) -> Self {
        Self::Pane { pane }
    }
    /// Return leaves in stable visual order.
    pub fn panes(&self) -> Vec<Id> {
        match self {
            Self::Pane { pane } => vec![*pane],
            Self::Split { first, second, .. } => {
                let mut ids = first.panes();
                ids.extend(second.panes());
                ids
            }
        }
    }
    /// Validate that the tree describes every pane exactly once, repairing only usable ratios.
    pub fn normalize(&mut self, expected: &[Id]) -> bool {
        let mut splits = HashSet::new();
        if !self.repair(&mut splits, 0) {
            return false;
        }
        let ids = self.panes();
        let unique: HashSet<_> = ids.iter().copied().collect();
        ids.len() == expected.len()
            && unique.len() == ids.len()
            && expected.iter().all(|id| unique.contains(id))
    }
    fn repair(&mut self, splits: &mut HashSet<Id>, depth: usize) -> bool {
        if depth > MAX_LOCAL_PANES {
            return false;
        }
        match self {
            Self::Pane { .. } => true,
            Self::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                if !splits.insert(*id) {
                    *id = Id::new_v4();
                    splits.insert(*id);
                }
                *ratio = if ratio.is_finite() {
                    ratio.clamp(0.1, 0.9)
                } else {
                    0.5
                };
                first.repair(splits, depth + 1) && second.repair(splits, depth + 1)
            }
        }
    }
    /// Split one existing leaf; reject caps and duplicate identities at the model boundary.
    pub fn split(&mut self, target: Id, added: Id, axis: Split) -> bool {
        let ids = self.panes();
        if ids.len() >= MAX_LOCAL_PANES || ids.contains(&added) || !ids.contains(&target) {
            return false;
        }
        self.insert(target, added, axis)
    }
    fn insert(&mut self, target: Id, added: Id, axis: Split) -> bool {
        match self {
            Self::Pane { pane } if *pane == target => {
                *self = Self::Split {
                    id: Id::new_v4(),
                    axis,
                    ratio: 0.5,
                    first: Box::new(Self::single(target)),
                    second: Box::new(Self::single(added)),
                };
                true
            }
            Self::Pane { .. } => false,
            Self::Split { first, second, .. } => {
                first.insert(target, added, axis) || second.insert(target, added, axis)
            }
        }
    }
    /// Remove a closed leaf and collapse empty branches while preserving the other leaves.
    pub fn without(self, target: Id) -> Option<Self> {
        match self {
            Self::Pane { pane } => (pane != target).then_some(Self::single(pane)),
            Self::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => match (first.without(target), second.without(target)) {
                (Some(a), Some(b)) => Some(Self::Split {
                    id,
                    axis,
                    ratio,
                    first: Box::new(a),
                    second: Box::new(b),
                }),
                (a, b) => a.or(b),
            },
        }
    }
    pub fn set_ratio(&mut self, target: Id, value: f32) -> bool {
        match self {
            Self::Pane { .. } => false,
            Self::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                if *id == target {
                    *ratio = if value.is_finite() {
                        value.clamp(0.1, 0.9)
                    } else {
                        0.5
                    };
                    true
                } else {
                    first.set_ratio(target, value) || second.set_ratio(target, value)
                }
            }
        }
    }
}

/// The window clamp is a display constraint, never the value persisted as the preference.
pub fn tool_width(preferred: Option<f32>, window_width: f32, work_width: f32) -> f32 {
    let maximum = (window_width / 2.).floor().min((work_width - 6.).max(0.));
    let minimum = TOOL_MIN_WIDTH.min(maximum);
    preferred
        .filter(|v| v.is_finite())
        .unwrap_or(minimum)
        .clamp(minimum, maximum)
}

/// Fallback advance, in ems, when the UI font cannot report a glyph. Calibrated on the wide
/// side against the macOS system UI font at 14px, where `2024-09-29 23:09:01` renders about
/// 142px wide (≈0.6em digits); the real file list measures glyphs and rarely needs this.
pub fn fallback_char_em(ch: char) -> f32 {
    match ch {
        '0'..='9' => 0.6,
        '-' | ':' | '.' | '/' => 0.4,
        ' ' => 0.3,
        c if c.is_ascii() => 0.6,
        _ => 1.,
    }
}

/// Single-line width of `text` from the font's per-character advances.
///
/// `advance` returns the measured advance in pixels, or `None` when the glyph is unavailable,
/// in which case [`fallback_char_em`] is used. Every `'0'` in `template` stands for any digit
/// and takes the widest measured digit, so proportional digit fonts never overflow the result.
pub fn template_text_width(
    template: &str,
    font_size: f32,
    mut advance: impl FnMut(char) -> Option<f32>,
) -> f32 {
    let mut measure = |ch: char| {
        advance(ch)
            .filter(|width| width.is_finite() && *width >= 0.)
            .unwrap_or(fallback_char_em(ch) * font_size)
    };
    let digit = ('0'..='9').map(&mut measure).fold(0., f32::max);
    template
        .chars()
        .map(|ch| if ch == '0' { digit } else { measure(ch) })
        .sum()
}

/// Width of a one-line column showing `template`-shaped values under `header`, plus `margin`.
///
/// `header_extra` is fixed chrome beside the header label (e.g. a sort arrow and its gap);
/// it widens the column only when the decorated header is wider than the values.
pub fn fitted_column_width(
    template: &str,
    header: &str,
    header_extra: f32,
    font_size: f32,
    margin: f32,
    mut advance: impl FnMut(char) -> Option<f32>,
) -> f32 {
    let values = template_text_width(template, font_size, &mut advance);
    // Labels contain literal characters only; a `'0'` there is never a digit slot.
    let label: f32 = header
        .chars()
        .map(|ch| {
            advance(ch)
                .filter(|width| width.is_finite() && *width >= 0.)
                .unwrap_or(fallback_char_em(ch) * font_size)
        })
        .sum::<f32>()
        + header_extra;
    values.max(label) + margin
}
