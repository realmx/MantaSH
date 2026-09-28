//! File row selection uses stable paths and the current visible order, never stale row numbers.
use std::collections::HashSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowSelection {
    paths: HashSet<String>,
    anchor: Option<String>,
    lead: Option<String>,
}

impl RowSelection {
    /// Number of selected entries available to batch actions.
    pub fn len(&self) -> usize {
        self.paths.len()
    }
    /// Whether batch actions have no targets.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
    /// Match selection by full path within its owning file view.
    pub fn contains(&self, path: &str) -> bool {
        self.paths.contains(path)
    }
    /// Iterate selected paths; callers needing display order should filter visible entries.
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.paths.iter()
    }
    /// The last ordinary click remains the pivot of repeated Shift ranges.
    pub fn anchor(&self) -> Option<&str> {
        self.anchor.as_deref()
    }
    /// Last clicked range endpoint, used by keyboard navigation without moving the pivot.
    pub fn lead(&self) -> Option<&str> {
        self.lead.as_deref()
    }
    /// Navigation and explicit clearing reset both selection and its range pivot.
    pub fn clear(&mut self) {
        self.paths.clear();
        self.anchor = None;
        self.lead = None;
    }
    /// Remove entries and anchors excluded by a visibility change.
    pub fn retain_visible(&mut self, visible: &[String]) {
        let valid: HashSet<_> = visible.iter().map(String::as_str).collect();
        self.paths.retain(|path| valid.contains(path.as_str()));
        if self
            .anchor
            .as_deref()
            .is_some_and(|path| !valid.contains(path))
        {
            self.anchor = None;
        }
        if self
            .lead
            .as_deref()
            .is_some_and(|path| !valid.contains(path))
        {
            self.lead = None;
        }
    }
    /// Select only displayed entries, with the first row as a predictable range pivot.
    pub fn select_all(&mut self, visible: &[String]) {
        self.paths = visible.iter().cloned().collect();
        self.anchor = visible.first().cloned();
        self.lead = self.anchor.clone();
    }
    /// Plain click replaces selection, additive click toggles, Shift selects an inclusive range.
    pub fn click(&mut self, visible: &[String], path: &str, range: bool, additive: bool) -> bool {
        let Some(index) = visible.iter().position(|item| item == path) else {
            return false;
        };
        self.retain_visible(visible);
        self.lead = Some(path.to_owned());
        if range {
            let start = self
                .anchor
                .as_ref()
                .and_then(|anchor| visible.iter().position(|item| item == anchor))
                .unwrap_or_else(|| {
                    self.anchor = Some(path.to_owned());
                    index
                });
            if !additive {
                self.paths.clear();
            }
            self.paths
                .extend(visible[start.min(index)..=start.max(index)].iter().cloned());
        } else {
            if !additive {
                self.paths.clear();
            }
            if !self.paths.insert(path.to_owned()) {
                self.paths.remove(path);
            }
            self.anchor = Some(path.to_owned());
        }
        true
    }
}
