//! Typed messages from background services. Each asynchronous result retains its owner.
use crate::{
    files::{FileEntry, OpenedFile, SaveResult},
    model::*,
    monitor::Sample,
};
use tokio::sync::oneshot;

pub enum Event {
    #[cfg(all(feature = "desktop", debug_assertions))]
    Qa(crate::ui::qa::Request),
    State(Owner, ConnectionState),
    Output(Owner),
    Title(Owner, String),
    Program(Owner, Option<String>),
    EncodingWarning(Owner),
    Directory(Owner, String),
    History(HistoryEntry),
    HostKey {
        owner: Owner,
        host: String,
        port: u16,
        previous: Option<String>,
        fingerprint: String,
        reply: oneshot::Sender<bool>,
    },
    Credentials {
        owner: Owner,
        reason: crate::credentials::PromptReason,
        remember: bool,
        reply: oneshot::Sender<crate::credentials::CredentialReply>,
    },
    CredentialStorage {
        owner: Owner,
        saved: bool,
    },
    Files {
        owner: Owner,
        request: Id,
        result: Result<(String, Vec<FileEntry>), String>,
    },
    FileOpened {
        owner: Owner,
        request: Id,
        result: Result<OpenedFile, String>,
    },
    FileLink {
        owner: Owner,
        request: Id,
        path: String,
        target: String,
        directory: bool,
    },
    FileSaved {
        owner: Owner,
        document: Id,
        revision: u64,
        result: Result<SaveResult, String>,
    },
    FileOperation {
        owner: Owner,
        request: Id,
        result: Result<(), String>,
    },
    Transfer(TransferRecord),
    Monitor {
        owner: Owner,
        request: Id,
        result: Result<Sample, String>,
    },
    ProcessDetails {
        owner: Owner,
        request: Id,
        result: Result<crate::processes::Details, String>,
    },
    ProcessAction {
        owner: Owner,
        request: Id,
        result: Result<crate::processes::Outcome, String>,
    },
    Error(String),
    ProfilesSaved(Result<(), String>),
}
