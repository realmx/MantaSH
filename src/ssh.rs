//! Verified SSH lifecycle and independently acquired SFTP/exec channels.
use crate::{
    credentials::{CredentialReply, PromptReason, SecretStore},
    events::Event,
    model::*,
    storage::Database,
};
use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use russh::{
    ChannelMsg, client,
    keys::{HashAlg, PublicKeyOrCertificate},
    mac,
};
use russh_sftp::client::SftpSession;
use std::{sync::Arc, time::Duration};
use tokio::sync::{OnceCell, oneshot};
use tokio_util::sync::CancellationToken;

pub struct Verifier {
    owner: Owner,
    profile: Profile,
    database: Arc<Mutex<Database>>,
    events: async_channel::Sender<Event>,
    cancel: CancellationToken,
}
impl client::Handler for Verifier {
    type Error = anyhow::Error;
    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool> {
        let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
        let db = self.database.clone();
        let host = self.profile.host.clone();
        let port = self.profile.port;
        let previous =
            tokio::task::spawn_blocking(move || db.lock().fingerprint(&host, port)).await??;
        if previous.as_deref() == Some(&fingerprint) {
            return Ok(true);
        }
        self.events
            .send(Event::State(self.owner, ConnectionState::HostVerification))
            .await?;
        let (reply, rx) = oneshot::channel();
        self.events
            .send(Event::HostKey {
                owner: self.owner,
                host: self.profile.host.clone(),
                port,
                previous,
                fingerprint: fingerprint.clone(),
                reply,
            })
            .await?;
        let accepted = tokio::select! { _ = self.cancel.cancelled() => false, answer = rx => answer.unwrap_or(false) };
        if accepted && !self.cancel.is_cancelled() {
            let db = self.database.clone();
            let host = self.profile.host.clone();
            tokio::task::spawn_blocking(move || db.lock().trust(&host, port, &fingerprint))
                .await??;
            Ok(!self.cancel.is_cancelled())
        } else {
            Ok(false)
        }
    }
}

pub struct Remote {
    pub owner: Owner,
    pub handle: client::Handle<Verifier>,
    pub cancel: CancellationToken,
    sftp: OnceCell<Arc<SftpSession>>,
}
impl Remote {
    /// Open SFTP lazily; failure does not discard the working SSH terminal.
    pub async fn sftp(&self) -> Result<Arc<SftpSession>> {
        if self.cancel.is_cancelled() {
            bail!("Connection is no longer available");
        }
        let sftp = self
            .sftp
            .get_or_try_init(|| async {
                let channel = self.handle.channel_open_session().await?;
                channel.request_subsystem(true, "sftp").await?;
                let session = SftpSession::new(channel.into_stream()).await?;
                session.set_timeout(20);
                Ok::<_, anyhow::Error>(Arc::new(session))
            })
            .await?;
        Ok(sftp.clone())
    }
    /// Execute a fixed read-only monitor command with bounded output and timeout.
    pub async fn exec(&self, command: &str) -> Result<String> {
        let operation = async {
            let mut channel = self.handle.channel_open_session().await?;
            channel.exec(true, command).await?;
            let mut out = Vec::new();
            while let Some(message) = channel.wait().await {
                match message {
                    ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                        if out.len() + data.len() > 8 * 1024 * 1024 {
                            bail!("Monitor output exceeds 8 MiB");
                        }
                        out.extend_from_slice(&data);
                    }
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }
            Ok(String::from_utf8_lossy(&out).into_owned())
        };
        tokio::select! { _ = self.cancel.cancelled() => bail!("Connection closed"), result = tokio::time::timeout(Duration::from_secs(20), operation) => result.context("Server did not respond within 20 seconds")? }
    }
    /// Execute a state-changing command and fail on a nonzero exit status,
    /// with the trailing stderr kept for the error message.
    pub async fn run(&self, command: &str) -> Result<()> {
        let operation = async {
            let mut channel = self.handle.channel_open_session().await?;
            channel.exec(true, command).await?;
            let mut stderr = Vec::new();
            let mut status: Option<u32> = None;
            while let Some(message) = channel.wait().await {
                match message {
                    ChannelMsg::Data { .. } => {}
                    ChannelMsg::ExtendedData { data, .. } => {
                        if stderr.len() + data.len() <= 64 * 1024 {
                            stderr.extend_from_slice(&data);
                        }
                    }
                    ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }
            match status {
                Some(0) => Ok(()),
                Some(code) => {
                    let tail = &stderr[stderr.len().saturating_sub(1024)..];
                    bail!(
                        "Command failed with exit status {code}: {}",
                        String::from_utf8_lossy(tail).trim()
                    )
                }
                None => bail!("Command finished without an exit status"),
            }
        };
        tokio::select! { _ = self.cancel.cancelled() => bail!("Connection closed"), result = tokio::time::timeout(Duration::from_secs(20), operation) => result.context("Server did not respond within 20 seconds")? }
    }
}

/// Connect and verify before loading/sending secrets. Cancellation invalidates every phase.
pub async fn connect(
    owner: Owner,
    profile: Profile,
    secret: zeroize::Zeroizing<String>,
    remember: bool,
    db: Arc<Mutex<Database>>,
    events: async_channel::Sender<Event>,
    cancel: CancellationToken,
    vault: Arc<crate::vault::LocalVault>,
) -> Result<Arc<Remote>> {
    connect_with_store(owner, profile, secret, remember, db, events, cancel, vault).await
}

/// Use the same verified lifecycle with an isolated credential store in protocol tests.
pub async fn connect_with_store(
    owner: Owner,
    profile: Profile,
    mut secret: zeroize::Zeroizing<String>,
    mut remember: bool,
    db: Arc<Mutex<Database>>,
    events: async_channel::Sender<Event>,
    cancel: CancellationToken,
    store: Arc<dyn SecretStore>,
) -> Result<Arc<Remote>> {
    profile.validate().map_err(anyhow::Error::msg)?;
    let verifier = Verifier {
        owner,
        profile: profile.clone(),
        database: db,
        events: events.clone(),
        cancel: cancel.clone(),
    };
    // Match OpenSSH's `-oHostKeyAlgorithms=+ssh-rsa`: enable only the
    // legacy RSA host-key algorithm while retaining russh's default suites.
    let mut config = client::Config {
        keepalive_interval: Some(Duration::from_secs(20)),
        keepalive_max: 3,
        ..Default::default()
    };
    // CentOS 6 commonly requires the legacy group14-SHA1 KEX in addition
    // to the ssh-rsa host-key algorithm. Keep every other russh default.
    config.preferred.kex.to_mut().push(russh::kex::DH_G14_SHA1);
    // CentOS 6 advertises only legacy MACs; enable the narrowest required one.
    config.preferred.mac.to_mut().push(mac::HMAC_SHA1);
    let config = Arc::new(config);
    // TCP timeout excludes the time a person takes to inspect the fingerprint.
    let socket = tokio::select! {
        _ = cancel.cancelled() => bail!("Connection cancelled"),
        result = tokio::time::timeout(Duration::from_secs(15), tokio::net::TcpStream::connect((profile.host.as_str(), profile.port))) => result.context("Connection timed out after 15 seconds")??,
    };
    let mut handle = tokio::select! { _ = cancel.cancelled() => bail!("Connection cancelled"), result = tokio::time::timeout(Duration::from_secs(90), client::connect_stream(config, socket, verifier)) => result.context("SSH handshake timed out after 90 seconds, including host confirmation")?? };
    if cancel.is_cancelled() {
        bail!("Connection cancelled");
    }
    let mut prompt = None;
    let mut persist_secret = remember;
    if secret.is_empty() {
        let id = profile.id;
        let reader = store.clone();
        let loaded = tokio::select! {
            _ = cancel.cancelled() => bail!("Connection cancelled"),
            result = tokio::task::spawn_blocking(move || reader.read(id)) => result?,
        };
        match loaded {
            Ok(Some(saved)) => {
                secret = saved;
                remember = true;
                persist_secret = false;
            }
            Ok(None) => {}
            Err(_) => prompt = Some(PromptReason::StoreUnavailable),
        }
        if secret.is_empty() && prompt.is_none() {
            prompt = Some(PromptReason::Missing);
        }
    }
    loop {
        if let Some(reason) = prompt.take() {
            let response = request_credentials(owner, reason, remember, &events, &cancel).await?;
            secret = response.secret;
            remember = response.remember;
            persist_secret = remember;
        }
        if cancel.is_cancelled() {
            bail!("Connection cancelled");
        }
        events
            .send(Event::State(owner, ConnectionState::Authenticating))
            .await?;
        let auth = async {
            Ok::<bool, anyhow::Error>(
                handle
                    .authenticate_password(&profile.username, secret.as_str())
                    .await?
                    .success(),
            )
        };
        let success = tokio::select! {
            _ = cancel.cancelled() => bail!("Connection cancelled"),
            result = tokio::time::timeout(Duration::from_secs(20), auth) => result.context("Authentication timed out after 20 seconds")??,
        };
        if success {
            break;
        }
        prompt = Some(PromptReason::Rejected);
        // Another authentication is only attempted after an explicit response to this prompt.
    }
    if cancel.is_cancelled() {
        bail!("Connection cancelled");
    }
    if persist_secret && !secret.is_empty() {
        let id = profile.id;
        let saved = secret.clone();
        let writer = store.clone();
        let active = cancel.clone();
        let result = tokio::task::spawn_blocking(move || {
            if active.is_cancelled() {
                bail!("Connection cancelled");
            }
            writer.write(id, &saved)
        })
        .await?;
        if !cancel.is_cancelled() {
            let _ = events
                .send(Event::CredentialStorage {
                    owner,
                    saved: result.is_ok(),
                })
                .await;
        }
    }
    Ok(Arc::new(Remote {
        owner,
        handle,
        cancel,
        sftp: OnceCell::new(),
    }))
}

/// Wait for the active pane's user, bound to its owner; closing or cancelling drops the request.
async fn request_credentials(
    owner: Owner,
    reason: PromptReason,
    remember: bool,
    events: &async_channel::Sender<Event>,
    cancel: &CancellationToken,
) -> Result<CredentialReply> {
    events
        .send(Event::State(owner, ConnectionState::CredentialsRequired))
        .await?;
    let (reply, receiver) = oneshot::channel();
    events
        .send(Event::Credentials {
            owner,
            reason,
            remember,
            reply,
        })
        .await?;
    tokio::select! {
        _ = cancel.cancelled() => bail!("Connection cancelled"),
        response = receiver => response.context("Credential entry cancelled"),
    }
}
