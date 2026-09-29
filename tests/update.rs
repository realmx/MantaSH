// Compile the same implementation with test-only loopback fixtures; the shipped
// library has no test source override or public way to construct download URLs.
#[allow(dead_code)]
#[path = "../src/update.rs"]
mod update;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use update::fixture_support as support;

const VERSION: &str = "1.10.0";
const NAME: &str = "MantaSH-1.10.0-macos-arm64.dmg";

fn asset(name: &str, size: usize) -> Value {
    json!({"name": name, "size": size, "browser_download_url": format!("https://github.com/realmx/MantaSH/releases/download/v{VERSION}/{name}")})
}

fn metadata(package: &[u8], checksum: &[u8]) -> Value {
    json!({"tag_name": format!("v{VERSION}"), "draft": false, "prerelease": false,
        "assets": [asset(NAME, package.len()), asset(&format!("{NAME}.sha256"), checksum.len())]})
}

fn checksum(package: &[u8]) -> Vec<u8> {
    format!("{:x}  {NAME}\n", Sha256::digest(package)).into_bytes()
}

fn select(value: &Value, current: &str) -> anyhow::Result<Option<update::Release>> {
    support::select(&serde_json::to_vec(value)?, current, "macos", "aarch64")
}

#[test]
fn strict_semver_and_stable_releases() {
    let mut release = metadata(b"package", &checksum(b"package"));
    assert_eq!(select(&release, "1.9.9").unwrap().unwrap().version, VERSION);
    assert!(select(&release, VERSION).unwrap().is_none());
    assert!(select(&release, "2.0.0").unwrap().is_none());
    assert!(select(&release, "1.10.0-rc.1").unwrap().is_some());
    for current in ["v1.0.0", "1.0", "01.0.0", "1.0.0 "] {
        assert!(select(&release, current).is_err(), "{current}");
    }
    for field in ["draft", "prerelease"] {
        release[field] = json!(true);
        assert!(select(&release, "1.0.0").unwrap().is_none());
        release[field] = json!(false);
    }
    for tag in ["v1.10.0-rc.1", "v1.10.0+build"] {
        release["tag_name"] = json!(tag);
        assert!(select(&release, "1.0.0").unwrap().is_none());
    }
    for tag in ["1.10.0", "v01.10.0", "v1.10", "v1.10.0/../../evil"] {
        release["tag_name"] = json!(tag);
        assert!(select(&release, "1.0.0").is_err());
    }
}

#[test]
fn selects_exact_five_packaged_architectures_and_requires_checksum() {
    for (os, arch, platform, suffix) in [
        ("macos", "aarch64", "macos-arm64", ".dmg"),
        ("macos", "x86_64", "macos-x64", ".dmg"),
        ("windows", "x86", "windows-x86", "-setup.exe"),
        ("windows", "x86_64", "windows-x64", "-setup.exe"),
        ("windows", "aarch64", "windows-arm64", "-setup.exe"),
    ] {
        let name = format!("MantaSH-{VERSION}-{platform}{suffix}");
        let mut value = metadata(b"package", &checksum(b"package"));
        value["assets"] = json!([asset(&name, 100), asset(&format!("{name}.sha256"), 120)]);
        let body = serde_json::to_vec(&value).unwrap();
        assert!(support::select(&body, "1.0.0", os, arch).unwrap().is_some());
        assert!(
            support::select(&body, "1.0.0", "linux", arch)
                .unwrap()
                .is_none()
        );
        value["assets"] = json!([asset(&name, 100)]);
        assert!(
            support::select(&serde_json::to_vec(&value).unwrap(), "1.0.0", os, arch)
                .unwrap()
                .is_none()
        );
    }
    let value = metadata(b"package", &checksum(b"package"));
    assert!(
        support::select(
            &serde_json::to_vec(&value).unwrap(),
            "1.0.0",
            "macos",
            "x86_64"
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn rejects_untrusted_urls_duplicate_assets_and_unbounded_sizes() {
    for url in [
        format!("http://github.com/realmx/MantaSH/releases/download/v{VERSION}/{NAME}"),
        format!("https://github.com/attacker/MantaSH/releases/download/v{VERSION}/{NAME}"),
        format!("https://github.com/realmx/other/releases/download/v{VERSION}/{NAME}"),
        format!("https://github.com/realmx/MantaSH/releases/download/v9.0.0/{NAME}"),
        format!("https://github.com/realmx/MantaSH/releases/download/v{VERSION}/{NAME}?x=1"),
        format!("https://github.com.evil.test/realmx/MantaSH/releases/download/v{VERSION}/{NAME}"),
        format!("https://user@github.com/realmx/MantaSH/releases/download/v{VERSION}/{NAME}"),
        format!("https://github.com/realmx/MantaSH/releases/download/v{VERSION}/%2e%2e/evil"),
    ] {
        let mut value = metadata(b"package", &checksum(b"package"));
        value["assets"][0]["browser_download_url"] = json!(url);
        assert!(select(&value, "1.0.0").is_err());
    }
    for size in [0_u64, 1024 * 1024 * 1024 + 1] {
        let mut value = metadata(b"package", &checksum(b"package"));
        value["assets"][0]["size"] = json!(size);
        assert!(select(&value, "1.0.0").is_err());
    }
    let mut value = metadata(b"package", &checksum(b"package"));
    value["assets"].as_array_mut().unwrap().push(asset(NAME, 7));
    assert!(select(&value, "1.0.0").is_err());
}

#[test]
fn checksum_is_one_exact_lowercase_line_with_expected_filename() {
    let valid = checksum(b"package");
    assert_eq!(
        support::checksum(&valid, NAME).unwrap(),
        <[u8; 32]>::from(Sha256::digest(b"package"))
    );
    for invalid in [
        valid[..valid.len() - 1].to_vec(),
        String::from_utf8(valid.clone())
            .unwrap()
            .to_uppercase()
            .into_bytes(),
        format!("{}  ../{NAME}\n", "0".repeat(64)).into_bytes(),
        format!("{} *{NAME}\n", "0".repeat(64)).into_bytes(),
        [valid.clone(), b"extra\n".to_vec()].concat(),
        b"short".to_vec(),
    ] {
        assert!(support::checksum(&invalid, NAME).is_err());
    }
}

#[derive(Clone)]
struct Reply {
    status: u16,
    body: Vec<u8>,
    declared_length: Option<usize>,
    slow: bool,
}
impl Reply {
    fn ok(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            body,
            declared_length: None,
            slow: false,
        }
    }
}
struct Server {
    updater: update::Updater,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server(routes: HashMap<String, Reply>) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let routes = Arc::new(routes);
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let Ok(bytes) = socket.read(&mut buffer).await else {
                        return;
                    };
                    if bytes == 0 || request.len() > 8192 {
                        return;
                    }
                    request.extend_from_slice(&buffer[..bytes]);
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("/");
                let Some(reply) = routes.get(path) else {
                    return;
                };
                let header = format!(
                    "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.status,
                    reply.declared_length.unwrap_or(reply.body.len())
                );
                if socket.write_all(header.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in reply.body.chunks(1024) {
                    if socket.write_all(chunk).await.is_err() {
                        return;
                    }
                    if reply.slow {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            });
        }
    });
    Server {
        updater: update::Updater::fixture(&base).unwrap(),
        task,
    }
}

async fn package_server(package: Vec<u8>, checksum: Vec<u8>, package_reply: Reply) -> Server {
    server(HashMap::from([
        (
            "/latest".to_owned(),
            Reply::ok(serde_json::to_vec(&metadata(&package, &checksum)).unwrap()),
        ),
        (format!("/{NAME}.sha256"), Reply::ok(checksum)),
        (format!("/{NAME}"), package_reply),
    ]))
    .await
}

async fn release(server: &Server) -> update::Release {
    server
        .updater
        .fixture_check("1.0.0", "macos", "aarch64")
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn streams_progress_verifies_private_file_and_drop_cleans_up() {
    let package = vec![42; 32 * 1024];
    let server = package_server(
        package.clone(),
        checksum(&package),
        Reply::ok(package.clone()),
    )
    .await;
    let progress = Arc::new(Mutex::new(Vec::new()));
    let recorded = progress.clone();
    let download = server
        .updater
        .download(
            release(&server).await,
            CancellationToken::new(),
            move |done, total| recorded.lock().unwrap().push((done, total)),
        )
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(support::package_path(&download))
            .await
            .unwrap(),
        package
    );
    let directory = support::download_directory(&download).to_owned();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let progress = progress.lock().unwrap();
    assert_eq!(progress.first().unwrap(), &(0, package.len() as u64));
    assert_eq!(
        progress.last().unwrap(),
        &(package.len() as u64, package.len() as u64)
    );
    assert!(progress.windows(2).all(|pair| pair[0].0 <= pair[1].0));
    drop(download);
    assert!(!directory.exists());
}

#[tokio::test]
async fn rejects_checksum_http_length_failures_and_cancellation() {
    let package = b"real package".to_vec();
    let server = package_server(
        package.clone(),
        checksum(b"fake package"),
        Reply::ok(package.clone()),
    )
    .await;
    let error = server
        .updater
        .download(release(&server).await, CancellationToken::new(), |_, _| {})
        .await
        .unwrap_err();
    assert!(error.to_string().contains("SHA-256"));
    assert_eq!(
        std::fs::read_dir(server.updater.fixture_staging())
            .unwrap()
            .count(),
        0
    );
    for reply in [
        Reply {
            status: 500,
            ..Reply::ok(package.clone())
        },
        Reply {
            declared_length: Some(package.len() + 1),
            ..Reply::ok(package.clone())
        },
        Reply {
            declared_length: Some(package.len() - 1),
            ..Reply::ok(package.clone())
        },
    ] {
        let server = package_server(package.clone(), checksum(&package), reply).await;
        assert!(
            server
                .updater
                .download(release(&server).await, CancellationToken::new(), |_, _| {})
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read_dir(server.updater.fixture_staging())
                .unwrap()
                .count(),
            0
        );
    }
    let package = vec![9; 64 * 1024];
    let server = package_server(
        package.clone(),
        checksum(&package),
        Reply {
            slow: true,
            ..Reply::ok(package)
        },
    )
    .await;
    let token = CancellationToken::new();
    let cancel = token.clone();
    let error = server
        .updater
        .download(release(&server).await, token, move |done, _| {
            if done > 0 {
                cancel.cancel();
            }
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert_eq!(
        std::fs::read_dir(server.updater.fixture_staging())
            .unwrap()
            .count(),
        0
    );
    let token = CancellationToken::new();
    token.cancel();
    assert!(
        server
            .updater
            .download(release(&server).await, token, |_, _| {})
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_dir(server.updater.fixture_staging())
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn api_http_failure_and_body_cap_are_errors() {
    for reply in [
        Reply {
            status: 403,
            ..Reply::ok(b"rate limited".to_vec())
        },
        Reply::ok(vec![b' '; 1024 * 1024 + 1]),
    ] {
        let server = server(HashMap::from([("/latest".to_owned(), reply)])).await;
        assert!(
            server
                .updater
                .fixture_check("1.0.0", "macos", "aarch64")
                .await
                .is_err()
        );
    }
    let server = server(HashMap::from([(
        "/latest".to_owned(),
        Reply {
            status: 404,
            ..Reply::ok(Vec::new())
        },
    )]))
    .await;
    assert!(
        server
            .updater
            .fixture_check("1.0.0", "macos", "aarch64")
            .await
            .unwrap()
            .is_none()
    );
    assert!(update::Updater::fixture("http://example.com/").is_err());
}

#[tokio::test]
async fn preparation_rejects_development_binary_and_cleans_download() {
    let package = b"not actually a DMG".to_vec();
    let server = package_server(package.clone(), checksum(&package), Reply::ok(package)).await;
    let download = server
        .updater
        .download(release(&server).await, CancellationToken::new(), |_, _| {})
        .await
        .unwrap();
    let directory = support::download_directory(&download).to_owned();
    assert!(update::prepare_install(download).await.is_err());
    assert!(!directory.exists());
    fn assert_send<T: Send>() {}
    assert_send::<update::DownloadedUpdate>();
    assert_send::<update::PreparedUpdate>();
    assert_send::<mantash::update::DownloadedUpdate>();
    assert_send::<mantash::update::PreparedUpdate>();
}

#[cfg(target_os = "macos")]
#[test]
fn installed_path_validation_rejects_development_and_translocation() {
    for path in [
        "/tmp/mantash",
        "/src/target/release/mantash",
        "/Volumes/MantaSH/MantaSH.app/Contents/MacOS/mantash",
        "/Applications/Other.app/Contents/MacOS/mantash",
    ] {
        assert!(support::target(Path::new(path), Path::new("/Users/example")).is_err());
    }
    let root = tempfile::tempdir().unwrap().path().canonicalize().unwrap();
    // A sandbox home allows the same ~/Applications checks without touching the real install.
    let home = tempfile::tempdir_in(root.parent().unwrap()).unwrap();
    let executable = home
        .path()
        .join("Applications/MantaSH.app/Contents/MacOS/mantash");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, b"test").unwrap();
    assert_eq!(
        support::target(&executable, home.path()).unwrap(),
        home.path().join("Applications/MantaSH.app")
    );
}

#[test]
fn windows_helper_has_bounded_real_exit_wait_exact_directory_and_result_check() {
    let helper = support::WINDOWS_HELPER;
    assert!(helper.contains("$parent.Handle"));
    assert!(helper.contains("$parent.WaitForExit(300000)"));
    assert!(helper.contains("$installer.WaitForExit(1800000)"));
    assert!(helper.contains("$installer.ExitCode -ne 0"));
    assert!(helper.contains("/NOCLOSEAPPLICATIONS"));
    assert!(helper.contains("/DIR=\"' + $Target"));
    assert!(!helper.contains("Invoke-Expression"));
    assert!(!helper.contains("cmd.exe"));
    assert!(support::WINDOWS_PREFLIGHT.contains("{A7A17D44-0650-46A4-ACD0-1B2F1DB83026}_is1"));
    assert!(support::WINDOWS_PREFLIGHT.contains("Registry64"));
    assert!(support::WINDOWS_PREFLIGHT.contains("Registry32"));
}

#[cfg(unix)]
fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(Instant::now() < deadline, "waiting for {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn sandbox_helper(
    restart_fails: bool,
) -> (tempfile::TempDir, PathBuf, PathBuf, std::process::Child) {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let stage = root.path().join(".mantash-update-space ' quote $ literal");
    let target = root.path().join("MantaSH ' $(touch INJECTION).app");
    std::fs::create_dir(&stage).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::write(stage.join("owned"), b"test").unwrap();
    std::fs::write(target.join("version"), b"old").unwrap();
    std::fs::create_dir(stage.join("MantaSH.app")).unwrap();
    std::fs::write(stage.join("MantaSH.app/version"), b"new").unwrap();
    // Replace system boundary commands only in this copied sandbox script. No real
    // codesign/open or installed app is ever invoked or changed by helper tests.
    let script = support::MAC_HELPER
        .replace("/bin/ps", "mock_ps")
        .replace("/bin/kill", "/usr/bin/false")
        .replace("/usr/bin/codesign", "/usr/bin/true")
        .replace(
            "/usr/bin/open",
            if restart_fails {
                "/usr/bin/false"
            } else {
                "/usr/bin/true"
            },
        );
    let script = script.replacen("set -u", "set -u\nmock_ps() { if [[ -f \"$stage/parent-alive\" ]]; then echo fixed-parent-identity; else return 1; fi; }", 1);
    let helper = stage.join("helper.sh");
    std::fs::write(&helper, script).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(stage.join("parent-alive"), b"alive").unwrap();
    let log = root.path().join("helper ' log.txt");
    let child = std::process::Command::new("/bin/bash")
        .arg(&helper)
        .arg(&stage)
        .arg(&target)
        .arg("1234")
        .arg(&log)
        .spawn()
        .unwrap();
    (root, stage, target, child)
}

#[cfg(unix)]
#[test]
fn mac_helper_waits_for_exit_handles_quoted_paths_and_cleans_sandbox() {
    for restart_fails in [false, true] {
        let (root, stage, target, mut child) = sandbox_helper(restart_fails);
        wait_file(&stage.join("ready"));
        std::fs::write(stage.join("commit"), b"commit").unwrap();
        std::thread::sleep(Duration::from_millis(150));
        assert!(child.try_wait().unwrap().is_none());
        assert_eq!(std::fs::read(target.join("version")).unwrap(), b"old");
        std::fs::remove_file(stage.join("parent-alive")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.success(), !restart_fails);
        assert_eq!(
            std::fs::read(target.join("version")).unwrap(),
            if restart_fails { b"old" } else { b"new" }
        );
        assert!(!stage.exists());
        assert!(!root.path().join("INJECTION").exists());
        let log = std::fs::read_to_string(root.path().join("helper ' log.txt")).unwrap();
        assert!(log.contains("Original application process exited"));
        assert!(log.contains(if restart_fails {
            "rollback attempted"
        } else {
            "not startup confirmation"
        }));
    }
}

#[cfg(unix)]
#[test]
fn mac_helper_script_is_valid_bash_and_does_not_bypass_gatekeeper() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("helper.sh");
    std::fs::write(&path, support::MAC_HELPER).unwrap();
    assert!(
        std::process::Command::new("/bin/bash")
            .arg("-n")
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
    assert!(!support::MAC_HELPER.contains("xattr"));
    assert!(!support::MAC_HELPER.contains("spctl"));
}

#[test]
fn windows_paths_and_encoded_parameters_preserve_quotes_without_evaluating_them() {
    use base64::Engine;
    let path = r"\\?\C:\Users\A ' quote $name; literal\MantaSH";
    assert_eq!(
        support::windows_path(Path::new(path)).unwrap(),
        PathBuf::from(&path[4..])
    );
    for path in [
        r"\\server\share\MantaSH",
        r"\\?\UNC\server\share\MantaSH",
        "relative",
        "C:relative",
    ] {
        assert!(support::windows_path(Path::new(path)).is_err());
    }
    let parameters = json!({"Stage": path, "Target": "C:\\space ' ; $(danger) \\\" literal\\MantaSH", "ParentId": 42});
    let encoded = support::encoded_powershell(
        "param($Stage,$Target,$ParentId)\nWrite-Output $Target",
        parameters.clone(),
    )
    .unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let units: Vec<u16> = decoded
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let source = String::from_utf16(&units).unwrap();
    assert!(!source.contains("$(danger)"));
    assert!(!source.contains("ExecutionPolicy"));
    let data = source
        .split("FromBase64String('")
        .nth(1)
        .unwrap()
        .split('\'')
        .next()
        .unwrap();
    let actual: Value = serde_json::from_slice(
        &base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, parameters);
    assert!(source.contains("@arguments"));
}

#[cfg(target_os = "macos")]
#[test]
fn mac_signed_bundle_accepts_pre_sign_manifest_and_rejects_tampering() {
    use std::process::Command;
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("MantaSH 空格 ' quote.app");
    let contents = app.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    std::fs::create_dir_all(contents.join("Resources")).unwrap();
    let source = root.path().join("fixture.c");
    let executable = contents.join("MacOS/mantash");
    std::fs::write(&source, "int main(void) { return 0; }\n").unwrap();
    assert!(
        Command::new("/usr/bin/cc")
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .status()
            .unwrap()
            .success()
    );
    let info = json!({"CFBundleIdentifier": "app.mantash.MantaSH", "CFBundleExecutable": "mantash",
        "CFBundlePackageType": "APPL", "CFBundleShortVersionString": VERSION, "CFBundleVersion": VERSION});
    let plist = contents.join("Info.plist");
    std::fs::write(&plist, serde_json::to_vec(&info).unwrap()).unwrap();
    assert!(
        Command::new("/usr/bin/plutil")
            .args(["-convert", "xml1"])
            .arg(&plist)
            .status()
            .unwrap()
            .success()
    );
    let (target, platform) = if cfg!(target_arch = "aarch64") {
        ("aarch64-apple-darwin", "macos-arm64")
    } else {
        ("x86_64-apple-darwin", "macos-x64")
    };
    let unsigned_hash = format!("{:x}", Sha256::digest(std::fs::read(&executable).unwrap()));
    // Match package_release.py: the manifest is written before bundle signing.
    let manifest = json!({"product": "MantaSH", "version": VERSION, "target": target,
        "profile": "release", "binary_sha256": unsigned_hash});
    std::fs::write(
        contents.join("Resources/build-info.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(
        Command::new("/usr/bin/codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(&app)
            .status()
            .unwrap()
            .success()
    );
    assert_ne!(
        manifest["binary_sha256"],
        format!("{:x}", Sha256::digest(std::fs::read(&executable).unwrap()))
    );
    support::bundle(&app, VERSION, platform).unwrap();
    assert!(support::bundle(&app, "9.9.9", platform).is_err());
    std::fs::write(&executable, b"tampered executable").unwrap();
    assert!(support::bundle(&app, VERSION, platform).is_err());
}

#[test]
fn windows_build_manifest_still_requires_exact_executable_hash() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("mantash.exe");
    let manifest = root.path().join("build-info.json");
    std::fs::write(&executable, b"isolated executable fixture").unwrap();
    std::fs::write(
        &manifest,
        serde_json::to_vec(&json!({"product": "MantaSH", "version": VERSION,
        "target": "x86_64-pc-windows-msvc", "profile": "release",
        "binary_sha256": format!("{:x}", Sha256::digest(b"isolated executable fixture"))}))
        .unwrap(),
    )
    .unwrap();
    support::build(&manifest, &executable, VERSION, "windows-x64").unwrap();
    std::fs::write(&executable, b"tampered executable").unwrap();
    assert!(support::build(&manifest, &executable, VERSION, "windows-x64").is_err());
}
