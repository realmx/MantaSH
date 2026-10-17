//! One FIFO for application metadata. Receipts describe committed state, never optimistic state.
use crate::{
    connections::{self, ImportPreview, ImportResult, PasswordRetry},
    events::Event,
    model::*,
    storage::Database,
    vault::LocalVault,
};
use anyhow::{Result, ensure};
use parking_lot::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};

pub enum Mutation {
    Record(HistoryScope, String),
    DeleteHistory(Vec<Id>),
    SaveProfile {
        profile: Profile,
        existing: bool,
    },
    DeleteProfiles(Vec<Id>),
    ReorderProfiles(Vec<Id>),
    SortProfiles,
    /// Whole-list replacement is restricted to isolated fixture/bootstrap callers.
    ReplaceProfiles(Vec<Profile>),
    Import {
        preview: ImportPreview,
        replace: bool,
    },
    RetryPasswords(PasswordRetry),
    Workspace(Preferences, Workspace),
}
pub enum Applied {
    History(HistoryCatalog),
    Profiles {
        profiles: Vec<Profile>,
        forget_failed: usize,
    },
    Import(ImportResult),
    Workspace,
}
pub struct Receipt {
    pub request: Id,
    pub revision: u64,
    pub result: std::result::Result<Applied, String>,
}
struct Work {
    request: Id,
    mutation: Mutation,
    done: Option<tokio::sync::oneshot::Sender<std::result::Result<(), String>>>,
}
#[derive(Clone)]
pub(super) struct Queue {
    sender: mpsc::Sender<Work>,
    events: async_channel::Sender<Event>,
    revision: Arc<AtomicU64>,
}
impl Queue {
    pub(super) fn start(
        db: Arc<Mutex<Database>>,
        vault: Arc<LocalVault>,
        events: async_channel::Sender<Event>,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::channel::<Work>();
        let output = events.clone();
        let revision = Arc::new(AtomicU64::new(0));
        let worker_revision = revision.clone();
        std::thread::Builder::new()
            .name("mantash-metadata".into())
            .spawn(move || {
                for work in receiver {
                    let result = apply(&mut db.lock(), &vault, work.mutation)
                        .map_err(|error| error.to_string());
                    let completed = result.as_ref().map(|_| ()).map_err(Clone::clone);
                    let _ = output.try_send(Event::Metadata(Receipt {
                        request: work.request,
                        revision: worker_revision.fetch_add(1, Ordering::SeqCst) + 1,
                        result,
                    }));
                    if let Some(done) = work.done {
                        let _ = done.send(completed);
                    }
                }
            })?;
        Ok(Self {
            sender,
            events,
            revision,
        })
    }
    pub(super) fn submit(&self, mutation: Mutation) -> Id {
        let request = Id::new_v4();
        if self
            .sender
            .send(Work {
                request,
                mutation,
                done: None,
            })
            .is_err()
        {
            let _ = self.events.try_send(Event::Metadata(Receipt {
                request,
                revision: self.revision.fetch_add(1, Ordering::SeqCst) + 1,
                result: Err("Metadata worker is unavailable".into()),
            }));
        }
        request
    }
    pub(super) fn barrier(
        &self,
        prefs: Preferences,
        workspace: Workspace,
    ) -> tokio::sync::oneshot::Receiver<std::result::Result<(), String>> {
        let (done, receiver) = tokio::sync::oneshot::channel();
        let _ = self.sender.send(Work {
            request: Id::new_v4(),
            mutation: Mutation::Workspace(prefs, workspace),
            done: Some(done),
        });
        receiver
    }
}

fn apply(db: &mut Database, vault: &LocalVault, mutation: Mutation) -> Result<Applied> {
    let mut forget_failed = 0;
    match mutation {
        Mutation::Record(scope, command) => {
            return Ok(Applied::History(db.record_history(scope, &command)?));
        }
        Mutation::DeleteHistory(ids) => return Ok(Applied::History(db.delete_history(&ids)?)),
        Mutation::SaveProfile { profile, existing } => {
            let mut profiles = db.profiles()?;
            if let Some(index) = profiles.iter().position(|p| p.id == profile.id) {
                profiles[index] = profile;
            } else {
                ensure!(!existing, "Connection was removed before this save");
                profiles.push(profile);
            }
            db.save_profiles(&profiles)?;
        }
        Mutation::DeleteProfiles(ids) => {
            let mut profiles = db.profiles()?;
            let removed: Vec<_> = profiles
                .iter()
                .filter(|p| ids.contains(&p.id))
                .map(|p| p.id)
                .collect();
            profiles.retain(|p| !ids.contains(&p.id));
            db.save_profiles(&profiles)?;
            for id in removed {
                if vault.forget(id).is_err() {
                    forget_failed += 1;
                }
            }
        }
        Mutation::ReorderProfiles(ids) => {
            let mut profiles = db.profiles()?;
            profiles.sort_by_key(|p| ids.iter().position(|id| *id == p.id).unwrap_or(usize::MAX));
            db.save_profiles(&profiles)?;
        }
        Mutation::SortProfiles => {
            let mut profiles = db.profiles()?;
            profiles.sort_by_key(|p| p.name.to_lowercase());
            db.save_profiles(&profiles)?;
        }
        Mutation::ReplaceProfiles(profiles) => db.save_profiles(&profiles)?,
        Mutation::Import { preview, replace } => {
            return Ok(Applied::Import(connections::import_confirmed(
                db, vault, &preview, replace,
            )?));
        }
        Mutation::RetryPasswords(pending) => {
            return Ok(Applied::Import(connections::retry_passwords(
                db, vault, pending,
            )?));
        }
        Mutation::Workspace(prefs, workspace) => {
            db.save_workspace(&prefs, &workspace)?;
            return Ok(Applied::Workspace);
        }
    }
    Ok(Applied::Profiles {
        profiles: db.profiles()?,
        forget_failed,
    })
}
