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
