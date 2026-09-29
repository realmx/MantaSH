//! Background orchestration for sessions, storage, files and transfers.
use crate::{
    events::Event,
    files::{self, OpenedFile},
    model::*,
    ssh::Remote,
    storage::{Database, Snapshot},
    terminal::{GridSize, HistoryParser, TerminalBuffer},
};
use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// macOS native zoom publishes geometry on several animation frames. Keep
/// SIGWINCH quiet until the latest frame has been stable long enough for zsh's
/// RPROMPT redraw to finish.
const LOCAL_RESIZE_SETTLE: Duration = Duration::from_millis(500);

pub enum Command {
    Input(Vec<u8>),
    Resize(GridSize),
    Close,
}
pub struct Session {
    pub owner: Owner,
    pub spec: SessionSpec,
    pub terminal: Arc<Mutex<TerminalBuffer>>,
    pub commands: mpsc::UnboundedSender<Command>,
    pub cancel: CancellationToken,
    pub remote: Mutex<Option<Arc<Remote>>>,
    pub output_wakeup: crate::terminal_io::OutputWakeup,
    /// Serializes local PTY geometry changes with output parsing. The PTY must
    /// report its new size before the emulator accepts bytes produced for it.
    pub resize_lock: Arc<Mutex<()>>,
}
impl Session {
    /// Queue terminal input without blocking the main thread.
    pub fn input(&self, bytes: Vec<u8>) {
        if !self.cancel.is_cancelled() {
            self.terminal.lock().input_sent(&bytes);
            let _ = self.commands.send(Command::Input(bytes));
        }
    }
    /// Resize both emulator and OS endpoint, preserving the same process.
    pub fn resize(&self, cols: usize, rows: usize) {
        if matches!(self.spec, SessionSpec::Local { .. }) {
            // Local PTY resizes are applied by the I/O worker. Updating the
            // emulator here would let shell redraw bytes arrive while the
            // kernel still exposes the previous window size.
            let size = GridSize {
                cols: cols.clamp(2, 1000),
                rows: rows.clamp(1, 500),
            };
            let _ = self.commands.send(Command::Resize(size));
            return;
        }
        let _resize = self.resize_lock.lock();
        let mut buffer = self.terminal.lock();
        if buffer.resize(cols, rows) {
            let _ = self.commands.send(Command::Resize(buffer.size));
        }
    }
    /// Invalidate this attempt and wake the blocking PTY writer.
    pub fn close(&self) {
        self.cancel.cancel();
        let _ = self.commands.send(Command::Close);
    }
}

#[derive(Clone)]
pub struct Backend {
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub database: Arc<Mutex<Database>>,
    pub events: async_channel::Sender<Event>,
    sessions: Arc<Mutex<HashMap<Id, Arc<Session>>>>,
    transfers: Arc<Mutex<HashMap<Id, CancellationToken>>>,
    permits: Arc<tokio::sync::Semaphore>,
    pub data_directory: std::path::PathBuf,
    pub vault: Arc<crate::vault::LocalVault>,
    layout_revision: Arc<AtomicU64>,
    profiles_revision: Arc<AtomicU64>,
}
impl Backend {
    /// Load on a worker before the native event loop starts. Corrupt data is never replaced.
    pub fn initialize() -> Result<(
        Self,
        async_channel::Receiver<Event>,
        Snapshot,
        Option<String>,
    )> {
        Self::initialize_from(crate::platform::data_directory()?)
    }
    /// Use an explicit isolated directory for integration tests or an independently selected profile.
    pub fn initialize_in(
        directory: std::path::PathBuf,
    ) -> Result<(
        Self,
        async_channel::Receiver<Event>,
        Snapshot,
        Option<String>,
    )> {
        Self::initialize_from(directory)
    }
    /// Resolve metadata on the background worker.
    fn initialize_from(
        directory: std::path::PathBuf,
    ) -> Result<(
        Self,
        async_channel::Receiver<Event>,
        Snapshot,
        Option<String>,
    )> {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(4)
                .build()?,
        );
        let vault = Arc::new(crate::vault::LocalVault::new(&directory));
        let dir = directory.clone();
        let (database, snapshot, warning) = runtime.block_on(async {
            tokio::task::spawn_blocking(move || {
                match Database::open(&dir).and_then(|mut db| {
                    // Startup compaction is best-effort: a failure keeps old duplicates
                    // visible instead of blocking the whole data directory.
                    let _ = db.compact_history();
                    Ok((db.load()?, db))
                }) {
                    Ok((snapshot, db)) => Ok::<_, anyhow::Error>((db, snapshot, None)),
                    Err(error) => Ok((
                        Database::memory()?,
                        Snapshot::default(),
                        Some(format!(
                            "{:#} — session-only mode. Existing data was retained; current data directory: {}",
                            error,
                            dir.display()
                        )),
                    )),
                }
            })
            .await?
        })?;
        let (events, receiver) = async_channel::unbounded();
        Ok((
            Self {
                runtime,
                database: Arc::new(Mutex::new(database)),
                events,
                sessions: Default::default(),
                transfers: Default::default(),
                permits: Arc::new(tokio::sync::Semaphore::new(2)),
                vault,
                data_directory: directory,
                layout_revision: Default::default(),
                profiles_revision: Default::default(),
            },
            receiver,
            snapshot,
            warning,
        ))
    }
    /// Start an independent local or remote session. Any previous attempt for its ID is cancelled.
    pub fn start(
        &self,
        owner: Owner,
        spec: SessionSpec,
        secret: zeroize::Zeroizing<String>,
        remember_secret: bool,
    ) -> Arc<Session> {
        let (commands, receiver) = mpsc::unbounded_channel();
        let session = Arc::new(Session {
            owner,
            terminal: Arc::new(Mutex::new(TerminalBuffer::new(spec.encoding()))),
            spec,
            commands,
            cancel: CancellationToken::new(),
            remote: Mutex::new(None),
            output_wakeup: Default::default(),
            resize_lock: Arc::new(Mutex::new(())),
        });
        if let Some(old) = self.sessions.lock().insert(owner.session, session.clone()) {
            old.close();
        }
        let backend = self.clone();
        let live = session.clone();
        let _ = self
            .events
            .try_send(Event::State(owner, ConnectionState::Connecting));
        match &session.spec {
            SessionSpec::Local { .. } => {
                std::thread::spawn(move || {
                    if let Err(error) = backend.local(live.clone(), receiver) {
                        let cancelled = live.cancel.is_cancelled();
                        live.close();
                        let _ = backend.events.try_send(Event::State(
                            owner,
                            if cancelled {
                                ConnectionState::Cancelled
                            } else {
                                ConnectionState::Failed(format!("{error:#}"))
                            },
                        ));
                    }
                });
            }
            SessionSpec::Ssh { .. } => {
                self.runtime.spawn(async move {
                    if let Err(error) = backend
                        .remote(live.clone(), receiver, secret, remember_secret)
                        .await
                    {
                        let cancelled = live.cancel.is_cancelled();
                        live.close();
                        let _ = backend
                            .events
                            .send(Event::State(
                                owner,
                                if cancelled {
                                    ConnectionState::Cancelled
                                } else {
                                    ConnectionState::Failed(format!("{error:#}"))
                                },
                            ))
                            .await;
                    }
                });
            }
        }
        session
    }
    fn local(
        &self,
        session: Arc<Session>,
        mut receiver: mpsc::UnboundedReceiver<Command>,
    ) -> Result<()> {
        let SessionSpec::Local {
            shell, directory, ..
        } = &session.spec
        else {
            bail!("Expected local session");
        };
        let directory = crate::platform::normalize_local_directory(directory, shell);
        let executable = crate::platform::find_executable(shell)
            .ok_or_else(|| anyhow::anyhow!("Configured local shell is unavailable: {shell}"))?;
        let persistent_hook_directory = self.data_directory.join("shell-integration");
        let (hook_directory, _temporary_hook_directory) =
            if crate::integration::write_local(&persistent_hook_directory).is_ok() {
                (persistent_hook_directory, None)
            } else {
                // A read-only configuration must not prevent starting a local shell.
                let temporary = tempfile::Builder::new()
                    .prefix("mantash-shell-")
                    .tempdir_in(std::env::temp_dir())?;
                crate::integration::write_local(temporary.path())?;
                (temporary.path().to_path_buf(), Some(temporary))
            };
        let token = Id::new_v4().to_string();
        session.terminal.lock().set_shell_token(token.clone());
        let mut command = portable_pty::CommandBuilder::new(&executable);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("MANTASH_HISTORY_TOKEN", &token);
        command.env("MANTASH_INTEGRATION_DIR", &hook_directory);
        let basename = executable
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        match basename.as_str() {
            "zsh" => {
                command.env(
                    "MANTASH_USER_ZDOTDIR",
                    std::env::var("ZDOTDIR").unwrap_or_else(|_| crate::platform::home_directory()),
                );
                command.env("ZDOTDIR", &hook_directory);
                command.args(["-l", "-i"]);
            }
            "bash" => {
                command.arg("--rcfile");
                command.arg(hook_directory.join("bash.rc"));
                command.arg("-i");
            }
            "pwsh" | "powershell" => {
                command.args(["-NoLogo", "-NoExit", "-Command"]);
                command.arg(format!(
                    ". '{}'",
                    hook_directory
                        .join("powershell.ps1")
                        .to_string_lossy()
                        .replace('\'', "''")
                ));
            }
            "cmd" => {
                let prompt = std::env::var("PROMPT").unwrap_or_else(|_| "$P$G".into());
                command.env("PROMPT", crate::integration::cmd_prompt(&prompt, &token));
            }
            _ => {
                command.arg("-i");
            }
        }
        let directory = files::expand_home(&directory);
        if !directory.is_dir() {
            bail!("Starting directory is unavailable: {}", directory.display());
        }
        command.cwd(&directory);
        let pty = portable_pty::native_pty_system().openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut child = pty.slave.spawn_command(command)?;
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader()?;
        let mut writer = pty.master.take_writer()?;
        let mut killer = child.clone_killer();
        let master = pty.master;
        self.events
            .try_send(Event::State(session.owner, ConnectionState::Connected))?;
        self.events.try_send(Event::Directory(
            session.owner,
            directory.to_string_lossy().into_owned(),
        ))?;
        let mut parser = HistoryParser::new(token);
        let mut bytes = vec![0u8; 64 * 1024];
        #[cfg(unix)]
        {
            use std::os::fd::RawFd;
            use std::sync::mpsc::{self, RecvTimeoutError};

            // Keep command handling and output parsing in one loop. The old
            // split writer/reader threads could resize the PTY while already
            // buffered output from the previous width was still unread.
            let fd: RawFd = master
                .as_raw_fd()
                .context("Local PTY does not expose a file descriptor")?;
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                bail!("Cannot set local PTY reader nonblocking");
            }
            let (command_tx, command_rx) = mpsc::channel();
            std::thread::spawn(move || {
                while let Some(command) = receiver.blocking_recv() {
                    if command_tx.send(command).is_err() {
                        break;
                    }
                }
            });
            let mut pending = None;
            'io: loop {
                let (alive, idle) =
                    self.drain_local_output(&session, &mut parser, &mut *reader, &mut bytes, 64)?;
                if !alive {
                    break;
                }
                let command = pending.take().or_else(|| command_rx.try_recv().ok());
                if let Some(command) = command {
                    match command {
                        Command::Input(data) => {
                            if writer
                                .write_all(&data)
                                .and_then(|_| writer.flush())
                                .is_err()
                            {
                                break;
                            }
                        }
                        Command::Resize(size) => {
                            // AppKit Zoom emits a stream of intermediate sizes.
                            // Let that stream settle before sending SIGWINCH:
                            // zsh redraws an RPROMPT asynchronously, and a
                            // second signal before the first redraw completes
                            // can otherwise leave fragments at both widths.
                            let mut size = size;
                            'settle_resize: loop {
                                let mut deadline = Instant::now() + LOCAL_RESIZE_SETTLE;
                                let mut disconnected = false;
                                loop {
                                    while let Ok(command) = command_rx.try_recv() {
                                        match command {
                                            Command::Resize(next) => {
                                                size = next;
                                                deadline = Instant::now() + LOCAL_RESIZE_SETTLE;
                                            }
                                            other => {
                                                pending = Some(other);
                                                break;
                                            }
                                        }
                                    }
                                    if pending.is_some() || Instant::now() >= deadline {
                                        break;
                                    }
                                    let (alive, idle) = self.drain_local_output(
                                        &session,
                                        &mut parser,
                                        &mut *reader,
                                        &mut bytes,
                                        64,
                                    )?;
                                    if !alive {
                                        disconnected = true;
                                        break;
                                    }
                                    if idle {
                                        let wait = (deadline - Instant::now())
                                            .min(Duration::from_millis(2));
                                        match command_rx.recv_timeout(wait) {
                                            Ok(Command::Resize(next)) => {
                                                size = next;
                                                deadline = Instant::now() + LOCAL_RESIZE_SETTLE;
                                            }
                                            Ok(other) => {
                                                pending = Some(other);
                                                break;
                                            }
                                            Err(RecvTimeoutError::Timeout) => {}
                                            Err(RecvTimeoutError::Disconnected) => {
                                                disconnected = true;
                                                break;
                                            }
                                        }
                                    }
                                }
                                if disconnected {
                                    break 'io;
                                }
                                loop {
                                    let (alive, _idle) = self.drain_local_output(
                                        &session,
                                        &mut parser,
                                        &mut *reader,
                                        &mut bytes,
                                        64,
                                    )?;
                                    if !alive {
                                        break 'io;
                                    }
                                    // Take one final bounded batch, then commit the geometry.
                                    // A continuously writing program must not starve resize;
                                    // remaining bytes are parsed under the same lock afterward.
                                    let mut newer_resize = false;
                                    if pending.is_none() {
                                        while let Ok(command) = command_rx.try_recv() {
                                            match command {
                                                Command::Resize(next) => {
                                                    size = next;
                                                    newer_resize = true;
                                                }
                                                other => {
                                                    pending = Some(other);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    if newer_resize {
                                        continue 'settle_resize;
                                    }
                                    break;
                                }
                                let _resize = session.resize_lock.lock();
                                if master
                                    .resize(portable_pty::PtySize {
                                        cols: size.cols as u16,
                                        rows: size.rows as u16,
                                        pixel_width: 0,
                                        pixel_height: 0,
                                    })
                                    .is_err()
                                {
                                    let _ = session.commands.send(Command::Resize(size));
                                    drop(_resize);
                                    break 'settle_resize;
                                }
                                let changed =
                                    session.terminal.lock().resize_local(size.cols, size.rows);
                                drop(_resize);
                                if changed {
                                    session.output_wakeup.mark();
                                    let _ = self.events.try_send(Event::Output(session.owner));
                                }
                                break 'settle_resize;
                            }
                        }
                        Command::Close => {
                            let _ = killer.kill();
                            break;
                        }
                    }
                    continue;
                }
                if idle {
                    match command_rx.recv_timeout(std::time::Duration::from_millis(2)) {
                        Ok(command) => pending = Some(command),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            }
        }
        #[cfg(not(unix))]
        {
            // Windows keeps the blocking reader/writer arrangement; ConPTY
            // resizing is covered by the existing platform-exempt baseline.
            let events = self.events.clone();
            let resize_lock = session.resize_lock.clone();
            let owner = session.owner;
            let resize_session = session.clone();
            std::thread::spawn(move || {
                let mut pending = None;
                loop {
                    let command = pending.take().or_else(|| receiver.blocking_recv());
                    let Some(command) = command else { break };
                    match command {
                        Command::Input(data) => {
                            if writer
                                .write_all(&data)
                                .and_then(|_| writer.flush())
                                .is_err()
                            {
                                break;
                            }
                        }
                        Command::Resize(size) => {
                            let mut size = size;
                            loop {
                                match receiver.try_recv() {
                                    Ok(Command::Resize(next)) => size = next,
                                    Ok(other) => {
                                        pending = Some(other);
                                        break;
                                    }
                                    Err(_) => break,
                                }
                            }
                            let _resize = resize_lock.lock();
                            let _ = master.resize(portable_pty::PtySize {
                                cols: size.cols as u16,
                                rows: size.rows as u16,
                                pixel_width: 0,
                                pixel_height: 0,
                            });
                            let changed =
                                resize_session.terminal.lock().resize(size.cols, size.rows);
                            drop(_resize);
                            if changed {
                                resize_session.output_wakeup.mark();
                                let _ = events.try_send(Event::Output(owner));
                            }
                        }
                        Command::Close => {
                            let _ = killer.kill();
                            break;
                        }
                    }
                }
            });
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(count) => self.feed(&session, &mut parser, &bytes[..count]),
                    Err(_) => break,
                }
            }
        }
        let _ = child.wait();
        session.cancel.cancel();
        let _ = session.commands.send(Command::Close);
        let _ = self
            .events
            .try_send(Event::State(session.owner, ConnectionState::Disconnected));
        Ok(())
    }
    async fn remote(
        &self,
        session: Arc<Session>,
        mut receiver: mpsc::UnboundedReceiver<Command>,
        secret: zeroize::Zeroizing<String>,
        remember: bool,
    ) -> Result<()> {
        let SessionSpec::Ssh { profile, .. } = &session.spec else {
            bail!("Expected SSH session");
        };
        let remote = crate::ssh::connect(
            session.owner,
            profile.clone(),
            secret,
            remember,
            self.database.clone(),
            self.events.clone(),
            session.cancel.clone(),
            self.vault.clone(),
        )
        .await?;
        *session.remote.lock() = Some(remote.clone());
        let mut channel = remote.handle.channel_open_session().await?;
        let size = session.terminal.lock().size;
        channel
            .request_pty(
                true,
                "xterm-256color",
                size.cols as u32,
                size.rows as u32,
                0,
                0,
                &[],
            )
            .await?;
        let token = Id::new_v4().to_string();
        session.terminal.lock().set_shell_token(token.clone());
        channel
            .exec(true, crate::integration::remote_command(&token))
            .await?;
        self.events
            .send(Event::State(session.owner, ConnectionState::Connected))
            .await?;
        let mut history = HistoryParser::new(token);
        loop {
            tokio::select! {
                _ = session.cancel.cancelled() => break,
                command = receiver.recv() => match command {
                    Some(Command::Input(data)) => channel.data(&data[..]).await?,
                    Some(Command::Resize(size)) => channel.window_change(size.cols as u32, size.rows as u32, 0, 0).await?,
                    Some(Command::Close) | None => break,
                },
                message = channel.wait() => match message {
                    Some(russh::ChannelMsg::Data { data } | russh::ChannelMsg::ExtendedData { data, .. }) => self.feed(&session, &mut history, &data),
                    Some(russh::ChannelMsg::Close) | None => break,
                    Some(russh::ChannelMsg::Failure) => bail!("Server refused the PTY or shell request"),
                    _ => {}
                }
            }
        }
        session.cancel.cancel();
        let _ = channel.close().await;
        let _ = remote
            .handle
            .disconnect(
                russh::Disconnect::ByApplication,
                "MantaSH session closed",
                "en",
            )
            .await;
        self.events
            .send(Event::State(session.owner, ConnectionState::Disconnected))
            .await?;
        Ok(())
    }
    #[cfg(unix)]
    fn drain_local_output(
        &self,
        session: &Session,
        history: &mut HistoryParser,
        reader: &mut dyn Read,
        bytes: &mut [u8],
        max_reads: usize,
    ) -> Result<(bool, bool)> {
        let mut reads = 0;
        loop {
            if reads >= max_reads {
                return Ok((true, false));
            }
            match reader.read(bytes) {
                Ok(0) => {
                    return Ok((false, true));
                }
                Ok(count) => {
                    reads += 1;
                    self.feed(session, history, &bytes[..count]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok((true, true));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn feed(&self, session: &Session, history: &mut HistoryParser, bytes: &[u8]) {
        if session.cancel.is_cancelled() {
            return;
        }
        // A local PTY resize holds this guard while changing both the kernel
        // endpoint and the emulator grid. Never parse bytes on either side of
        // that boundary with mismatched column counts.
        let _resize = session.resize_lock.lock();
        let mut terminal = session.terminal.lock();
        let malformed = terminal.feed(bytes);
        let events: Vec<_> = terminal.events.0.lock().drain(..).collect();
        // Mark under the grid lock: acknowledgement cannot race with unrecorded output.
        let wake = session.output_wakeup.mark();
        drop(terminal);
        if wake {
            let _ = self.events.try_send(Event::Output(session.owner));
        }
        if malformed {
            let _ = self.events.try_send(Event::EncodingWarning(session.owner));
        }
        for event in events {
            match event {
                alacritty_terminal::event::Event::PtyWrite(text) => {
                    session.input(text.into_bytes())
                }
                alacritty_terminal::event::Event::Title(text) => {
                    let _ = self.events.try_send(Event::Title(session.owner, text));
                }
                alacritty_terminal::event::Event::TextAreaSizeRequest(format) => {
                    let s = session.terminal.lock().size;
                    session.input(
                        format(alacritty_terminal::event::WindowSize {
                            num_lines: s.rows as u16,
                            num_cols: s.cols as u16,
                            cell_width: 8,
                            cell_height: 18,
                        })
                        .into_bytes(),
                    );
                }
                _ => {}
            }
        }
        for report in history.feed(bytes) {
            let _ = self.events.try_send(Event::Program(
                session.owner,
                crate::titles::reported_program(&report.command),
            ));
            if !report.directory.is_empty() {
                let _ = self
                    .events
                    .try_send(Event::Directory(session.owner, report.directory));
            }
            // Empty-command reports update cwd at the prompt, without adding history.
            if report.command.is_empty() {
                continue;
            }
            let entry = HistoryEntry {
                id: Id::new_v4(),
                scope: session.spec.scope(),
                command: report.command,
                timestamp: chrono::Utc::now().timestamp_millis(),
            };
            let _ = self.events.try_send(Event::History(entry.clone()));
            let db = self.database.clone();
            let events = self.events.clone();
            self.runtime.spawn_blocking(move || {
                if let Err(error) = db.lock().touch_history(&entry) {
                    let _ = events.try_send(Event::Error(error.to_string()));
                }
            });
        }
    }
    /// Get a specific live remote without relying on the active tab.
    pub fn get_remote(&self, owner: Owner) -> Result<Arc<Remote>> {
        let sessions = self.sessions.lock();
        let session = sessions.get(&owner.session).context("Session is closed")?;
        if session.owner != owner || session.cancel.is_cancelled() {
            bail!("Connection attempt is no longer available");
        }
        let remote = session
            .remote
            .lock()
            .clone()
            .context("SSH is not connected")?;
        Ok(remote)
    }
    /// Close one owner; a late close cannot terminate a replacement attempt.
    pub fn close(&self, owner: Owner) {
        let mut sessions = self.sessions.lock();
        if sessions
            .get(&owner.session)
            .is_some_and(|s| s.owner == owner)
        {
            if let Some(s) = sessions.remove(&owner.session) {
                s.close();
            }
        }
    }
    /// Shut down runtime endpoints before application exit.
    pub fn shutdown(&self) {
        for session in self.sessions.lock().values() {
            session.close();
        }
        for cancel in self.transfers.lock().values() {
            cancel.cancel();
        }
    }
    /// Persist connection metadata on a worker and acknowledge failures to the UI.
    pub fn save_profiles(&self, profiles: Vec<Profile>) {
        let db = self.database.clone();
        let events = self.events.clone();
        let revision = self.profiles_revision.fetch_add(1, Ordering::SeqCst) + 1;
        let latest = self.profiles_revision.clone();
        self.runtime.spawn_blocking(move || {
            let mut db = db.lock();
            if latest.load(Ordering::SeqCst) != revision {
                return;
            }
            let result = db.save_profiles(&profiles).map_err(|e| e.to_string());
            let _ = events.try_send(Event::ProfilesSaved(result));
        });
    }
    /// Persist layout in the background; writes are serialized by the database lock.
    pub fn save_workspace(&self, prefs: Preferences, workspace: Workspace) {
        let db = self.database.clone();
        let events = self.events.clone();
        let revision = self.layout_revision.fetch_add(1, Ordering::SeqCst) + 1;
        let latest = self.layout_revision.clone();
        self.runtime.spawn_blocking(move || {
            let mut db = db.lock();
            if latest.load(Ordering::SeqCst) != revision {
                return;
            }
            if let Err(error) = db.save_workspace(&prefs, &workspace) {
                let _ = events.try_send(Event::Error(error.to_string()));
            }
        });
    }
    /// Await this final write before quitting so a fast exit cannot lose the current layout.
    pub fn final_workspace(
        &self,
        prefs: Preferences,
        workspace: Workspace,
    ) -> tokio::task::JoinHandle<Result<()>> {
        self.layout_revision.fetch_add(1, Ordering::SeqCst);
        let db = self.database.clone();
        self.runtime
            .spawn_blocking(move || db.lock().save_workspace(&prefs, &workspace))
    }
    /// Remove selected history entries using an explicit list.
    pub fn delete_history(&self, ids: Vec<Id>) {
        let db = self.database.clone();
        let events = self.events.clone();
        self.runtime.spawn_blocking(move || {
            if let Err(error) = db.lock().delete_history(&ids) {
                let _ = events.try_send(Event::Error(error.to_string()));
            }
        });
    }
    /// Load a directory, tagging the response with a unique navigation request.
    pub fn list_files(&self, owner: Owner, request: Id, path: String) {
        let backend = self.clone();
        self.runtime.spawn(async move {
            let result = async { files::list(backend.get_remote(owner)?.as_ref(), &path).await }
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = backend
                .events
                .send(Event::Files {
                    owner,
                    request,
                    result,
                })
                .await;
        });
    }
    /// Open a fixed remote path; changing tabs while loading cannot retarget the result.
    pub fn open_file(
        &self,
        owner: Owner,
        request: Id,
        path: String,
        encoding: Option<crate::encoding::Encoding>,
    ) {
        let backend = self.clone();
        self.runtime.spawn(async move {
            if let Ok(remote) = backend.get_remote(owner) {
                if let Ok(Some((target, directory))) = files::link_target(&remote, &path).await {
                    let _ = backend
                        .events
                        .send(Event::FileLink {
                            owner,
                            request,
                            path,
                            target,
                            directory,
                        })
                        .await;
                    return;
                }
            }
            let result =
                async { files::open(backend.get_remote(owner)?.as_ref(), &path, encoding).await }
                    .await
                    .map_err(|e| format!("{e:#}"));
            let _ = backend
                .events
                .send(Event::FileOpened {
                    owner,
                    request,
                    result,
                })
                .await;
        });
    }
    /// Save a captured editor revision; the view clears dirty only if it still matches.
    pub fn save_file(
        &self,
        owner: Owner,
        document: Id,
        revision: u64,
        original: OpenedFile,
        text: String,
        encoding: crate::encoding::Encoding,
        force: bool,
    ) {
        let backend = self.clone();
        self.runtime.spawn(async move {
            let result = async {
                files::save(
                    backend.get_remote(owner)?.as_ref(),
                    &original,
                    &text,
                    encoding,
                    force,
                )
                .await
            }
            .await
            .map_err(|e| format!("{e:#}"));
            let _ = backend
                .events
                .send(Event::FileSaved {
                    owner,
                    document,
                    revision,
                    result,
                })
                .await;
        });
    }
    /// Run a file mutation with immutable targets and a fresh operation ID.
    pub fn file_operation(&self, owner: Owner, request: Id, operation: FileOperation) {
        let backend = self.clone();
        self.runtime.spawn(async move {
            let result = async {
                let remote = backend.get_remote(owner)?;
                match operation {
                    FileOperation::Delete(paths) => files::delete(&remote, &paths).await?,
                    FileOperation::Mkdir(path) => remote.sftp().await?.create_dir(path).await?,
                    FileOperation::Rename(from, to) => {
                        let sftp = remote.sftp().await?;
                        if sftp.try_exists(&to).await? {
                            bail!("Rename target already exists");
                        }
                        sftp.rename(from, to).await?;
                    }
                    // SFTP has no server-side copy; the same verified session
                    // runs a quoted `cp -R` instead, checked by exit status.
                    FileOperation::Copy(sources, target) => {
                        let mut command = String::from("cp -R --");
                        for source in &sources {
                            command.push(' ');
                            command.push_str(&shell_quote(source));
                        }
                        command.push(' ');
                        command.push_str(&shell_quote(&target));
                        remote.run(&command).await?
                    }
                }
                Ok::<_, anyhow::Error>(())
            }
            .await
            .map_err(|e| format!("{e:#}"));
            let _ = backend
                .events
                .send(Event::FileOperation {
                    owner,
                    request,
                    result,
                })
                .await;
        });
    }
    /// Sample only on explicit demand; the UI schedules this while the system tool is visible.
    pub fn sample(&self, owner: Owner, request: Id, previous: Option<crate::monitor::Sample>) {
        let backend = self.clone();
        self.runtime.spawn(async move {
            let result = async {
                let output = backend
                    .get_remote(owner)?
                    .exec(crate::monitor::SAMPLE_COMMAND)
                    .await?;
                Ok::<_, anyhow::Error>(crate::monitor::parse(
                    &output,
                    previous.as_ref(),
                    chrono::Utc::now().timestamp(),
                ))
            }
            .await
            .map_err(|e| format!("{e:#}"));
            let _ = backend
                .events
                .send(Event::Monitor {
                    owner,
                    request,
                    result,
                })
                .await;
        });
    }
    /// Fetch details for one captured process identity, independent of the active tab.
    pub fn process_details(&self, owner: Owner, request: Id, identity: crate::processes::Identity) {
        let backend = self.clone();
        self.runtime.spawn(async move {
            let result = async {
                let command = crate::processes::details_command(&identity)?;
                let output = backend.get_remote(owner)?.exec(&command).await?;
                crate::processes::details(&output)
            }
            .await
            .map_err(|e| format!("{e:#}"));
            let _ = backend
                .events
                .send(Event::ProcessDetails {
                    owner,
                    request,
                    result,
                })
                .await;
        });
    }
    /// Execute only an allowlisted signal against the original connection attempt and identity.
    pub fn process_action(
        &self,
        owner: Owner,
        request: Id,
        identity: crate::processes::Identity,
        action: crate::processes::Action,
    ) {
        let backend = self.clone();
        self.runtime.spawn(async move {
            let result = async {
                let command = crate::processes::signal_command(&identity, action)?;
                let output = backend.get_remote(owner)?.exec(&command).await?;
                Ok::<_, anyhow::Error>(crate::processes::outcome(&output))
            }
            .await
            .map_err(|e| format!("{e:#}"));
            let _ = backend
                .events
                .send(Event::ProcessAction {
                    owner,
                    request,
                    result,
                })
                .await;
        });
    }
    /// Queue a transfer independently from selection, with two concurrent workers.
    pub fn transfer(&self, owner: Owner, mut record: TransferRecord, overwrite: bool) {
        let cancel = CancellationToken::new();
        self.transfers.lock().insert(record.id, cancel.clone());
        let backend = self.clone();
        self.runtime.spawn(async move {
            let initial = record.clone(); let db = backend.database.clone();
            if let Err(error) = tokio::task::spawn_blocking(move || db.lock().save_transfer(&initial)).await.context("Transfer history worker failed").and_then(|r| r) {
                let _ = backend.events.send(Event::Error(format!("Transfer history could not be stored: {error}"))).await;
            }
            let result = async {
                let remote = backend.get_remote(owner)?;
                let _permit = tokio::select! { _ = cancel.cancelled() => bail!("Transfer cancelled"), permit = backend.permits.acquire() => permit? };
                record.state = TransferState::Running; let _ = backend.events.try_send(Event::Transfer(record.clone()));
                let mut last = std::time::Instant::now();
                let report = Mutex::new(&mut last);
                files::transfer(remote, &mut record, overwrite, cancel.clone(), |r| {
                    let mut last = report.lock(); if last.elapsed().as_millis() > 150 { **last = std::time::Instant::now(); let _ = backend.events.try_send(Event::Transfer(r.clone())); }
                }).await
            }.await;
            // Removing under the same lock used by cancel establishes one terminal outcome.
            backend.transfers.lock().remove(&record.id);
            record.state = if cancel.is_cancelled() { TransferState::Cancelled } else if result.is_ok() { TransferState::Completed } else { TransferState::Failed };
            record.error = result.err().map(|e| format!("{e:#}"));
            let db = backend.database.clone(); let saved = record.clone();
            if let Err(error) = tokio::task::spawn_blocking(move || db.lock().save_transfer(&saved)).await.context("Transfer history worker failed").and_then(|r| r) {
                let _ = backend.events.send(Event::Error(format!("Transfer history could not be stored: {error}"))).await;
            }
            let _ = backend.events.send(Event::Transfer(record)).await;
        });
    }
    /// Cancellation is monotonic; task progress is ignored by the UI after this action.
    pub fn cancel_transfer(&self, id: Id) -> bool {
        if let Some(cancel) = self.transfers.lock().get(&id) {
            cancel.cancel();
            true
        } else {
            false
        }
    }
}

pub enum FileOperation {
    Delete(Vec<String>),
    Mkdir(String),
    Rename(String, String),
    /// Copy source entries verbatim into the target directory.
    Copy(Vec<String>, String),
}

/// Quote one path for a POSIX shell command line: single-quote wrapping with
/// the embedded-quote escape, so no metacharacter in a name can break out.
fn shell_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}
