//! Stable official-release updates. Downloading never installs or stops the app.
//! Installation handoff is separate: save the workspace, call `launch_install` on
//! a blocking worker, and only then shut down. Success means handoff, not install.
#![cfg_attr(test, allow(dead_code))]
use anyhow::{Context, Result, bail, ensure};
use reqwest::{Client, Url};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

const LATEST: &str = "https://api.github.com/repos/realmx/MantaSH/releases/latest";
const API_LIMIT: u64 = 1024 * 1024;
const PACKAGE_LIMIT: u64 = 1024 * 1024 * 1024;
const CHECKSUM_LIMIT: u64 = 256;
const BUNDLE_ID: &str = "app.mantash.MantaSH";

/// Cloneable HTTP service; there is no configurable production release source.
#[derive(Clone)]
pub struct Updater {
    api: Client,
    assets: Client,
    #[cfg(test)]
    fixture: Option<Url>,
    #[cfg(test)]
    fixture_directory: Option<std::sync::Arc<TempDir>>,
}

/// An authenticated-by-origin release selection. Download fields cannot be supplied by UI code.
#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    package: Asset,
    checksum: Asset,
    platform: String,
}

/// A verified package in a private temporary directory; dropping it cancels staging.
#[derive(Debug)]
pub struct DownloadedUpdate {
    directory: TempDir,
    package: PathBuf,
    release: Release,
}

/// A preflighted installation. Drop cleans up until successful helper handoff.
#[derive(Debug)]
pub struct PreparedUpdate {
    directory: Option<TempDir>,
    target: PathBuf,
    log: PathBuf,
}

impl PreparedUpdate {
    /// Persistent helper log, available before handoff, including failures and retained staging paths.
    pub fn log_path(&self) -> &Path {
        &self.log
    }

    fn staging_path(&self) -> &Path {
        self.directory
            .as_ref()
            .expect("Staging ownership not transferred")
            .path()
    }
}

impl Drop for PreparedUpdate {
    fn drop(&mut self) {
        if let Some(directory) = self.directory.take() {
            let log = self.log.clone();
            let cleanup = move || {
                let path = directory.path().to_owned();
                if let Err(error) = directory.close() {
                    use std::io::Write;
                    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(log) {
                        let _ = writeln!(
                            file,
                            "ERROR: Staging cleanup failed at {}: {error}",
                            path.display()
                        );
                    }
                }
            };
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn_blocking(cleanup);
            } else {
                let _ = std::thread::Builder::new()
                    .name("mantash-update-cleanup".into())
                    .spawn(cleanup);
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

impl Updater {
    pub fn new() -> Result<Self> {
        let api = Client::builder()
            .user_agent(concat!("MantaSH/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let assets = Client::builder()
            .user_agent(concat!("MantaSH/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(1800))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let url = attempt.url();
                if attempt.previous().len() >= 5 || !official_cdn(url) {
                    attempt.error("Update redirect is not an official HTTPS release CDN")
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        Ok(Self {
            api,
            assets,
            #[cfg(test)]
            fixture: None,
            #[cfg(test)]
            fixture_directory: None,
        })
    }

    /// Check GitHub's stable latest release for this binary's OS/architecture.
    /// Pass `env!("CARGO_PKG_VERSION")`; malformed versions are errors, not lexical comparisons.
    pub async fn check(&self, current_version: &str) -> Result<Option<Release>> {
        self.check_platform(
            current_version,
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
        .await
    }

    async fn check_platform(&self, current: &str, os: &str, arch: &str) -> Result<Option<Release>> {
        Version::parse(current).context("Invalid current application semver")?;
        if platform_name(os, arch).is_none() {
            return Ok(None);
        }
        let url = self.request_url(LATEST)?;
        let response = self
            .api
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let body = limited_body(response, API_LIMIT, None, &CancellationToken::new()).await?;
        select_release(&body, current, os, arch)
    }

    /// Stream into private staging, enforce declared size, and verify the official `.sha256`.
    /// Cancellation or any failure drops the temporary directory. Progress is bytes/total.
    pub async fn download(
        &self,
        release: Release,
        cancel: CancellationToken,
        progress: impl Fn(u64, u64) + Send + Sync + 'static,
    ) -> Result<DownloadedUpdate> {
        validate_asset(&release.package, &release.version, PACKAGE_LIMIT)?;
        validate_asset(&release.checksum, &release.version, CHECKSUM_LIMIT)?;
        ensure!(!cancel.is_cancelled(), "Update download cancelled");
        #[cfg(test)]
        let parent = self
            .fixture_directory
            .as_ref()
            .map(|directory| directory.path().to_owned());
        #[cfg(not(test))]
        let parent: Option<PathBuf> = None;
        let directory = tokio::task::spawn_blocking(move || {
            private_directory(parent.as_deref(), "mantash-update-")
        })
        .await??;
        let checksum_response = tokio::select! {
            _ = cancel.cancelled() => bail!("Update download cancelled"),
            response = self.assets.get(self.request_url(&release.checksum.browser_download_url)?).send() => response?,
        };
        let checksum = limited_body(
            checksum_response,
            CHECKSUM_LIMIT,
            Some(release.checksum.size),
            &cancel,
        )
        .await?;
        let expected_hash = parse_checksum(&checksum, &release.package.name)?;
        let mut response = tokio::select! {
            _ = cancel.cancelled() => bail!("Update download cancelled"),
            response = self.assets.get(self.request_url(&release.package.browser_download_url)?).send() => response?.error_for_status()?,
        };
        check_length(&response, PACKAGE_LIMIT, Some(release.package.size))?;
        let package = directory.path().join(&release.package.name);
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&package)
            .await?;
        let mut hash = Sha256::new();
        let mut bytes = 0_u64;
        progress(0, release.package.size);
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => bail!("Update download cancelled"),
                chunk = response.chunk() => chunk?,
            };
            let Some(chunk) = chunk else { break };
            bytes = bytes
                .checked_add(chunk.len() as u64)
                .context("Download size overflow")?;
            ensure!(
                bytes <= release.package.size && bytes <= PACKAGE_LIMIT,
                "Update package exceeds declared size"
            );
            hash.update(&chunk);
            file.write_all(&chunk).await?;
            // Finish pending filesystem work before cancellation can remove the directory on Windows.
            file.flush().await?;
            progress(bytes, release.package.size);
        }
        ensure!(!cancel.is_cancelled(), "Update download cancelled");
        ensure!(bytes == release.package.size, "Truncated update package");
        ensure!(
            <[u8; 32]>::from(hash.finalize()) == expected_hash,
            "Update package SHA-256 mismatch"
        );
        file.flush().await?;
        ensure!(!cancel.is_cancelled(), "Update download cancelled");
        drop(file);
        Ok(DownloadedUpdate {
            directory,
            package,
            release,
        })
    }

    fn request_url(&self, official: &str) -> Result<Url> {
        #[cfg(test)]
        if let Some(base) = &self.fixture {
            return base
                .join(if official == LATEST {
                    "latest"
                } else {
                    official
                        .rsplit('/')
                        .next()
                        .context("Missing asset filename")?
                })
                .map_err(Into::into);
        }
        Url::parse(official).map_err(Into::into)
    }

    #[cfg(test)]
    pub(crate) fn fixture(base: &str) -> Result<Self> {
        let base = Url::parse(base)?;
        ensure!(
            base.scheme() == "http"
                && matches!(base.host_str(), Some("127.0.0.1" | "[::1]" | "localhost")),
            "Fixture must be loopback HTTP"
        );
        let mut updater = Self::new()?;
        updater.api = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        updater.assets = updater.api.clone();
        updater.fixture_directory = Some(std::sync::Arc::new(private_directory(
            None,
            "mantash-fixture-",
        )?));
        updater.fixture = Some(base);
        Ok(updater)
    }

    #[cfg(test)]
    pub(crate) fn fixture_staging(&self) -> &Path {
        self.fixture_directory
            .as_ref()
            .expect("Fixture root required")
            .path()
    }

    #[cfg(test)]
    pub(crate) async fn fixture_check(
        &self,
        current: &str,
        os: &str,
        arch: &str,
    ) -> Result<Option<Release>> {
        self.check_platform(current, os, arch).await
    }
}

/// Whether official release artifacts exist for the running binary's platform.
pub fn current_supported() -> bool {
    platform_name(std::env::consts::OS, std::env::consts::ARCH).is_some()
}

fn platform_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("macos-arm64"),
        ("macos", "x86_64") => Some("macos-x64"),
        ("windows", "x86") => Some("windows-x86"),
        ("windows", "x86_64") => Some("windows-x64"),
        ("windows", "aarch64") => Some("windows-arm64"),
        _ => None,
    }
}

fn select_release(body: &[u8], current: &str, os: &str, arch: &str) -> Result<Option<Release>> {
    let current = Version::parse(current)?;
    let Some(platform) = platform_name(os, arch) else {
        return Ok(None);
    };
    let release: ApiRelease =
        serde_json::from_slice(body).context("Invalid GitHub release metadata")?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let version = release
        .tag_name
        .strip_prefix('v')
        .context("Release tag must start with v")?;
    let parsed = Version::parse(version).context("Invalid release semver")?;
    if !parsed.pre.is_empty() || !parsed.build.is_empty() {
        return Ok(None);
    }
    if parsed <= current {
        return Ok(None);
    }
    let suffix = if os == "macos" { ".dmg" } else { "-setup.exe" };
    let name = format!("MantaSH-{version}-{platform}{suffix}");
    let find = |name: &str| -> Result<Option<Asset>> {
        let mut assets = release.assets.iter().filter(|asset| asset.name == name);
        let asset = assets.next().cloned();
        ensure!(assets.next().is_none(), "Duplicate release asset: {name}");
        Ok(asset)
    };
    let (Some(package), Some(checksum)) = (find(&name)?, find(&format!("{name}.sha256"))?) else {
        return Ok(None);
    };
    validate_asset(&package, version, PACKAGE_LIMIT)?;
    validate_asset(&checksum, version, CHECKSUM_LIMIT)?;
    Ok(Some(Release {
        version: version.to_owned(),
        package,
        checksum,
        platform: platform.to_owned(),
    }))
}

fn validate_asset(asset: &Asset, version: &str, cap: u64) -> Result<()> {
    ensure!(
        asset.size > 0 && asset.size <= cap,
        "Invalid update asset size"
    );
    let url = Url::parse(&asset.browser_download_url)?;
    ensure!(
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Update asset is not an official HTTPS URL"
    );
    let parts: Vec<_> = url.path().split('/').collect();
    ensure!(
        parts.len() == 7
            && parts[0].is_empty()
            && parts[1] == "realmx"
            && parts[2].eq_ignore_ascii_case("MantaSH")
            && parts[3] == "releases"
            && parts[4] == "download"
            && parts[5] == format!("v{version}")
            && parts[6] == asset.name,
        "Update asset URL does not match repository, tag and filename"
    );
    ensure!(
        asset
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._".contains(&byte)),
        "Invalid update asset filename"
    );
    Ok(())
}

fn official_cdn(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com")
        )
}

fn check_length(response: &reqwest::Response, cap: u64, expected: Option<u64>) -> Result<()> {
    if let Some(length) = response.content_length() {
        ensure!(length <= cap, "Update response exceeds size limit");
        if let Some(expected) = expected {
            ensure!(
                length == expected,
                "Update content-length differs from release metadata"
            );
        }
    }
    Ok(())
}

async fn limited_body(
    mut response: reqwest::Response,
    cap: u64,
    expected: Option<u64>,
    cancel: &CancellationToken,
) -> Result<Vec<u8>> {
    response.error_for_status_ref()?;
    check_length(&response, cap, expected)?;
    let mut body = Vec::new();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => bail!("Update download cancelled"),
            chunk = response.chunk() => chunk?,
        };
        let Some(chunk) = chunk else { break };
        ensure!(
            body.len() as u64 + chunk.len() as u64 <= cap,
            "Update response exceeds size limit"
        );
        body.extend_from_slice(&chunk);
    }
    if let Some(expected) = expected {
        ensure!(body.len() as u64 == expected, "Truncated update response");
    }
    Ok(body)
}

fn parse_checksum(body: &[u8], name: &str) -> Result<[u8; 32]> {
    ensure!(
        body.len() == 64 + 2 + name.len() + 1
            && &body[64..66] == b"  "
            && &body[66..body.len() - 1] == name.as_bytes()
            && body.last() == Some(&b'\n'),
        "Invalid checksum format or filename"
    );
    let mut result = [0_u8; 32];
    for (index, pair) in body[..64].chunks_exact(2).enumerate() {
        let digit = |b: u8| -> Result<u8> {
            match b {
                b'0'..=b'9' => Ok(b - b'0'),
                b'a'..=b'f' => Ok(b - b'a' + 10),
                _ => bail!("Checksum must be lowercase SHA-256 hex"),
            }
        };
        result[index] = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Ok(result)
}

/// Persistent logs under MantaSH's normal local application-data directory `/updates`.
/// Logs are not proof of successful installation; inspect their final status.
pub fn log_directory() -> Result<PathBuf> {
    directories::ProjectDirs::from("app", "MantaSH", "MantaSH")
        .map(|dirs| dirs.data_local_dir().join("updates"))
        .context("Cannot locate MantaSH update logs")
}

/// Preflight and prepare only. Blocking system/file work runs outside the async executor.
/// Development binaries, translocated bundles and non-Inno Windows installs are rejected.
pub async fn prepare_install(download: DownloadedUpdate) -> Result<PreparedUpdate> {
    tokio::task::spawn_blocking(move || prepare(download)).await?
}

fn prepare(download: DownloadedUpdate) -> Result<PreparedUpdate> {
    let executable = std::env::current_exe()?.canonicalize()?;
    ensure!(
        download.release.platform
            == platform_name(std::env::consts::OS, std::env::consts::ARCH).unwrap_or("unsupported"),
        "Update is for another platform"
    );
    #[cfg(target_os = "macos")]
    let (directory, target) = prepare_macos(download, &executable)?;
    #[cfg(target_os = "windows")]
    let (directory, target) = prepare_windows(download, &executable)?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (download, executable);
        bail!("Automatic installation is unsupported on this platform");
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let log_directory = log_directory()?;
        std::fs::create_dir_all(&log_directory)?;
        let log = log_directory.join(format!("update-{}.log", uuid::Uuid::new_v4()));
        let mut log_options = std::fs::OpenOptions::new();
        log_options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            log_options.mode(0o600);
        }
        log_options.open(&log)?;
        std::fs::write(directory.path().join("owned"), b"MantaSH update staging\n")?;
        Ok(PreparedUpdate {
            directory: Some(directory),
            target,
            log,
        })
    }
}

/// Start a helper and wait for its ready/commit handshake. Call on a blocking worker.
/// The helper holds this process's identity, waits at most five minutes for actual exit,
/// and then installs. Only successful handoff permits the caller to quit the app.
pub fn launch_install(mut prepared: PreparedUpdate) -> Result<()> {
    let mut command = helper_command(&prepared)?;
    let output = std::fs::OpenOptions::new()
        .append(true)
        .open(&prepared.log)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(output.try_clone()?))
        .stderr(Stdio::from(output));
    let mut child = command.spawn().with_context(|| {
        format!(
            "Cannot start update helper; log: {}",
            prepared.log.display()
        )
    })?;
    let handoff = (|| -> Result<()> {
        wait_for_helper(&mut child, &prepared.staging_path().join("ready"))?;
        std::fs::write(
            prepared.staging_path().join("commit"),
            b"install after parent exit\n",
        )?;
        wait_for_helper(&mut child, &prepared.staging_path().join("accepted"))?;
        ensure!(
            child.try_wait()?.is_none(),
            "Update helper exited during handoff"
        );
        Ok(())
    })();
    if let Err(error) = handoff {
        // The parent is still alive, so stopping this helper cannot interrupt installation.
        let _ = child.kill();
        let _ = child.wait();
        return Err(error)
            .with_context(|| format!("Update handoff failed; log: {}", prepared.log.display()));
    }
    let _ = prepared
        .directory
        .take()
        .context("Missing prepared staging")?
        .keep();
    Ok(())
}

fn wait_for_helper(child: &mut std::process::Child, marker: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !marker.is_file() {
        if let Some(status) = child.try_wait()? {
            bail!("Update helper exited before acknowledgement ({status})");
        }
        ensure!(
            Instant::now() < deadline,
            "Update helper acknowledgement timed out"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}

fn helper_command(prepared: &PreparedUpdate) -> Result<Command> {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("/bin/bash");
        command.arg(prepared.staging_path().join("helper.sh"));
        command
            .arg(prepared.staging_path())
            .arg(&prepared.target)
            .arg(std::process::id().to_string())
            .arg(&prepared.log);
        Ok(command)
    }
    #[cfg(target_os = "windows")]
    {
        powershell_command(
            WINDOWS_HELPER,
            serde_json::json!({
                "Stage": prepared.staging_path(), "Target": prepared.target,
                "ParentId": std::process::id(), "Log": prepared.log,
                "HelperPath": prepared.staging_path().join("helper.ps1"),
            }),
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = prepared;
        bail!("Automatic installation is unsupported on this platform")
    }
}

fn checked_command(command: &mut Command) -> Result<std::process::Output> {
    let output = command.stdin(Stdio::null()).output()?;
    ensure!(
        output.status.success(),
        "{} failed: {}",
        command.get_program().to_string_lossy(),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

fn private_directory(parent: Option<&Path>, prefix: &str) -> Result<TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = match parent {
        Some(parent) => builder.tempdir_in(parent)?,
        None => builder.tempdir()?,
    };
    #[cfg(target_os = "windows")]
    checked_command(&mut powershell_command(
        WINDOWS_PRIVACY,
        serde_json::json!({"Stage": directory.path()}),
    )?)?;
    Ok(directory)
}

#[cfg(any(target_os = "windows", test))]
fn encoded_powershell(script: &str, parameters: serde_json::Value) -> Result<String> {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&parameters)?);
    // Encoded data is deserialized and splatted as values, never evaluated as source.
    // -EncodedCommand does not change Windows execution policy (no Bypass flags).
    let source = format!(
        "$ErrorActionPreference='Stop'; $data=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{data}')) | ConvertFrom-Json; $arguments=@{{}}; foreach($property in $data.PSObject.Properties) {{$arguments[$property.Name]=$property.Value}}; & {{\n{script}\n}} @arguments"
    );
    let utf16: Vec<u8> = source.encode_utf16().flat_map(u16::to_le_bytes).collect();
    Ok(base64::engine::general_purpose::STANDARD.encode(utf16))
}

#[cfg(any(target_os = "windows", test))]
fn powershell_command(script: &str, parameters: serde_json::Value) -> Result<Command> {
    let mut command = Command::new(powershell()?);
    command
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(encoded_powershell(script, parameters)?);
    Ok(command)
}

#[cfg(any(target_os = "windows", test))]
fn windows_path(path: &Path) -> Result<PathBuf> {
    let text = path
        .to_str()
        .context("Non-Unicode installation path is unsupported")?;
    let text = text.strip_prefix("\\\\?\\").unwrap_or(text);
    let bytes = text.as_bytes();
    ensure!(
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\',
        "Automatic update requires a local installed Windows drive path"
    );
    Ok(PathBuf::from(text))
}

#[cfg(target_os = "macos")]
fn mac_target(executable: &Path, home: &Path) -> Result<PathBuf> {
    let suffix = Path::new("MantaSH.app/Contents/MacOS/mantash");
    for parent in [PathBuf::from("/Applications"), home.join("Applications")] {
        let expected = parent.join(suffix);
        if executable == expected {
            ensure!(
                expected.canonicalize()? == expected,
                "Symlinked installations are unsupported"
            );
            return Ok(parent.join("MantaSH.app"));
        }
    }
    bail!(
        "Automatic update requires /Applications/MantaSH.app or ~/Applications/MantaSH.app; development or translocated binaries cannot be replaced"
    )
}

#[cfg(target_os = "macos")]
fn validate_bundle(app: &Path, version: &str, platform: &str) -> Result<()> {
    ensure!(
        !std::fs::symlink_metadata(app)?.file_type().is_symlink(),
        "Linked app bundle is unsupported"
    );
    let output = checked_command(
        Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(app.join("Contents/Info.plist")),
    )?;
    let info: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    ensure!(
        info["CFBundleIdentifier"] == BUNDLE_ID
            && info["CFBundleExecutable"] == "mantash"
            && info["CFBundlePackageType"] == "APPL"
            && info["CFBundleShortVersionString"] == version
            && info["CFBundleVersion"] == version,
        "Unexpected MantaSH bundle identity or version"
    );
    validate_build(
        &app.join("Contents/Resources/build-info.json"),
        &app.join("Contents/MacOS/mantash"),
        version,
        platform,
    )?;
    checked_command(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(app),
    )?;
    Ok(())
}

fn validate_build(manifest: &Path, executable: &Path, version: &str, platform: &str) -> Result<()> {
    let file = std::fs::File::open(manifest)?;
    ensure!(
        file.metadata()?.len() <= 16 * 1024,
        "Oversized installed build manifest"
    );
    let info: serde_json::Value = serde_json::from_reader(file)?;
    let target = match platform {
        "macos-arm64" => "aarch64-apple-darwin",
        "macos-x64" => "x86_64-apple-darwin",
        "windows-x86" => "i686-pc-windows-msvc",
        "windows-x64" => "x86_64-pc-windows-msvc",
        "windows-arm64" => "aarch64-pc-windows-msvc",
        _ => bail!("Unsupported build target"),
    };
    ensure!(
        info["product"] == "MantaSH"
            && info["version"] == version
            && info["target"] == target
            && info["profile"] == "release",
        "Current application is not the expected packaged release"
    );
    let mut source = std::fs::File::open(executable)?;
    ensure!(
        source.metadata()?.len() <= PACKAGE_LIMIT,
        "Oversized app executable"
    );
    // package_release.py records the source binary hash before macOS bundle signing,
    // which changes the Mach-O. validate_bundle verifies the signed app instead.
    // Windows packaging does not transform the executable after this hash is recorded.
    if platform.starts_with("windows-") {
        let mut hash = Sha256::new();
        std::io::copy(&mut source, &mut hash)?;
        ensure!(
            info["binary_sha256"] == format!("{:x}", hash.finalize()),
            "Packaged executable integrity mismatch"
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn prepare_macos(download: DownloadedUpdate, executable: &Path) -> Result<(TempDir, PathBuf)> {
    let home = directories::BaseDirs::new().context("Cannot locate home directory")?;
    let target = mac_target(executable, home.home_dir())?;
    validate_bundle(
        &target,
        env!("CARGO_PKG_VERSION"),
        &download.release.platform,
    )?;
    // Same-filesystem staging makes replacement/rollback rename operations atomic.
    let directory = private_directory(
        Some(target.parent().context("Missing installation parent")?),
        ".mantash-update-",
    )
    .context("Installation directory is not writable; update requires normal OS permissions")?;
    let mount = download.directory.path().join("mount");
    std::fs::create_dir(&mount)?;
    checked_command(
        Command::new("/usr/bin/hdiutil")
            .args([
                "attach",
                "-readonly",
                "-nobrowse",
                "-noautoopen",
                "-mountpoint",
            ])
            .arg(&mount)
            .arg(&download.package),
    )?;
    let mounted = (|| -> Result<()> {
        let source = mount.join("MantaSH.app");
        validate_bundle(
            &source,
            &download.release.version,
            &download.release.platform,
        )?;
        checked_command(
            Command::new("/usr/bin/ditto")
                .args(["--rsrc", "--extattr", "--acl"])
                .arg(&source)
                .arg(directory.path().join("MantaSH.app")),
        )?;
        validate_bundle(
            &directory.path().join("MantaSH.app"),
            &download.release.version,
            &download.release.platform,
        )?;
        Ok(())
    })();
    if let Err(error) = checked_command(Command::new("/usr/bin/hdiutil").arg("detach").arg(&mount))
    {
        let retained = download.directory.keep();
        bail!(
            "Cannot detach readonly update image at {}; staging retained at {}: {error:#}",
            mount.display(),
            retained.display()
        );
    }
    mounted?;
    std::fs::write(directory.path().join("helper.sh"), MAC_HELPER)?;
    Ok((directory, target))
}

#[cfg(any(target_os = "windows", test))]
fn powershell() -> Result<PathBuf> {
    let root = std::env::var_os("SystemRoot").context("Missing Windows system directory")?;
    let path = PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    ensure!(path.is_file(), "Windows PowerShell is unavailable");
    Ok(path)
}

#[cfg(any(target_os = "windows", test))]
fn prepare_windows(download: DownloadedUpdate, executable: &Path) -> Result<(TempDir, PathBuf)> {
    let executable = windows_path(executable)?;
    ensure!(
        executable
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("mantash.exe")),
        "Development executable cannot be updated"
    );
    let target = executable
        .parent()
        .context("Missing installation directory")?
        .to_owned();
    validate_build(
        &target.join("build-info.json"),
        &executable,
        env!("CARGO_PKG_VERSION"),
        &download.release.platform,
    )?;
    let preflight = download.directory.path().join("preflight.ps1");
    std::fs::write(&preflight, WINDOWS_PREFLIGHT)?;
    checked_command(&mut powershell_command(
        WINDOWS_PREFLIGHT,
        serde_json::json!({"Target": target}),
    )?)?;
    // This Inno package uses PrivilegesRequired=lowest: do not promise elevation for protected installs.
    tempfile::NamedTempFile::new_in(&target).context("Installation directory is not writable; use normal OS authorization or the official installer")?;
    std::fs::rename(
        &download.package,
        download.directory.path().join("installer.exe"),
    )?;
    std::fs::write(
        download.directory.path().join("version"),
        &download.release.version,
    )?;
    std::fs::write(
        download.directory.path().join("platform"),
        &download.release.platform,
    )?;
    std::fs::write(download.directory.path().join("helper.ps1"), WINDOWS_HELPER)?;
    Ok((download.directory, target))
}

// All dynamic paths travel as positional arguments, never interpolated shell source.
#[cfg(any(target_os = "macos", test))]
const MAC_HELPER: &str = r#"#!/bin/bash
set -u
stage=$1
target=$2
parent=$3
log=$4
backup="$stage/previous.app"
preserve=0
exec >>"$log" 2>&1
printf 'Helper started; staging: %s\n' "$stage"
cleanup() {
    status=$?
    trap - EXIT
    if [[ -e "$backup" ]]; then
        if [[ ! -e "$target" ]]; then /bin/mv "$backup" "$target" || preserve=1; else preserve=1; fi
    fi
    if [[ "$preserve" == 0 && "$stage" == */.mantash-update-* && ! -L "$stage" && -f "$stage/owned" && "$0" == "$stage/helper.sh" ]]; then
        /bin/rm -rf -- "$stage"
    else
        printf 'Staging retained for recovery: %s\n' "$stage"
    fi
    printf 'Helper finished with status %s\n' "$status"
    exit "$status"
}
trap cleanup EXIT
trap 'echo "Helper interrupted"; exit 1' HUP INT TERM
fail() { printf 'ERROR: %s\n' "$1"; exit 1; }
[[ "$stage" == */.mantash-update-* && -f "$stage/owned" && "$0" == "$stage/helper.sh" && ! -L "$target" ]] || fail 'Invalid staging or target'
identity=$(/bin/ps -p "$parent" -o lstart=) || fail 'Parent disappeared before handoff'
[[ -n "$identity" ]] || fail 'Parent identity unavailable'
/usr/bin/touch "$stage/ready" || fail 'Cannot acknowledge handoff'
deadline=$((SECONDS + 30))
while [[ ! -f "$stage/commit" ]]; do
    (( SECONDS < deadline )) || fail 'Handoff was not committed'
    /bin/sleep 0.1
done
/usr/bin/touch "$stage/accepted" || fail 'Cannot acknowledge committed handoff'
deadline=$((SECONDS + 300))
while true; do
    next_identity=$(/bin/ps -p "$parent" -o lstart= 2>/dev/null)
    if [[ "$next_identity" != "$identity" ]]; then
        if [[ -z "$next_identity" ]] && /bin/kill -0 "$parent" 2>/dev/null; then fail 'Cannot verify application exit'; fi
        break
    fi
    (( SECONDS < deadline )) || fail 'Application did not exit within five minutes; no installation performed'
    /bin/sleep 0.2
done
echo 'Original application process exited'
/usr/bin/codesign --verify --deep --strict "$stage/MantaSH.app" || fail 'Staged bundle integrity failed'
/bin/mv "$target" "$backup" || fail 'Cannot move current app to rollback backup'
if ! /bin/mv "$stage/MantaSH.app" "$target"; then
    /bin/mv "$backup" "$target" || preserve=1
    fail 'Replacement failed; rollback attempted'
fi
if ! /usr/bin/open -n "$target"; then
    /bin/mv "$target" "$stage/MantaSH.app" && /bin/mv "$backup" "$target" || preserve=1
    /usr/bin/open -n "$target" || true
    fail 'Restart request failed; rollback attempted'
fi
/bin/rm -rf -- "$backup" || { preserve=1; fail 'Updated application launch requested; backup cleanup failed'; }
echo 'Replacement completed; new application launch request accepted (not startup confirmation)'
"#;

#[cfg(any(target_os = "windows", test))]
const WINDOWS_PREFLIGHT: &str = r#"param([Parameter(Mandatory=$true)][string]$Target)
$ErrorActionPreference = 'Stop'
$targetPath = [IO.Path]::GetFullPath($Target).TrimEnd('\')
$found = $false
foreach ($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryHive]::LocalMachine)) {
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32, [Microsoft.Win32.RegistryView]::Registry64)) {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, $view)
        try {
            $key = $base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\{A7A17D44-0650-46A4-ACD0-1B2F1DB83026}_is1')
            if ($null -ne $key) {
                try {
                    $location = [string]$key.GetValue('InstallLocation')
                    if ($location -and [string]::Equals([IO.Path]::GetFullPath($location).TrimEnd('\'), $targetPath, [StringComparison]::OrdinalIgnoreCase)) { $found = $true }
                } finally { $key.Dispose() }
            }
        } finally { $base.Dispose() }
    }
}
if (-not $found -or -not [IO.File]::Exists([IO.Path]::Combine($Target, 'unins000.exe'))) { throw 'Automatic update requires this exact directory to be a registered MantaSH Inno installation' }
"#;

#[cfg(any(target_os = "windows", test))]
const WINDOWS_PRIVACY: &str = r#"param([string]$Stage)
$ErrorActionPreference = 'Stop'
$acl = [Security.AccessControl.DirectorySecurity]::new()
$acl.SetAccessRuleProtection($true, $false)
$inherit = [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit
$user = [Security.Principal.WindowsIdentity]::GetCurrent().User
$acl.SetOwner($user)
$system = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
foreach ($sid in @($user, $system)) {
    $rule = [Security.AccessControl.FileSystemAccessRule]::new($sid, [Security.AccessControl.FileSystemRights]::FullControl, $inherit, [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow)
    $acl.AddAccessRule($rule)
}
Set-Acl -LiteralPath $Stage -AclObject $acl
"#;

#[cfg(any(target_os = "windows", test))]
const WINDOWS_HELPER: &str = r#"param([Parameter(Mandatory=$true)][string]$Stage, [Parameter(Mandatory=$true)][string]$Target, [Parameter(Mandatory=$true)][int]$ParentId, [Parameter(Mandatory=$true)][string]$Log, [Parameter(Mandatory=$true)][string]$HelperPath)
$ErrorActionPreference = 'Stop'
$preserve = $false
$status = 1
function Write-Log([string]$message) { [Console]::Error.WriteLine([DateTime]::UtcNow.ToString('o') + ' ' + $message) }
try {
    Write-Log ('Helper started; staging: ' + $Stage)
    if ((Split-Path -Leaf $Stage) -notlike 'mantash-update-*' -or -not [IO.File]::Exists((Join-Path $Stage 'owned')) -or $HelperPath -ne (Join-Path $Stage 'helper.ps1') -or ((Get-Item -LiteralPath $Stage).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Invalid private staging directory' }
    $parent = [Diagnostics.Process]::GetProcessById($ParentId)
    $expectedExe = [IO.Path]::Combine($Target, 'mantash.exe')
    if (-not [string]::Equals($parent.MainModule.FileName, $expectedExe, [StringComparison]::OrdinalIgnoreCase)) { throw 'Parent is not the installed MantaSH executable' }
    $null = $parent.Handle
    [IO.File]::WriteAllText((Join-Path $Stage 'ready'), 'ready')
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not [IO.File]::Exists((Join-Path $Stage 'commit'))) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Handoff was not committed' }
        Start-Sleep -Milliseconds 100
    }
    [IO.File]::WriteAllText((Join-Path $Stage 'accepted'), 'accepted')
    if (-not $parent.WaitForExit(300000)) { throw 'Application did not exit within five minutes; no installation performed' }
    $parent.Dispose()
    Write-Log 'Original application process exited'
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = Join-Path $Stage 'installer.exe'
    $start.UseShellExecute = $true
    $start.Arguments = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /NOCLOSEAPPLICATIONS /NORESTARTAPPLICATIONS /DIR="' + $Target + '" /LOG="' + $Log + '.inno.log"'
    $installer = [Diagnostics.Process]::Start($start)
    if (-not $installer.WaitForExit(1800000)) { $preserve = $true; throw 'Installer still running after 30 minutes; staging retained, no restart attempted' }
    if ($installer.ExitCode -ne 0) { throw ('Inno installation failed with exit code ' + $installer.ExitCode) }
    $installer.Dispose()
    $version = [IO.File]::ReadAllText((Join-Path $Stage 'version'))
    $build = [IO.File]::ReadAllText((Join-Path $Target 'build-info.json')) | ConvertFrom-Json
    $platform = [IO.File]::ReadAllText((Join-Path $Stage 'platform'))
    $targets = @{ 'windows-x86'='i686-pc-windows-msvc'; 'windows-x64'='x86_64-pc-windows-msvc'; 'windows-arm64'='aarch64-pc-windows-msvc' }
    $binaryHash = (Get-FileHash -LiteralPath $expectedExe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($build.product -ne 'MantaSH' -or $build.version -ne $version -or $build.profile -ne 'release' -or $build.target -ne $targets[$platform] -or $build.binary_sha256 -ne $binaryHash) { throw 'Installed version or integrity verification failed; no restart attempted' }
    $null = [Diagnostics.Process]::Start($expectedExe)
    Write-Log 'Installer completed and version verified; new application launch request accepted (not startup confirmation)'
    $status = 0
} catch { Write-Log ('ERROR: ' + $_.Exception.Message) }
finally {
    if (-not $preserve -and (Split-Path -Leaf $Stage) -like 'mantash-update-*' -and [IO.File]::Exists((Join-Path $Stage 'owned')) -and $HelperPath -eq (Join-Path $Stage 'helper.ps1') -and -not ((Get-Item -LiteralPath $Stage).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        try { Remove-Item -LiteralPath $Stage -Recurse -Force } catch { Write-Log ('Staging cleanup failed: ' + $Stage + ' ' + $_.Exception.Message) }
    } else { Write-Log ('Staging retained for recovery: ' + $Stage) }
    Write-Log ('Helper finished with status ' + $status)
}
exit $status
"#;

#[cfg(test)]
pub(crate) mod fixture_support {
    #[cfg(target_os = "macos")]
    pub(crate) fn bundle(
        app: &std::path::Path,
        version: &str,
        platform: &str,
    ) -> anyhow::Result<()> {
        super::validate_bundle(app, version, platform)
    }
    pub(crate) fn build(
        manifest: &std::path::Path,
        executable: &std::path::Path,
        version: &str,
        platform: &str,
    ) -> anyhow::Result<()> {
        super::validate_build(manifest, executable, version, platform)
    }
    pub(crate) const MAC_HELPER: &str = super::MAC_HELPER;
    pub(crate) const WINDOWS_HELPER: &str = super::WINDOWS_HELPER;
    pub(crate) const WINDOWS_PREFLIGHT: &str = super::WINDOWS_PREFLIGHT;
    pub(crate) fn windows_path(path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
        super::windows_path(path)
    }
    pub(crate) fn encoded_powershell(
        script: &str,
        parameters: serde_json::Value,
    ) -> anyhow::Result<String> {
        super::encoded_powershell(script, parameters)
    }
    pub(crate) fn select(
        body: &[u8],
        current: &str,
        os: &str,
        arch: &str,
    ) -> anyhow::Result<Option<super::Release>> {
        super::select_release(body, current, os, arch)
    }
    pub(crate) fn checksum(body: &[u8], name: &str) -> anyhow::Result<[u8; 32]> {
        super::parse_checksum(body, name)
    }
    pub(crate) fn package_path(download: &super::DownloadedUpdate) -> &std::path::Path {
        &download.package
    }
    pub(crate) fn download_directory(download: &super::DownloadedUpdate) -> &std::path::Path {
        download.directory.path()
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn target(
        executable: &std::path::Path,
        home: &std::path::Path,
    ) -> anyhow::Result<std::path::PathBuf> {
        super::mac_target(executable, home)
    }
}
