//! Fixed Linux process operations. Display text never becomes executable command text.
use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

/// PID plus process and host lifetimes; a reused PID is a different target.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Identity {
    pub pid: u32,
    pub start_ticks: u64,
    pub boot_id: String,
}

impl Identity {
    /// Reject incomplete identities before opening an execution channel.
    pub fn validate(&self) -> Result<()> {
        if self.pid <= 2 || self.start_ticks == 0 || uuid::Uuid::parse_str(&self.boot_id).is_err() {
            bail!("Protected process or unavailable process identity");
        }
        Ok(())
    }
}

/// Why a sampled process cannot be treated as the originally selected instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetIssue {
    IdentityMissing,
    InvalidSample,
    HostChanged,
    Gone,
    Changed,
}

/// Return metrics only for a verified Linux sample of the original host and process lifetime.
pub fn current_process<'a>(
    identity: &Identity,
    sample: &'a crate::monitor::Sample,
) -> std::result::Result<&'a crate::monitor::Process, TargetIssue> {
    identity
        .validate()
        .map_err(|_| TargetIssue::IdentityMissing)?;
    if sample.system != "Linux" || sample.errors.contains_key("processes") {
        return Err(TargetIssue::InvalidSample);
    }
    if sample.boot_id.as_deref() != Some(identity.boot_id.as_str()) {
        return Err(TargetIssue::HostChanged);
    }
    let process = sample
        .processes
        .iter()
        .find(|p| p.pid == identity.pid)
        .ok_or(TargetIssue::Gone)?;
    if process.identity.as_ref() != Some(identity) {
        return Err(TargetIssue::Changed);
    }
    Ok(process)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Terminate,
    Force,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Sent,
    Gone,
    Changed,
    Denied,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Details {
    pub command: String,
    pub status: String,
}

impl Details {
    /// Display /proc status fields not already covered by the process summary.
    /// Preserve identifiers such as Uid/Tgid and unknown fields: they are not duplicates.
    pub fn supplementary_status(&self) -> String {
        self.status
            .lines()
            .filter(|line| {
                !line.split_once(':').is_some_and(|(key, _)| {
                    matches!(key.trim(), "State" | "Pid" | "PPid" | "VmRSS")
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Both probes compare identities immediately before the operation. No name or command
/// received from the server is interpolated. POSIX kill still has a small check/send race.
fn script(identity: &Identity, operation: &str) -> Result<String> {
    identity.validate()?;
    let boot = uuid::Uuid::parse_str(&identity.boot_id)?.to_string();
    let body = format!(
        r#"export LC_ALL=C
pid={pid}
expected={start}
boot='{boot}'
finish() {{ printf '__MANTASH_PROCESS_%s__\n' "$1"; exit 0; }}
[ "$(uname -s)" = Linux ] || finish UNSUPPORTED
check() {{
  current_boot=$(cat /proc/sys/kernel/random/boot_id 2>/dev/null) || finish DENIED
  [ "$current_boot" = "$boot" ] || finish CHANGED
  [ -d "/proc/$pid" ] || finish GONE
  stat=$(cat "/proc/$pid/stat" 2>/dev/null) || finish DENIED
  current=$(printf '%s\n' "$stat" | awk '{{sub(/^.*\) /, ""); print $20}}')
  [ "$current" = "$expected" ] || finish CHANGED
  kernel=$(printf '%s\n' "$stat" | awk '{{sub(/^.*\) /, ""); print int($7 / 2097152) % 2}}')
  [ "$kernel" = 0 ] || finish DENIED
}}
check
{operation}
"#,
        pid = identity.pid,
        start = identity.start_ticks
    );
    Ok(format!("sh -c '{}'", body.replace('\'', "'\\''")))
}

/// Read at most 64 KiB of command text, then recheck before returning it to the UI.
pub fn details_command(identity: &Identity) -> Result<String> {
    script(
        identity,
        r#"command -v base64 >/dev/null 2>&1 || finish UNSUPPORTED
command=$(head -c 65536 "/proc/$pid/cmdline" 2>/dev/null | base64 | tr -d '\n')
status=$(head -c 16384 "/proc/$pid/status" 2>/dev/null | base64 | tr -d '\n')
check
printf '__MANTASH_PROCESS_DETAILS__\n%s\n%s\n' "$command" "$status""#,
    )
}

/// Signals are an enum, and the target PID is numeric; force is never an automatic fallback.
pub fn signal_command(identity: &Identity, action: Action) -> Result<String> {
    let signal = match action {
        Action::Terminate => "TERM",
        Action::Force => "KILL",
    };
    script(
        identity,
        &format!(
            r#"check
if kill -{signal} "$pid" 2>/dev/null; then finish SENT; fi
check
finish DENIED"#
        ),
    )
}

/// Unknown or truncated responses never imply successful termination.
pub fn outcome(output: &str) -> Outcome {
    output
        .lines()
        .find_map(|line| {
            Some(match line {
                "__MANTASH_PROCESS_SENT__" => Outcome::Sent,
                "__MANTASH_PROCESS_GONE__" => Outcome::Gone,
                "__MANTASH_PROCESS_CHANGED__" => Outcome::Changed,
                "__MANTASH_PROCESS_DENIED__" => Outcome::Denied,
                "__MANTASH_PROCESS_UNSUPPORTED__" => Outcome::Unsupported,
                _ => return None,
            })
        })
        .unwrap_or(Outcome::Unknown)
}

/// Decode display-only details without leaking full commands into logs or persistence.
pub fn details(output: &str) -> Result<Details> {
    let Some((_, rest)) = output.split_once("__MANTASH_PROCESS_DETAILS__\n") else {
        bail!("Process details unavailable: {:?}", outcome(output));
    };
    let mut lines = rest.lines();
    let command = STANDARD.decode(lines.next().unwrap_or_default())?;
    let status = STANDARD.decode(lines.next().unwrap_or_default())?;
    Ok(Details {
        command: String::from_utf8_lossy(&command)
            .replace('\0', " ")
            .trim()
            .to_string(),
        status: String::from_utf8_lossy(&status).to_string(),
    })
}

/// A sent request remains pending until a valid subsequent sample proves the instance gone.
pub fn instance_gone(identity: &Identity, sample: &crate::monitor::Sample) -> bool {
    sample.system == "Linux"
        && !sample.errors.contains_key("processes")
        && sample.boot_id.as_deref() == Some(identity.boot_id.as_str())
        && !sample.processes.iter().any(|p| {
            p.pid == identity.pid
                && p.identity
                    .as_ref()
                    .is_none_or(|current| current == identity)
        })
}
