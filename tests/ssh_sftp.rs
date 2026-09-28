//! Loopback SSH verification and real OpenSSH SFTP subprocess integration.
#![cfg(unix)]
use mantash::{encoding::Encoding, events::Event, files, model::*, ssh, storage::Database};
use parking_lot::Mutex;
use russh::{
    Channel, ChannelId,
    server::{self, ChannelOpenHandle, Msg, Server as _, Session},
};
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct FixtureServer {
    root: std::path::PathBuf,
    password: String,
    client_key: russh::keys::PublicKey,
    auth_calls: Arc<AtomicUsize>,
}
struct FixtureClient {
    config: FixtureServer,
    channels: HashMap<ChannelId, Channel<Msg>>,
    pty_sizes: HashMap<ChannelId, (u16, u16)>,
}
impl server::Server for FixtureServer {
    type Handler = FixtureClient;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> FixtureClient {
        FixtureClient {
            config: self.clone(),
            channels: HashMap::new(),
            pty_sizes: HashMap::new(),
        }
    }
}
impl server::Handler for FixtureClient {
    type Error = anyhow::Error;
    async fn auth_password(&mut self, user: &str, password: &str) -> anyhow::Result<server::Auth> {
        self.config.auth_calls.fetch_add(1, Ordering::SeqCst);
        Ok(
            if user == "mantash-test" && password == self.config.password {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> anyhow::Result<server::Auth> {
        self.config.auth_calls.fetch_add(1, Ordering::SeqCst);
        Ok(
            if user == "mantash-test"
                && key.fingerprint(russh::keys::HashAlg::Sha256)
                    == self
                        .config
                        .client_key
                        .fingerprint(russh::keys::HashAlg::Sha256)
            {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> anyhow::Result<()> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }
    async fn pty_request(
        &mut self,
        id: ChannelId,
        _: &str,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> anyhow::Result<()> {
        self.pty_sizes.insert(id, (cols as u16, rows as u16));
        session.channel_success(id)?;
        Ok(())
    }
    async fn exec_request(
        &mut self,
        id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> anyhow::Result<()> {
        if let Some(size) = self.pty_sizes.remove(&id) {
            return self
                .bridge_pty(id, std::str::from_utf8(data)?, size, session)
                .await;
        }
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(std::str::from_utf8(data)?)
            .current_dir(&self.config.root);
        self.bridge(id, command, session).await
    }
    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> anyhow::Result<()> {
        if name != "sftp" {
            session.channel_failure(id)?;
            return Ok(());
        }
        let binary = if cfg!(target_os = "macos") {
            "/usr/libexec/sftp-server"
        } else {
            "/usr/lib/openssh/sftp-server"
        };
        let mut command = tokio::process::Command::new(binary);
        command.arg("-d").arg(&self.config.root);
        self.bridge(id, command, session).await
    }
}
impl FixtureClient {
    /// Use a genuine PTY for interactive SSH channels; pipes remain appropriate for SFTP and exec.
    async fn bridge_pty(
        &mut self,
        id: ChannelId,
        command: &str,
        (cols, rows): (u16, u16),
        session: &mut Session,
    ) -> anyhow::Result<()> {
        use std::io::{Read, Write};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let pair = portable_pty::native_pty_system().openpty(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut builder = portable_pty::CommandBuilder::new("/bin/sh");
        builder.args(["-c", command]);
        builder.cwd(&self.config.root);
        builder.env("SHELL", "/bin/bash");
        builder.env("TERM", "xterm-256color");
        let mut child = pair.slave.spawn_command(builder)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let mut killer = child.clone_killer();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
        let (in_tx, mut in_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buffer = vec![0; 32768];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if out_tx.blocking_send(buffer[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = child.wait();
        });
        std::thread::spawn(move || {
            let _master = pair.master;
            while let Some(bytes) = in_rx.blocking_recv() {
                if writer
                    .write_all(&bytes)
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
            let _ = killer.kill();
        });
        let channel = self.channels.remove(&id).unwrap();
        let handle = session.handle();
        session.channel_success(id)?;
        tokio::spawn(async move {
            let (mut input, mut output) = tokio::io::split(channel.into_stream());
            let incoming = tokio::spawn(async move {
                let mut buffer = vec![0; 32768];
                loop {
                    match input.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if in_tx.send(buffer[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
            while let Some(bytes) = out_rx.recv().await {
                if output.write_all(&bytes).await.is_err() {
                    break;
                }
            }
            incoming.abort();
            let _ = handle.eof(id).await;
            let _ = handle.close(id).await;
        });
        Ok(())
    }
    /// Bridge encrypted channel bytes to a genuine subprocess, never an in-memory SFTP mock.
    async fn bridge(
        &mut self,
        id: ChannelId,
        mut command: tokio::process::Command,
        session: &mut Session,
    ) -> anyhow::Result<()> {
        let channel = self.channels.remove(&id).unwrap();
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        session.channel_success(id)?;
        let handle = session.handle();
        tokio::spawn(async move {
            let stream = channel.into_stream();
            let (mut reader, mut writer) = tokio::io::split(stream);
            let mut input = child.stdin.take().unwrap();
            let mut output = child.stdout.take().unwrap();
            let send = tokio::spawn(async move {
                let _ = tokio::io::copy(&mut reader, &mut input).await;
            });
            let _ = tokio::io::copy(&mut output, &mut writer).await;
            let status = child.wait().await;
            send.abort();
            let _ = handle
                .exit_status_request(id, status.ok().and_then(|s| s.code()).unwrap_or(1) as u32)
                .await;
            let _ = handle.eof(id).await;
            let _ = handle.close(id).await;
        });
        Ok(())
    }
}

struct Fixture {
    profile: Profile,
    password: String,
    calls: Arc<AtomicUsize>,
    server: tokio::task::JoinHandle<()>,
    _directory: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let password = Id::new_v4().to_string();
        let calls = Arc::new(AtomicUsize::new(0));
        let client_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let mut server = FixtureServer {
            root: directory.path().into(),
            password: password.clone(),
            client_key: client_key.public_key().clone(),
            auth_calls: calls.clone(),
        };
        let key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let config = Arc::new(server::Config {
            keys: vec![key],
            auth_rejection_time: Duration::from_millis(5),
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let socket = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = socket.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let _ = server.run_on_socket(config, &socket).await;
        });
        Self {
            profile: Profile {
                id: Id::new_v4(),
                name: "Loopback fixture".into(),
                host: "127.0.0.1".into(),
                port,
                username: "mantash-test".into(),
            },
            password,
            calls,
            server: task,
            _directory: directory,
        }
    }
    async fn connected(&self) -> Arc<ssh::Remote> {
        let (events, receiver) = async_channel::unbounded();
        let owner = Owner::new();
        let db = Arc::new(Mutex::new(Database::memory().unwrap()));
        let task = tokio::spawn(ssh::connect(
            owner,
            self.profile.clone(),
            zeroize::Zeroizing::new(self.password.clone()),
            false,
            db,
            events,
            CancellationToken::new(),
            Arc::new(mantash::vault::LocalVault::new(self._directory.path())),
        ));
        loop {
            if let Event::HostKey { reply, .. } =
                tokio::time::timeout(Duration::from_secs(10), receiver.recv())
                    .await
                    .unwrap()
                    .unwrap()
            {
                assert_eq!(self.calls.load(Ordering::SeqCst), 0);
                reply.send(true).unwrap();
                break;
            }
        }
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[tokio::test]
async fn credentials_wait_for_explicit_host_approval() {
    let fixture = Fixture::new().await;
    let remote = fixture.connected().await;
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        remote.exec("printf 'SSH_EXEC_OK'").await.unwrap(),
        "SSH_EXEC_OK"
    );
    remote.cancel.cancel();
}

#[tokio::test]
async fn cancelled_changed_fingerprint_never_sends_credentials() {
    let fixture = Fixture::new().await;
    let (events, receiver) = async_channel::unbounded();
    let db = Arc::new(Mutex::new(Database::memory().unwrap()));
    db.lock()
        .trust(
            &fixture.profile.host,
            fixture.profile.port,
            "SHA256:old-value",
        )
        .unwrap();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(ssh::connect(
        Owner::new(),
        fixture.profile.clone(),
        zeroize::Zeroizing::new(fixture.password.clone()),
        false,
        db.clone(),
        events,
        cancel.clone(),
        Arc::new(mantash::vault::LocalVault::new(fixture._directory.path())),
    ));
    loop {
        if let Event::HostKey {
            previous, reply, ..
        } = tokio::time::timeout(Duration::from_secs(10), receiver.recv())
            .await
            .unwrap()
            .unwrap()
        {
            assert_eq!(previous.as_deref(), Some("SHA256:old-value"));
            cancel.cancel();
            assert!(task.await.unwrap().is_err());
            let _ = reply.send(true);
            break;
        }
    }
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        db.lock()
            .fingerprint(&fixture.profile.host, fixture.profile.port)
            .unwrap()
            .as_deref(),
        Some("SHA256:old-value")
    );
}

#[tokio::test]
async fn real_sftp_edit_last_write_wins_and_directory_transfer() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new().await;
    let remote = fixture.connected().await;
    let root = fixture._directory.path().to_string_lossy().into_owned();
    let path = format!("{root}/hello.txt");
    tokio::fs::write(&path, "initial 中文\n").await.unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let (_, entries) = files::list(&remote, &root).await.unwrap();
    assert!(entries.iter().any(|e| e.name == "hello.txt"));
    let document = files::open(&remote, &path, None).await.unwrap();
    assert_eq!(document.text, "initial 中文\n");
    assert!(matches!(
        files::save(&remote, &document, "edited 中文\n", Encoding::Utf8, false)
            .await
            .unwrap(),
        files::SaveResult::Saved(_)
    ));
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        tokio::fs::read_to_string(&path).await.unwrap(),
        "edited 中文\n"
    );
    assert!(matches!(
        files::save(&remote, &document, "stale draft", Encoding::Utf8, false)
            .await
            .unwrap(),
        files::SaveResult::Saved(_)
    ));
    assert_eq!(
        tokio::fs::read_to_string(&path).await.unwrap(),
        "stale draft"
    );
    // Simulate an external vi save, then reopen through the same remote
    // session: the read must observe the newest bytes rather than a cached
    // editor document.
    tokio::fs::write(&path, "vi latest 中文\n").await.unwrap();
    let reopened = files::open(&remote, &path, None).await.unwrap();
    assert_eq!(reopened.text, "vi latest 中文\n");
    assert!(matches!(
        files::save(
            &remote,
            &reopened,
            "editor final 中文\n",
            Encoding::Utf8,
            false
        )
        .await
        .unwrap(),
        files::SaveResult::Saved(_)
    ));
    assert_eq!(
        tokio::fs::read_to_string(&path).await.unwrap(),
        "editor final 中文\n"
    );
    let final_read = files::open(&remote, &path, None).await.unwrap();
    assert_eq!(final_read.text, "editor final 中文\n");
    let source = tempfile::tempdir().unwrap();
    tokio::fs::create_dir(source.path().join("nested"))
        .await
        .unwrap();
    tokio::fs::write(source.path().join("nested/中文.txt"), "transfer bytes")
        .await
        .unwrap();
    let mut task = TransferRecord {
        id: Id::new_v4(),
        profile: fixture.profile.clone(),
        upload: true,
        local: source.path().to_string_lossy().into_owned(),
        remote: format!("{root}/uploaded"),
        session: Some(remote.owner.session),
        attempt: Some(remote.owner.attempt),
        state: TransferState::Running,
        bytes: 0,
        total: None,
        error: None,
        timestamp: 0,
    };
    files::transfer(
        remote.clone(),
        &mut task,
        false,
        CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(
        tokio::fs::read_to_string(format!("{root}/uploaded/nested/中文.txt"))
            .await
            .unwrap(),
        "transfer bytes"
    );
    let download = tempfile::tempdir().unwrap();
    task.id = Id::new_v4();
    task.upload = false;
    task.local = download.path().join("copy").to_string_lossy().into_owned();
    task.bytes = 0;
    files::transfer(
        remote.clone(),
        &mut task,
        false,
        CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(download.path().join("copy/nested/中文.txt")).unwrap(),
        "transfer bytes"
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(
        files::transfer(remote.clone(), &mut task, true, cancelled, |_| {})
            .await
            .is_err()
    );
    files::delete(&remote, &[format!("{root}/uploaded")])
        .await
        .unwrap();
    assert!(!std::path::Path::new(&format!("{root}/uploaded")).exists());
    let blocked = format!("{root}/blocked");
    tokio::fs::create_dir(&blocked).await.unwrap();
    tokio::fs::write(format!("{blocked}/bad\\name"), "blocked")
        .await
        .unwrap();
    assert!(
        files::delete(&remote, std::slice::from_ref(&blocked))
            .await
            .is_err()
    );
    assert!(std::path::Path::new(&blocked).exists());
    assert!(files::delete(&remote, &["/".into()]).await.is_err());
    remote.cancel.cancel();
}

#[tokio::test]
async fn upload_size_and_progress_follow_the_current_regular_file() {
    let fixture = Fixture::new().await;
    let remote = fixture.connected().await;
    let source = tempfile::tempdir().unwrap();
    let file = source.path().join("upload.bin");
    let empty = source.path().join("empty.bin");
    let link = source.path().join("link.bin");
    tokio::fs::write(&file, vec![7u8; 256 * 1024])
        .await
        .unwrap();
    tokio::fs::write(&empty, []).await.unwrap();
    std::os::unix::fs::symlink(&file, &link).unwrap();
    assert_eq!(files::upload_file_size(&file).await, Some(256 * 1024));
    assert_eq!(files::upload_file_size(&empty).await, Some(0));
    assert_eq!(files::upload_file_size(source.path()).await, None);
    assert_eq!(files::upload_file_size(&link).await, None);
    assert_eq!(
        files::upload_file_size(&source.path().join("missing")).await,
        None
    );

    let mut record = TransferRecord {
        id: Id::new_v4(),
        profile: fixture.profile.clone(),
        upload: true,
        local: file.to_string_lossy().into_owned(),
        remote: fixture
            ._directory
            .path()
            .join("uploaded.bin")
            .to_string_lossy()
            .into_owned(),
        session: Some(remote.owner.session),
        attempt: Some(remote.owner.attempt),
        state: TransferState::Running,
        bytes: 0,
        total: Some(1), // The source changed since the confirmation was rendered.
        error: None,
        timestamp: 0,
    };
    let progress = Arc::new(Mutex::new(Vec::new()));
    let samples = progress.clone();
    files::transfer(
        remote.clone(),
        &mut record,
        false,
        CancellationToken::new(),
        move |task| {
            samples.lock().push((task.bytes, task.total));
        },
    )
    .await
    .unwrap();
    assert_eq!(record.total, Some(256 * 1024));
    assert_eq!(record.bytes, 256 * 1024);
    let progress = progress.lock();
    assert!(!progress.is_empty());
    assert!(progress.iter().all(|(_, total)| *total == record.total));
    assert!(progress.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert_eq!(progress.last().unwrap().0, record.bytes);
    drop(progress);
    assert_eq!(
        tokio::fs::read(&record.remote).await.unwrap(),
        vec![7u8; 256 * 1024]
    );
    remote.cancel.cancel();
}

#[tokio::test]
async fn sftp_failed_save_and_cancel_preserve_original_bytes() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new().await;
    let remote = fixture.connected().await;
    let path = fixture._directory.path().join("protected.txt");
    std::fs::write(&path, "original").unwrap();
    let document = files::open(&remote, &path.to_string_lossy(), None)
        .await
        .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(
        files::save(&remote, &document, "new draft", Encoding::Utf8, false)
            .await
            .is_err()
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    let link = fixture._directory.path().join("link.txt");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let resolved = files::link_target(&remote, link.to_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        resolved.0,
        std::fs::canonicalize(&path).unwrap().to_string_lossy()
    );
    assert!(!resolved.1);
    assert!(
        files::open(&remote, &link.to_string_lossy(), None)
            .await
            .is_err()
    );
    files::delete(
        &remote,
        std::slice::from_ref(&link.to_string_lossy().into_owned()),
    )
    .await
    .unwrap();
    assert!(path.exists());
    assert!(!link.exists());
    let source = fixture._directory.path().join("large.bin");
    std::fs::write(&source, vec![7u8; 2 * 1024 * 1024]).unwrap();
    let mut record = TransferRecord {
        id: Id::new_v4(),
        profile: fixture.profile.clone(),
        upload: true,
        local: source.to_string_lossy().into_owned(),
        remote: path.to_string_lossy().into_owned(),
        session: Some(remote.owner.session),
        attempt: Some(remote.owner.attempt),
        state: TransferState::Running,
        bytes: 0,
        total: None,
        error: None,
        timestamp: 0,
    };
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let private_temporary = format!("{}.mantash-transfer-{}", record.remote, record.id);
    assert!(
        files::transfer(remote.clone(), &mut record, true, cancel, move |_| {
            assert_eq!(
                std::fs::metadata(&private_temporary)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            trigger.cancel();
        })
        .await
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    remote.cancel.cancel();
}

#[tokio::test]
#[ignore = "Opt-in native UI fixture; requires MANTASH_FIXTURE_DIR and stops on stop-fixture or after 45 minutes"]
async fn native_qa_fixture() {
    let destination = std::path::PathBuf::from(
        std::env::var_os("MANTASH_FIXTURE_DIR").expect("set MANTASH_FIXTURE_DIR"),
    );
    std::fs::create_dir_all(&destination).unwrap();
    let fixture = Fixture::new().await;
    let root = fixture._directory.path();
    std::fs::create_dir(root.join("config")).unwrap();
    std::fs::create_dir(root.join("logs")).unwrap();
    std::fs::write(root.join("README.md"), "# MantaSH\n\nA compact terminal for local and SSH work.\n\n## Native acceptance\n\n- Real SSH transport\n- OpenSSH SFTP file operations\n- Unicode text: 你好，世界\n").unwrap();
    std::fs::write(
        root.join("config/service.toml"),
        "name = \"mantash\"\nenabled = true\nworkers = 4\n\n# 本地验收使用隔离数据\n",
    )
    .unwrap();
    let mut profile = fixture.profile.clone();
    profile.name = "MantaSH · loopback".into();
    std::fs::write(
        destination.join("profile.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"profile":profile})).unwrap(),
    )
    .unwrap();
    std::fs::write(
        destination.join("fixture.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"root":root,"port":profile.port})).unwrap(),
    )
    .unwrap();
    // Opt-in credential tests use only this random loopback password in a private temporary file.
    if std::env::var_os("MANTASH_FIXTURE_PASSWORD").is_some() {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(destination.join("password.txt"))
            .unwrap();
        file.write_all(fixture.password.as_bytes()).unwrap();
    }
    println!(
        "Loopback SSH/OpenSSH SFTP fixture ready; metadata: {}",
        destination.display()
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(2700);
    while std::time::Instant::now() < deadline && !destination.join("stop-fixture").exists() {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[path = "support/credential_reconnect.rs"]
mod credential_reconnect;
