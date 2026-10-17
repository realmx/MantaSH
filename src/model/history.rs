//! Stable command identities and two logical lists sharing one bounded MRU order.
use super::{HistoryScope, Id};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const HISTORY_LIMIT: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryItem {
    pub id: Id,
    pub command: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryCatalog {
    pub items: Vec<HistoryItem>,
    pub local_ids: Vec<Id>,
    pub ssh_ids: Vec<Id>,
}
impl HistoryCatalog {
    pub fn members(&self, scope: HistoryScope) -> &[Id] {
        match scope {
            HistoryScope::Local => &self.local_ids,
            HistoryScope::Ssh => &self.ssh_ids,
        }
    }
    pub fn list(&self, scope: HistoryScope) -> impl Iterator<Item = &HistoryItem> {
        let ids: HashSet<_> = self.members(scope).iter().copied().collect();
        self.items.iter().filter(move |item| ids.contains(&item.id))
    }
    /// Only reliable execution reports call this; browsing never changes MRU order.
    pub fn record(&mut self, scope: HistoryScope, command: &str) -> Option<Id> {
        let command = command.trim();
        if command.is_empty() {
            return None;
        }
        let found = self
            .items
            .iter()
            .position(|item| item.command == command && self.members(scope).contains(&item.id));
        let item = found
            .map(|index| self.items.remove(index))
            .unwrap_or_else(|| {
                let id = Id::new_v4();
                match scope {
                    HistoryScope::Local => &mut self.local_ids,
                    HistoryScope::Ssh => &mut self.ssh_ids,
                }
                .push(id);
                HistoryItem {
                    id,
                    command: command.to_string(),
                }
            });
        let id = item.id;
        self.items.insert(0, item);
        self.items.truncate(HISTORY_LIMIT);
        self.prune_members();
        Some(id)
    }
    pub fn delete(&mut self, ids: &[Id]) {
        let ids: HashSet<_> = ids.iter().copied().collect();
        self.items.retain(|item| !ids.contains(&item.id));
        self.prune_members();
    }
    fn prune_members(&mut self) {
        let ids: HashSet<_> = self.items.iter().map(|item| item.id).collect();
        self.local_ids.retain(|id| ids.contains(id));
        self.ssh_ids.retain(|id| ids.contains(id));
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        use anyhow::ensure;
        ensure!(
            self.items.len() <= HISTORY_LIMIT,
            "History capacity exceeded"
        );
        let ids: HashSet<_> = self.items.iter().map(|item| item.id).collect();
        ensure!(ids.len() == self.items.len(), "Duplicate history identity");
        let mut assigned = HashSet::new();
        for scope in [HistoryScope::Local, HistoryScope::Ssh] {
            let mut commands = HashSet::new();
            for id in self.members(scope) {
                ensure!(
                    ids.contains(id) && assigned.insert(*id),
                    "Invalid history membership"
                );
            }
            for item in self.list(scope) {
                ensure!(
                    !item.command.is_empty() && item.command.trim() == item.command,
                    "Invalid history command"
                );
                ensure!(commands.insert(&item.command), "Duplicate history command");
            }
        }
        ensure!(ids == assigned, "Missing history membership");
        Ok(())
    }
}
