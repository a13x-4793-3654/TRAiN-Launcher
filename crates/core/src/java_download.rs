//! Install verified Eclipse Temurin JREs without modifying system Java settings.
//!
//! Only a fully extracted, verified and probed runtime is moved into `installed`.
//! Download and extraction failures leave no discoverable partial installation.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::java::{managed_runtime_root, probe_runtime, JavaRuntime};
use crate::CoreError;

const MAX_METADATA_SIZE: usize = 4 * 1024 * 1024;
const MAX_PACKAGE_SIZE: u64 = 1024 * 1024 * 1024;
const MAX_EXTRACTED_SIZE: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArchiveFormat {
    Zip,
    TarGz,
}

#[derive(Clone, Copy, Debug)]
struct Platform {
    architecture: &'static str,
    os: &'static str,
    format: ArchiveFormat,
}

impl Platform {
    fn current() -> Result<Self, String> {
        Self::for_target(std::env::consts::ARCH, std::env::consts::OS)
    }

    fn for_target(architecture: &str, os: &str) -> Result<Self, String> {
        let architecture = match architecture {
            "x86_64" => "x64",
            "x86" => "x86",
            "aarch64" => "aarch64",
            other => {
                return Err(format!(
                    "このCPU ({other}) 向けのJava自動ダウンロードには対応していません"
                ))
            }
        };
        let (os, format) = match os {
            "windows" => ("windows", ArchiveFormat::Zip),
            "linux" => ("linux", ArchiveFormat::TarGz),
            "macos" => ("mac", ArchiveFormat::TarGz),
            other => {
                return Err(format!(
                    "このOS ({other}) 向けのJava自動ダウンロードには対応していません"
                ))
            }
        };
        Ok(Self {
            architecture,
            os,
            format,
        })
    }

    fn executable_name(self) -> &'static str {
        if self.os == "windows" {
            "java.exe"
        } else {
            "java"
        }
    }
}

#[derive(Debug, Deserialize)]
struct Asset {
    binary: Binary,
    version: Version,
}

#[derive(Debug, Deserialize)]
struct Binary {
    architecture: String,
    os: String,
    image_type: String,
    package: Package,
}

#[derive(Debug, Deserialize)]
struct Version {
    major: u32,
}

#[derive(Debug, Deserialize)]
struct Package {
    checksum: String,
    link: String,
    size: u64,
    name: String,
}

#[derive(Debug)]
struct VerifiedPackage {
    url: reqwest::Url,
    checksum: [u8; 32],
    size: u64,
    format: ArchiveFormat,
}

pub(crate) async fn install_runtime(
    required_major: u32,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<JavaRuntime, CoreError> {
    install_runtime_at(required_major, on_progress, &managed_runtime_root()).await
}

async fn install_runtime_at(
    required_major: u32,
    on_progress: &(dyn Fn(&str) + Send + Sync),
    root: &Path,
) -> Result<JavaRuntime, CoreError> {
    install_inner(required_major, on_progress, root)
        .await
        .map_err(|reason| {
            CoreError::JavaRuntime(format!(
                "必要なJava {required_major} の自動インストールに失敗しました: {reason}。\n\
                 通信環境と保存先の空き容量を確認して再度起動してください。\
                 未公開の環境では対応するJava {required_major} を手動でインストールし、設定で指定してください。"
            ))
        })
}

async fn install_inner(
    required_major: u32,
    on_progress: &(dyn Fn(&str) + Send + Sync),
    root: &Path,
) -> Result<JavaRuntime, String> {
    if required_major == 0 {
        return Err("必要なJavaのメジャーバージョンが不正です".into());
    }
    let platform = Platform::current()?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .timeout(Duration::from_secs(15 * 60))
        .user_agent(concat!("TRAiN-Launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| format!("ダウンロード用の通信設定を作成できません: {err}"))?;

    on_progress(&format!("Java {required_major} の配布情報を確認中..."));
    let package = fetch_package(&client, required_major, platform).await?;

    tokio::fs::create_dir_all(root)
        .await
        .map_err(|err| format!("Javaの保存先を作成できません: {err}"))?;
    let root = tokio::fs::canonicalize(root)
        .await
        .map_err(|err| format!("Javaの保存先を確認できません: {err}"))?;
    let downloads = root.join("downloads");
    let installed = root.join("installed");
    for directory in [&downloads, &installed] {
        tokio::fs::create_dir_all(directory)
            .await
            .map_err(|err| format!("Javaの保存用ディレクトリを作成できません: {err}"))?;
        let resolved = tokio::fs::canonicalize(directory)
            .await
            .map_err(|err| format!("Javaの保存用ディレクトリを確認できません: {err}"))?;
        if !resolved.starts_with(&root) {
            return Err("Javaの保存先が管理ディレクトリの外部を参照しています".into());
        }
    }
    let staging = tempfile::Builder::new()
        .prefix("java-")
        .rand_bytes(16)
        .tempdir_in(&downloads)
        .map_err(|err| format!("Javaの一時保存先を作成できません: {err}"))?;
    let archive = staging.path().join("package");
    on_progress(&format!("Java {required_major} をダウンロード中..."));
    download_package(&client, &package, &archive, required_major, on_progress).await?;

    on_progress(&format!(
        "Java {required_major} を検証済みファイルから展開中..."
    ));
    // The worker owns the RAII guard so cancellation cannot remove its directory
    // while extraction is still running, or leave it behind after completion.
    let (staging, executable) = tokio::task::spawn_blocking(move || {
        let payload = staging.path().join("runtime");
        extract_archive(&archive, &payload, package.format)?;
        let executable = find_java(&payload, platform.executable_name())?;
        Ok::<_, String>((staging, executable))
    })
    .await
    .map_err(|err| format!("Javaの展開処理を完了できません: {err}"))??;

    on_progress(&format!("Java {required_major} の動作を確認中..."));
    let runtime = probe_runtime(executable)
        .await
        .map_err(|err| format!("ダウンロードしたJavaを実行できません: {err}"))?;
    let runtime = publish_runtime(&staging, runtime, required_major, &installed).await?;
    on_progress(&format!(
        "Java {required_major} のインストールが完了しました"
    ));
    Ok(runtime)
}

async fn publish_runtime(
    staging: &tempfile::TempDir,
    mut runtime: JavaRuntime,
    required_major: u32,
    installed: &Path,
) -> Result<JavaRuntime, String> {
    validate_runtime(&runtime, required_major)?;
    let payload = staging.path().join("runtime");
    let canonical_payload = fs::canonicalize(&payload)
        .map_err(|err| format!("展開したJavaの場所を確認できません: {err}"))?;
    let relative_executable = runtime
        .executable
        .strip_prefix(&canonical_payload)
        .map_err(|_| "Java実行ファイルが展開先の外部を参照しています".to_string())?;
    let unique_name = staging
        .path()
        .file_name()
        .ok_or_else(|| "Javaの一時ディレクトリ名を取得できません".to_string())?
        .to_string_lossy();
    let destination = installed.join(format!("java-{required_major}-{unique_name}"));
    let final_executable = destination.join(relative_executable);
    if destination
        .try_exists()
        .map_err(|err| format!("Javaのインストール先を確認できません: {err}"))?
    {
        return Err("Javaのインストール先が既に存在します。既存の環境は変更しません".into());
    }
    tokio::fs::rename(&payload, &destination)
        .await
        .map_err(|err| format!("検証済みJavaをインストール先に移動できません: {err}"))?;
    runtime.executable = final_executable;
    Ok(runtime)
}

async fn fetch_package(
    client: &reqwest::Client,
    required_major: u32,
    platform: Platform,
) -> Result<VerifiedPackage, String> {
    let mut response = client
        .get(format!(
            "https://api.adoptium.net/v3/assets/latest/{required_major}/hotspot"
        ))
        .query(&[
            ("architecture", platform.architecture),
            ("image_type", "jre"),
            ("os", platform.os),
            ("vendor", "eclipse"),
        ])
        .timeout(Duration::from_secs(45))
        .send()
        .await
        .map_err(|err| format!("Adoptiumの配布情報を取得できません: {}", err.without_url()))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND
        || response.status() == reqwest::StatusCode::NO_CONTENT
    {
        return Err(unavailable_message(required_major, platform));
    }
    response = response
        .error_for_status()
        .map_err(|err| format!("Adoptiumの配布情報を取得できません: {}", err.without_url()))?;
    let mut metadata = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("Javaの配布情報を読み取れません: {}", err.without_url()))?
    {
        if metadata.len().saturating_add(chunk.len()) > MAX_METADATA_SIZE {
            return Err("Javaの配布情報が許容サイズを超えています".into());
        }
        metadata.extend_from_slice(&chunk);
    }
    let assets: Vec<Asset> = serde_json::from_slice(&metadata)
        .map_err(|err| format!("Javaの配布情報が不正です: {err}"))?;
    select_package(assets, required_major, platform)
}

fn unavailable_message(required_major: u32, platform: Platform) -> String {
    format!(
        "AdoptiumでJava {required_major} JRE ({} / {}) が公開されていません",
        platform.os, platform.architecture
    )
}

fn select_package(
    assets: Vec<Asset>,
    required_major: u32,
    platform: Platform,
) -> Result<VerifiedPackage, String> {
    let asset = assets
        .into_iter()
        .find(|asset| {
            asset.version.major == required_major
                && asset.binary.architecture == platform.architecture
                && asset.binary.os == platform.os
                && asset.binary.image_type == "jre"
        })
        .ok_or_else(|| unavailable_message(required_major, platform))?;
    validate_package(asset.binary.package, platform.format)
}

fn validate_package(package: Package, format: ArchiveFormat) -> Result<VerifiedPackage, String> {
    let url = reqwest::Url::parse(&package.link)
        .map_err(|_| "JavaのダウンロードURLが不正です".to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("JavaのダウンロードURLが安全なHTTPS URLではありません".into());
    }
    let name = safe_relative_path(&package.name)?;
    let extension = match format {
        ArchiveFormat::Zip => ".zip",
        ArchiveFormat::TarGz => ".tar.gz",
    };
    if name.components().count() != 1 || !package.name.to_ascii_lowercase().ends_with(extension) {
        return Err(format!(
            "Javaの配布ファイルは {extension} 形式ではありません"
        ));
    }
    if package.size == 0 || package.size > MAX_PACKAGE_SIZE {
        return Err("Javaの配布ファイルのサイズが不正です".into());
    }
    if package.checksum.len() != 64 || !package.checksum.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Javaの配布情報に有効なSHA256チェックサムがありません".into());
    }
    let mut checksum = [0; 32];
    for (index, byte) in checksum.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&package.checksum[index * 2..index * 2 + 2], 16)
            .map_err(|_| "JavaのSHA256チェックサムが不正です".to_string())?;
    }
    Ok(VerifiedPackage {
        url,
        checksum,
        size: package.size,
        format,
    })
}

struct IntegrityCheck {
    hasher: Sha256,
    size: u64,
    expected_size: u64,
    expected_checksum: [u8; 32],
}

impl IntegrityCheck {
    fn new(package: &VerifiedPackage) -> Self {
        Self {
            hasher: Sha256::new(),
            size: 0,
            expected_size: package.size,
            expected_checksum: package.checksum,
        }
    }

    fn update(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.size = self
            .size
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| "Javaのダウンロードサイズが不正です".to_string())?;
        if self.size > self.expected_size {
            return Err("Javaのダウンロードサイズが配布情報と一致しません (サイズ超過)".into());
        }
        self.hasher.update(bytes);
        Ok(())
    }

    fn finish(self) -> Result<(), String> {
        if self.size != self.expected_size {
            return Err(format!(
                "Javaのダウンロードサイズが配布情報と一致しません (期待値 {}、実際 {})",
                self.expected_size, self.size
            ));
        }
        if self.hasher.finalize()[..] != self.expected_checksum {
            return Err(
                "JavaのSHA256チェックサムが一致しません。安全のため展開と実行を中止しました".into(),
            );
        }
        Ok(())
    }
}

async fn download_package(
    client: &reqwest::Client,
    package: &VerifiedPackage,
    destination: &Path,
    required_major: u32,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    let mut response = client
        .get(package.url.clone())
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|err| format!("Javaをダウンロードできません: {}", err.without_url()))?;
    if response
        .content_length()
        .is_some_and(|length| length != package.size)
    {
        return Err("JavaのHTTP応答サイズが配布情報と一致しません".into());
    }
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .await
        .map_err(|err| format!("Javaのダウンロードファイルを作成できません: {err}"))?;
    let mut integrity = IntegrityCheck::new(package);
    let mut reported_percent = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("Javaのダウンロードが中断されました: {}", err.without_url()))?
    {
        integrity.update(&chunk)?;
        output
            .write_all(&chunk)
            .await
            .map_err(|err| format!("Javaのダウンロードファイルを保存できません: {err}"))?;
        let percent = integrity.size * 100 / package.size;
        if percent >= reported_percent + 5 {
            reported_percent = percent;
            on_progress(&format!(
                "Java {required_major} をダウンロード中... ({percent}%)"
            ));
        }
    }
    integrity.finish()?;
    output
        .sync_all()
        .await
        .map_err(|err| format!("Javaのダウンロードファイルを確定できません: {err}"))?;
    Ok(())
}

fn validate_runtime(runtime: &JavaRuntime, required_major: u32) -> Result<(), String> {
    if runtime.major_version != required_major || !runtime.supports(required_major) {
        return Err(format!(
            "Javaの実行結果が必要条件と一致しません (必要: Java {required_major} / {}、実際: Java {} / {})",
            std::env::consts::ARCH,
            runtime.major_version,
            runtime.architecture
        ));
    }
    Ok(())
}

fn safe_component(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    !component.ends_with([' ', '.'])
        && !component.contains(['\\', ':', '\0', '<', '>', '"', '|', '?', '*'])
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

fn safe_relative_path(name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.starts_with(['/', '\\']) {
        return Err("Javaのアーカイブに絶対パスまたは空のパスが含まれています".into());
    }
    let mut path = PathBuf::new();
    for component in name.split('/') {
        match component {
            "" | "." => continue,
            ".." => return Err("Javaのアーカイブに親ディレクトリへのパスが含まれています".into()),
            value if safe_component(value) => path.push(value),
            _ => return Err("Javaのアーカイブに安全でないファイル名が含まれています".into()),
        }
    }
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Javaのアーカイブ内のパスが不正です".into());
    }
    Ok(path)
}

fn ensure_directories(root: &Path, relative: &Path) -> Result<(), String> {
    let mut directory = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err("Javaの展開先のパスが不正です".into());
        }
        directory.push(component);
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err("Javaの展開先にディレクトリ以外のファイルがあります".into()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&directory)
                    .map_err(|err| format!("Javaの展開先を作成できません: {err}"))?;
            }
            Err(err) => return Err(format!("Javaの展開先を確認できません: {err}")),
        }
    }
    Ok(())
}

fn extract_file(
    reader: &mut impl Read,
    root: &Path,
    relative: &Path,
    size: u64,
    mode: Option<u32>,
) -> Result<(), String> {
    ensure_directories(root, relative.parent().unwrap_or_else(|| Path::new("")))?;
    let path = root.join(relative);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|err| format!("Javaの展開ファイルを作成できません: {err}"))?;
    let copied = io::copy(&mut reader.take(size.saturating_add(1)), &mut output)
        .map_err(|err| format!("Javaのファイルを展開できません: {err}"))?;
    if copied != size {
        return Err("Javaの展開ファイルのサイズがアーカイブの情報と一致しません".into());
    }
    output
        .flush()
        .map_err(|err| format!("Javaの展開ファイルを保存できません: {err}"))?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(mode & 0o777))
            .map_err(|err| format!("Javaの実行権限を設定できません: {err}"))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

fn add_extracted_size(total: &mut u64, size: u64) -> Result<(), String> {
    *total = total
        .checked_add(size)
        .filter(|total| *total <= MAX_EXTRACTED_SIZE)
        .ok_or_else(|| "Javaのアーカイブの展開サイズが許容値を超えています".to_string())?;
    Ok(())
}

fn extract_archive(archive: &Path, root: &Path, format: ArchiveFormat) -> Result<(), String> {
    fs::create_dir(root).map_err(|err| format!("Javaの展開先を作成できません: {err}"))?;
    let file = File::open(archive)
        .map_err(|err| format!("検証済みのJavaアーカイブを開けません: {err}"))?;
    match format {
        ArchiveFormat::Zip => extract_zip(file, root),
        ArchiveFormat::TarGz => extract_tar(GzDecoder::new(file), root),
    }
}

fn extract_zip(file: File, root: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| format!("JavaのZIPアーカイブが不正です: {err}"))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err("Javaのアーカイブのファイル数が許容値を超えています".into());
    }
    let mut total = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("JavaのZIPエントリを読み取れません: {err}"))?;
        let relative = safe_relative_path(entry.name())?;
        let mode = entry.unix_mode();
        if mode.is_some_and(|mode| {
            let kind = mode & 0o170000;
            kind != 0 && kind != 0o100000 && kind != 0o040000
        }) {
            return Err("JavaのZIPにシンボリックリンクまたは特殊ファイルが含まれています".into());
        }
        if entry.is_dir() {
            ensure_directories(root, &relative)?;
        } else {
            let size = entry.size();
            add_extracted_size(&mut total, size)?;
            extract_file(&mut entry, root, &relative, size, mode)?;
        }
    }
    Ok(())
}

struct ArchiveLink {
    path: PathBuf,
    target: PathBuf,
    symbolic: bool,
}

fn link_target(path: &Path, target: &str, symbolic: bool) -> Result<PathBuf, String> {
    if !symbolic {
        return safe_relative_path(target);
    }
    if target.is_empty() || target.starts_with(['/', '\\']) {
        return Err("Javaのアーカイブに絶対パスのリンクが含まれています".into());
    }
    let mut resolved = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    for component in target.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if !resolved.pop() {
                    return Err("Javaのアーカイブのリンクが展開先の外部を参照しています".into());
                }
            }
            value if safe_component(value) => resolved.push(value),
            _ => return Err("Javaのアーカイブのリンク先が不正です".into()),
        }
    }
    Ok(resolved)
}

fn extract_tar(reader: impl Read, root: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|err| format!("JavaのTARアーカイブが不正です: {err}"))?;
    let mut links = Vec::new();
    let mut total = 0;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_ARCHIVE_ENTRIES {
            return Err("Javaのアーカイブのファイル数が許容値を超えています".into());
        }
        let mut entry = entry.map_err(|err| format!("JavaのTARエントリが不正です: {err}"))?;
        let path = entry
            .path()
            .map_err(|err| format!("JavaのTAR内のパスが不正です: {err}"))?
            .into_owned();
        let name = path
            .to_str()
            .ok_or_else(|| "JavaのTAR内のファイル名を読み取れません".to_string())?;
        let kind = entry.header().entry_type();
        if kind.is_dir() && matches!(name, "." | "./") {
            continue;
        }
        let relative = safe_relative_path(name)?;
        if kind.is_dir() {
            ensure_directories(root, &relative)?;
        } else if kind.is_file() {
            let size = entry.size();
            let mode = entry
                .header()
                .mode()
                .map_err(|err| format!("JavaのTAR内の実行権限が不正です: {err}"))?;
            add_extracted_size(&mut total, size)?;
            extract_file(&mut entry, root, &relative, size, Some(mode))?;
        } else if kind.is_symlink() || kind.is_hard_link() {
            let target = entry
                .link_name()
                .map_err(|err| format!("JavaのTAR内のリンクが不正です: {err}"))?
                .ok_or_else(|| "JavaのTAR内のリンク先がありません".to_string())?;
            let target = target
                .to_str()
                .ok_or_else(|| "JavaのTAR内のリンク先を読み取れません".to_string())?;
            links.push(ArchiveLink {
                target: link_target(&relative, target, kind.is_symlink())?,
                path: relative,
                symbolic: kind.is_symlink(),
            });
        } else {
            return Err("JavaのTARにサポートされていない特殊ファイルが含まれています".into());
        }
    }
    install_links(root, links)
}

fn install_links(root: &Path, mut links: Vec<ArchiveLink>) -> Result<(), String> {
    let canonical_root = fs::canonicalize(root)
        .map_err(|err| format!("Javaのリンク展開先を確認できません: {err}"))?;
    // Links are resolved only after regular files, without ever writing through
    // one. Store relative links so publishing the directory cannot break them.
    while !links.is_empty() {
        let previous_count = links.len();
        let mut deferred = Vec::new();
        for link in links {
            ensure_directories(root, link.path.parent().unwrap_or_else(|| Path::new("")))?;
            let target = match fs::canonicalize(root.join(&link.target)) {
                Ok(target) if target.starts_with(&canonical_root) => target,
                Ok(_) => return Err("Javaのリンク先が展開先の外部を参照しています".into()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    deferred.push(link);
                    continue;
                }
                Err(err) => return Err(format!("Javaのリンク先を確認できません: {err}")),
            };
            let destination = root.join(&link.path);
            if link.symbolic {
                let relative_target = relative_link(
                    link.path.parent().unwrap_or_else(|| Path::new("")),
                    target
                        .strip_prefix(&canonical_root)
                        .map_err(|_| "Javaのリンク先が展開先の外部です".to_string())?,
                );
                create_symlink(&relative_target, &destination)?;
            } else {
                if !target.is_file() {
                    return Err("Javaのハードリンク先が通常ファイルではありません".into());
                }
                fs::hard_link(&target, &destination)
                    .map_err(|err| format!("Javaのハードリンクを作成できません: {err}"))?;
            }
        }
        if deferred.len() == previous_count {
            return Err("Javaのアーカイブに循環リンクまたは存在しないリンク先があります".into());
        }
        links = deferred;
    }
    Ok(())
}

fn relative_link(parent: &Path, target: &Path) -> PathBuf {
    let parents: Vec<_> = parent.components().collect();
    let targets: Vec<_> = target.components().collect();
    let shared = parents
        .iter()
        .zip(&targets)
        .take_while(|(left, right)| left == right)
        .count();
    let mut result = PathBuf::new();
    for _ in shared..parents.len() {
        result.push("..");
    }
    for component in &targets[shared..] {
        result.push(component);
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

#[cfg(unix)]
fn create_symlink(target: &Path, destination: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, destination)
        .map_err(|err| format!("Javaのシンボリックリンクを作成できません: {err}"))
}

#[cfg(not(unix))]
fn create_symlink(_target: &Path, _destination: &Path) -> Result<(), String> {
    Err("このOSではJavaのTARシンボリックリンクを展開できません。ZIP形式が必要です".into())
}

fn find_java(root: &Path, executable_name: &str) -> Result<PathBuf, String> {
    let canonical_root = fs::canonicalize(root)
        .map_err(|err| format!("Javaの展開ディレクトリを確認できません: {err}"))?;
    let mut homes = vec![root.to_path_buf()];
    for entry in fs::read_dir(root)
        .map_err(|err| format!("Javaの展開ディレクトリを読み取れません: {err}"))?
    {
        let entry = entry.map_err(|err| format!("Javaの展開内容を確認できません: {err}"))?;
        if entry
            .file_type()
            .map_err(|err| format!("Javaの展開内容を確認できません: {err}"))?
            .is_dir()
        {
            homes.push(entry.path());
        }
    }
    let mut executables = HashSet::new();
    for home in homes {
        for java_home in [home.clone(), home.join("Contents").join("Home")] {
            let candidate = java_home.join("bin").join(executable_name);
            if candidate.is_file() {
                let candidate = fs::canonicalize(&candidate)
                    .map_err(|err| format!("展開したJavaの実行ファイルを確認できません: {err}"))?;
                if !candidate.starts_with(&canonical_root) {
                    return Err("Java実行ファイルが展開先の外部を参照しています".into());
                }
                executables.insert(candidate);
            }
        }
    }
    if executables.len() != 1 {
        return Err(format!(
            "展開したJavaの bin/{executable_name} を一意に特定できません (候補数 {})",
            executables.len()
        ));
    }
    executables
        .into_iter()
        .next()
        .ok_or_else(|| "展開したJavaの実行ファイルがありません".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;

    // Keep all test artifacts inside the workspace, not the OS temporary folder.
    fn test_dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(".java-download-test-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap()
    }

    fn package(bytes: &[u8]) -> Package {
        Package {
            checksum: format!("{:x}", Sha256::digest(bytes)),
            link: "https://github.com/adoptium/temurin21-binaries/releases/download/test/java.zip"
                .into(),
            size: bytes.len() as u64,
            name: "java.zip".into(),
        }
    }

    fn asset(bytes: &[u8]) -> Asset {
        Asset {
            binary: Binary {
                architecture: "x64".into(),
                os: "windows".into(),
                image_type: "jre".into(),
                package: package(bytes),
            },
            version: Version { major: 21 },
        }
    }

    fn windows_platform() -> Platform {
        Platform::for_target("x86_64", "windows").unwrap()
    }

    #[test]
    fn verifies_streamed_hash_and_size() {
        let bytes = b"verified runtime archive";
        let package = validate_package(package(bytes), ArchiveFormat::Zip).unwrap();
        let mut check = IntegrityCheck::new(&package);
        for chunk in bytes.chunks(3) {
            check.update(chunk).unwrap();
        }
        check.finish().unwrap();

        let mut check = IntegrityCheck::new(&package);
        check.update(&vec![b'x'; bytes.len()]).unwrap();
        assert!(check.finish().unwrap_err().contains("SHA256"));

        let mut check = IntegrityCheck::new(&package);
        check.update(&bytes[..bytes.len() - 1]).unwrap();
        assert!(check.finish().unwrap_err().contains("サイズ"));

        let mut check = IntegrityCheck::new(&package);
        assert!(check.update(&vec![0; bytes.len() + 1]).is_err());
    }

    #[test]
    fn validates_package_metadata_and_target_without_fallback() {
        let platform = windows_platform();
        let selected = select_package(vec![asset(b"archive")], 21, platform).unwrap();
        assert_eq!(selected.size, 7);
        for case in 0..4 {
            let mut wrong = asset(b"archive");
            match case {
                0 => wrong.version.major = 22,
                1 => wrong.binary.architecture = "aarch64".into(),
                2 => wrong.binary.os = "linux".into(),
                _ => wrong.binary.image_type = "jdk".into(),
            }
            let error = select_package(vec![wrong], 21, platform).unwrap_err();
            assert!(error.contains("Java 21"));
            assert!(error.contains("windows / x64"));
        }
        assert!(select_package(Vec::new(), 21, platform).is_err());
        assert!(select_package(vec![asset(b"archive")], 17, platform).is_err());
        assert_eq!(Platform::for_target("aarch64", "macos").unwrap().os, "mac");
        assert_eq!(
            Platform::for_target("x86", "windows").unwrap().architecture,
            "x86"
        );
        assert!(Platform::for_target("riscv64", "linux").is_err());
        assert!(Platform::for_target("x86_64", "unknown").is_err());
    }

    #[test]
    fn rejects_invalid_checksum_url_size_and_archive_name() {
        for checksum in ["", "00", &"z".repeat(64), &"é".repeat(32)] {
            let mut candidate = package(b"archive");
            candidate.checksum = checksum.into();
            assert!(validate_package(candidate, ArchiveFormat::Zip).is_err());
        }
        let mut candidate = package(b"archive");
        candidate.checksum.make_ascii_uppercase();
        assert!(validate_package(candidate, ArchiveFormat::Zip).is_ok());
        for url in [
            "http://example.com/java.zip",
            "file:///java.zip",
            "https://user:secret@example.com/java.zip",
            "not a url",
        ] {
            let mut candidate = package(b"archive");
            candidate.link = url.into();
            assert!(validate_package(candidate, ArchiveFormat::Zip).is_err());
        }
        for size in [0, MAX_PACKAGE_SIZE + 1] {
            let mut candidate = package(b"archive");
            candidate.size = size;
            assert!(validate_package(candidate, ArchiveFormat::Zip).is_err());
        }
        for name in ["../java.zip", "C:java.zip", "java.tar.gz", "dir/java.zip"] {
            let mut candidate = package(b"archive");
            candidate.name = name.into();
            assert!(validate_package(candidate, ArchiveFormat::Zip).is_err());
        }
        let mut candidate = package(b"archive");
        candidate.name = "java.tar.gz".into();
        assert!(validate_package(candidate, ArchiveFormat::TarGz).is_ok());
    }

    #[test]
    fn rejects_cross_platform_path_traversal_and_aliases() {
        for path in [
            "",
            "/absolute",
            "../escape",
            "root/../../escape",
            "root/../escape",
            r"..\escape",
            r"root\..\escape",
            r"C:\escape",
            "C:escape",
            r"\\server\share",
            "root/file:stream",
            "root/NUL.txt",
            "root/COM1",
            "root/trailing.",
            "root/trailing ",
            "root/file\0",
        ] {
            assert!(safe_relative_path(path).is_err(), "{path:?}");
        }
        assert_eq!(
            safe_relative_path("./jdk-21/bin/java").unwrap(),
            PathBuf::from("jdk-21").join("bin").join("java")
        );
    }

    fn zip_file(path: &Path, entries: &[(&str, &[u8])], symlink: bool) {
        let mut archive = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, contents) in entries {
            archive
                .start_file(*name, SimpleFileOptions::default().unix_permissions(0o755))
                .unwrap();
            archive.write_all(contents).unwrap();
        }
        if symlink {
            archive
                .add_symlink("linked-java", "../outside", SimpleFileOptions::default())
                .unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn extracts_zip_and_detects_java_layout() {
        let directory = test_dir();
        let archive = directory.path().join("java.zip");
        let destination = directory.path().join("runtime");
        zip_file(&archive, &[("jdk-21/bin/java.exe", b"not executed")], false);
        extract_archive(&archive, &destination, ArchiveFormat::Zip).unwrap();
        let executable = find_java(&destination, "java.exe").unwrap();
        assert_eq!(fs::read(&executable).unwrap(), b"not executed");
        assert!(executable.starts_with(fs::canonicalize(&destination).unwrap()));
    }

    #[test]
    fn rejects_zip_traversal_symlinks_and_duplicate_files() {
        for case in 0..3 {
            let directory = test_dir();
            let archive = directory.path().join("java.zip");
            let destination = directory.path().join("runtime");
            match case {
                0 => zip_file(&archive, &[("../outside", b"bad")], false),
                1 => zip_file(&archive, &[("bin/java", b"good")], true),
                _ => {
                    zip_file(
                        &archive,
                        &[("bin/java", b"first"), ("bin/./java", b"second")],
                        false,
                    );
                }
            }
            assert!(extract_archive(&archive, &destination, ArchiveFormat::Zip).is_err());
            assert!(!directory.path().join("outside").exists());
        }
    }

    fn tar_bytes(entries: &[(&str, tar::EntryType, &str, &[u8])]) -> Vec<u8> {
        let mut archive = tar::Builder::new(Vec::new());
        for (name, kind, target, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(*kind);
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            // Set raw names so hostile paths aren't rejected by the test builder.
            header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
            if !target.is_empty() {
                header.set_link_name(target).unwrap();
            }
            header.set_cksum();
            archive.append(&header, Cursor::new(*bytes)).unwrap();
        }
        archive.into_inner().unwrap()
    }

    #[test]
    fn extracts_tar_gz_and_preserves_executable_permissions() {
        let directory = test_dir();
        let archive = directory.path().join("java.tar.gz");
        let destination = directory.path().join("runtime");
        let bytes = tar_bytes(&[(
            "jdk-21/bin/java",
            tar::EntryType::Regular,
            "",
            b"not executed",
        )]);
        let mut gzip = flate2::write::GzEncoder::new(
            File::create(&archive).unwrap(),
            flate2::Compression::default(),
        );
        gzip.write_all(&bytes).unwrap();
        gzip.finish().unwrap();
        extract_archive(&archive, &destination, ArchiveFormat::TarGz).unwrap();
        let executable = find_java(&destination, "java").unwrap();
        assert_eq!(fs::read(&executable).unwrap(), b"not executed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(executable).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
    }

    #[test]
    fn rejects_tar_traversal_escaping_links_and_special_files() {
        for (name, kind, target) in [
            ("../outside", tar::EntryType::Regular, ""),
            ("/outside", tar::EntryType::Regular, ""),
            ("runtime/link", tar::EntryType::Symlink, "../../outside"),
            ("runtime/link", tar::EntryType::Symlink, "/outside"),
            ("runtime/link", tar::EntryType::Link, "../outside"),
            ("runtime/link", tar::EntryType::Link, "missing"),
            ("runtime/device", tar::EntryType::Char, ""),
            ("runtime/pipe", tar::EntryType::Fifo, ""),
        ] {
            let directory = test_dir();
            let destination = directory.path().join("runtime");
            fs::create_dir(&destination).unwrap();
            let bytes = tar_bytes(&[(name, kind, target, b"")]);
            assert!(
                extract_tar(Cursor::new(bytes), &destination).is_err(),
                "{name}"
            );
            assert!(!directory.path().join("outside").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn extracts_safe_hard_links_and_rejects_dangling_links() {
        let directory = test_dir();
        let bytes = tar_bytes(&[
            ("jdk/link", tar::EntryType::Link, "jdk/bin/java", b""),
            ("jdk/bin/java", tar::EntryType::Regular, "", b"java"),
        ]);
        extract_tar(Cursor::new(bytes), directory.path()).unwrap();
        assert_eq!(
            fs::read(directory.path().join("jdk").join("link")).unwrap(),
            b"java"
        );

        let directory = test_dir();
        let bytes = tar_bytes(&[("jdk/link", tar::EntryType::Link, "missing", b"")]);
        assert!(extract_tar(Cursor::new(bytes), directory.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn safe_symbolic_links_survive_relocation_and_cannot_be_write_parents() {
        let directory = test_dir();
        let staging = directory.path().join("staging");
        fs::create_dir(&staging).unwrap();
        let bytes = tar_bytes(&[
            ("jdk/lib/java", tar::EntryType::Symlink, "../bin/java", b""),
            ("jdk/bin/java", tar::EntryType::Regular, "", b"java"),
        ]);
        extract_tar(Cursor::new(bytes), &staging).unwrap();
        let installed = directory.path().join("installed");
        fs::rename(&staging, &installed).unwrap();
        assert_eq!(fs::read(installed.join("jdk/lib/java")).unwrap(), b"java");

        let directory = test_dir();
        let bytes = tar_bytes(&[
            ("jdk/bin/java", tar::EntryType::Regular, "", b"java"),
            ("jdk/link", tar::EntryType::Symlink, "bin", b""),
            (
                "jdk/link/other",
                tar::EntryType::Symlink,
                "../bin/java",
                b"",
            ),
        ]);
        assert!(extract_tar(Cursor::new(bytes), directory.path()).is_err());
        assert!(!directory.path().join("jdk/bin/other").exists());
    }

    #[test]
    fn detects_direct_wrapped_and_macos_layouts_but_not_arbitrary_nesting() {
        for layout in [
            PathBuf::from("bin"),
            PathBuf::from("jdk-21").join("bin"),
            PathBuf::from("Contents").join("Home").join("bin"),
            PathBuf::from("jdk-21.jre")
                .join("Contents")
                .join("Home")
                .join("bin"),
        ] {
            let directory = test_dir();
            let bin = directory.path().join(layout);
            fs::create_dir_all(&bin).unwrap();
            fs::write(bin.join("java"), b"not executed").unwrap();
            assert!(find_java(directory.path(), "java").is_ok());
        }
        let directory = test_dir();
        assert!(find_java(directory.path(), "java").is_err());
        fs::create_dir_all(directory.path().join("a").join("b").join("bin")).unwrap();
        fs::write(
            directory
                .path()
                .join("a")
                .join("b")
                .join("bin")
                .join("java"),
            b"not executed",
        )
        .unwrap();
        assert!(find_java(directory.path(), "java").is_err());
        for home in ["one", "two"] {
            let bin = directory.path().join(home).join("bin");
            fs::create_dir_all(&bin).unwrap();
            fs::write(bin.join("java"), b"not executed").unwrap();
        }
        assert!(find_java(directory.path(), "java").is_err());
    }

    #[test]
    fn requires_exact_probed_major_and_native_architecture() {
        let mut runtime = JavaRuntime {
            executable: PathBuf::from("java"),
            major_version: 21,
            architecture: std::env::consts::ARCH.into(),
        };
        assert!(validate_runtime(&runtime, 21).is_ok());
        assert!(validate_runtime(&runtime, 17).is_err());
        assert!(validate_runtime(&runtime, 25).is_err());
        runtime.architecture = "not-the-native-architecture".into();
        assert!(validate_runtime(&runtime, 21).is_err());
    }

    #[test]
    fn failed_extraction_cleans_staging_without_publishing() {
        let directory = test_dir();
        let downloads = directory.path().join("downloads");
        let installed = directory.path().join("installed");
        fs::create_dir(&downloads).unwrap();
        fs::create_dir(&installed).unwrap();
        let failed_install = || -> Result<(), String> {
            let staging = tempfile::Builder::new().tempdir_in(&downloads).unwrap();
            let archive = staging.path().join("package");
            zip_file(
                &archive,
                &[("jdk/bin/java", b"partial"), ("../outside", b"bad")],
                false,
            );
            extract_archive(
                &archive,
                &staging.path().join("runtime"),
                ArchiveFormat::Zip,
            )
        };
        assert!(failed_install().is_err());
        assert_eq!(fs::read_dir(&downloads).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&installed).unwrap().count(), 0);
    }

    #[test]
    fn limits_expansion_and_calculates_relocatable_links() {
        let mut total = MAX_EXTRACTED_SIZE - 1;
        add_extracted_size(&mut total, 1).unwrap();
        assert!(add_extracted_size(&mut total, 1).is_err());
        assert_eq!(
            relative_link(
                &PathBuf::from("jdk").join("lib"),
                &PathBuf::from("jdk").join("bin").join("java")
            ),
            PathBuf::from("..").join("bin").join("java")
        );
        assert!(link_target(Path::new("jdk/link"), "../../escape", true).is_err());
    }

    fn staged_runtime(downloads: &Path) -> (tempfile::TempDir, JavaRuntime) {
        let staging = tempfile::Builder::new().tempdir_in(downloads).unwrap();
        let bin = staging.path().join("runtime").join("jdk-21").join("bin");
        fs::create_dir_all(&bin).unwrap();
        let java = bin.join(Platform::current().unwrap().executable_name());
        fs::write(&java, b"offline publication fixture; never executed").unwrap();
        fs::write(staging.path().join("package"), b"archive fixture").unwrap();
        let runtime = JavaRuntime {
            executable: fs::canonicalize(java).unwrap(),
            major_version: 21,
            architecture: std::env::consts::ARCH.into(),
        };
        (staging, runtime)
    }

    #[tokio::test]
    async fn publication_persists_after_staging_cleanup_without_overwriting_existing_runtime() {
        let directory = test_dir();
        let root = fs::canonicalize(directory.path()).unwrap();
        let downloads = root.join("downloads");
        let installed = root.join("installed");
        fs::create_dir(&downloads).unwrap();
        fs::create_dir(&installed).unwrap();
        let (staging, runtime) = staged_runtime(&downloads);
        let published = publish_runtime(&staging, runtime, 21, &installed)
            .await
            .unwrap();
        drop(staging);
        assert!(published.executable.is_absolute());
        assert!(published.executable.starts_with(&installed));
        assert!(published.executable.is_file());
        assert_eq!(fs::read_dir(&downloads).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&installed).unwrap().count(), 1);

        let (staging, runtime) = staged_runtime(&downloads);
        let collision = installed.join(format!(
            "java-21-{}",
            staging.path().file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir(&collision).unwrap();
        fs::write(collision.join("user-file"), b"unchanged").unwrap();
        assert!(publish_runtime(&staging, runtime, 21, &installed)
            .await
            .is_err());
        drop(staging);
        assert_eq!(fs::read(collision.join("user-file")).unwrap(), b"unchanged");
        assert!(published.executable.is_file());
        assert_eq!(fs::read_dir(&downloads).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn published_java_homes_fit_the_discovery_depth_limit() {
        for (home, expected_depth) in [
            (PathBuf::from("jdk-21"), 2),
            (PathBuf::from("jdk-21.jre").join("Contents").join("Home"), 4),
        ] {
            let directory = test_dir();
            let root = fs::canonicalize(directory.path()).unwrap();
            let downloads = root.join("downloads");
            let installed = root.join("installed");
            fs::create_dir(&downloads).unwrap();
            fs::create_dir(&installed).unwrap();
            let staging = tempfile::Builder::new().tempdir_in(&downloads).unwrap();
            let payload = staging.path().join("runtime");
            let bin = payload.join(&home).join("bin");
            fs::create_dir_all(&bin).unwrap();
            fs::write(bin.join("java"), b"layout fixture; never executed").unwrap();
            let runtime = JavaRuntime {
                executable: find_java(&payload, "java").unwrap(),
                major_version: 21,
                architecture: std::env::consts::ARCH.into(),
            };
            let published = publish_runtime(&staging, runtime, 21, &installed)
                .await
                .unwrap();
            drop(staging);
            let java_home = published.executable.parent().unwrap().parent().unwrap();
            assert_eq!(
                java_home
                    .strip_prefix(&installed)
                    .unwrap()
                    .components()
                    .count(),
                expected_depth
            );
            assert!(published.executable.is_file());
        }
    }

    #[tokio::test]
    async fn failed_runtime_validation_cleans_staging_and_never_publishes() {
        let directory = test_dir();
        let downloads = directory.path().join("downloads");
        let installed = directory.path().join("installed");
        fs::create_dir(&downloads).unwrap();
        fs::create_dir(&installed).unwrap();
        let (staging, runtime) = staged_runtime(&downloads);
        assert!(publish_runtime(&staging, runtime, 17, &installed)
            .await
            .is_err());
        drop(staging);
        assert_eq!(fs::read_dir(&downloads).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&installed).unwrap().count(), 0);
    }

    #[test]
    fn integrity_failures_drop_partial_downloads() {
        let directory = test_dir();
        let downloads = directory.path().join("downloads");
        fs::create_dir(&downloads).unwrap();
        let candidate = validate_package(package(b"expected"), ArchiveFormat::Zip).unwrap();
        for bytes in [&b"tampered"[..], &b"short"[..]] {
            let attempt = || -> Result<(), String> {
                let staging = tempfile::Builder::new().tempdir_in(&downloads).unwrap();
                fs::write(staging.path().join("package"), bytes).unwrap();
                let mut check = IntegrityCheck::new(&candidate);
                check.update(bytes)?;
                check.finish()
            };
            assert!(attempt().is_err());
            assert_eq!(fs::read_dir(&downloads).unwrap().count(), 0);
        }
    }

    #[tokio::test]
    async fn invalid_major_returns_localized_actionable_error_without_creating_files() {
        let directory = test_dir();
        let error = install_runtime_at(0, &|_| {}, directory.path())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Java 0"));
        assert!(error.contains("再度起動"));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    #[ignore = "Downloads and runs an official Java 21 JRE; explicitly opt in to network validation"]
    async fn downloads_verified_java_21_into_isolated_managed_root() {
        let directory = test_dir();
        let runtime = install_runtime_at(21, &|label| eprintln!("{label}"), directory.path())
            .await
            .unwrap();
        let root = fs::canonicalize(directory.path()).unwrap();
        assert!(runtime.executable.is_absolute());
        assert!(runtime.executable.starts_with(root.join("installed")));
        assert!(runtime.executable.is_file());
        assert_eq!(runtime.major_version, 21);
        assert!(runtime.supports(21));
        assert_eq!(runtime.architecture, std::env::consts::ARCH);
        let probed = probe_runtime(runtime.executable.clone()).await.unwrap();
        assert_eq!(probed.major_version, 21);
        assert!(probed.supports(21));
        assert_eq!(fs::read_dir(root.join("downloads")).unwrap().count(), 0);
        assert_eq!(fs::read_dir(root.join("installed")).unwrap().count(), 1);
    }
}
