//! Credential values stay in memory or the encrypted local vault, never in profile exports.
use crate::model::Id;
use anyhow::Result;
use zeroize::Zeroizing;

/// A reply to one verified connection attempt; deliberately not Debug or serializable.
pub struct CredentialReply {
    pub secret: Zeroizing<String>,
    pub remember: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptReason {
    Missing,
    Rejected,
    StoreUnavailable,
}

/// Blocking store operations run off the UI thread and only after host verification.
pub trait SecretStore: Send + Sync {
    fn read(&self, id: Id) -> Result<Option<Zeroizing<String>>>;
    fn write(&self, id: Id, secret: &str) -> Result<()>;
}
