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

/// Blocking store operations run off the UI thread. Connections read only after host
/// verification; the only other reads are user-initiated local views (the edit-form
/// eye toggle and CSV export).
pub trait SecretStore: Send + Sync {
    fn read(&self, id: Id) -> Result<Option<Zeroizing<String>>>;
    fn write(&self, id: Id, secret: &str) -> Result<()>;
}

/// Pure decisions for showing a saved password in the connection edit form.
/// The form never pre-reads: a read starts only from the user's eye click.
pub mod reveal {
    use crate::model::Id;

    /// Whether an eye click should read the saved password: only for an existing
    /// (not cloned) profile, with credential memory on, an untouched (empty) field,
    /// and no earlier completed read in this form.
    pub fn may_read_saved(
        editing_saved: bool,
        remember: bool,
        typed_empty: bool,
        already_read: bool,
    ) -> bool {
        editing_saved && remember && typed_empty && !already_read
    }

    /// Whether a finished read still belongs to the open form: the same pending
    /// request, the same profile, and the user has not typed anything meanwhile.
    pub fn applies(
        pending: Option<Id>,
        form_profile: Id,
        request: Id,
        profile: Id,
        typed_empty: bool,
    ) -> bool {
        pending == Some(request) && form_profile == profile && typed_empty
    }

    /// The secret a submit passes on. A revealed saved value left unchanged is not a
    /// new password: it submits empty, so the normal saved-credential reuse applies
    /// and the store is never rewritten. Any edit is submitted as typed.
    pub fn submitted_secret<'a>(typed: &'a str, revealed: Option<&str>) -> &'a str {
        if revealed.is_some_and(|saved| saved == typed) {
            ""
        } else {
            typed
        }
    }
}
