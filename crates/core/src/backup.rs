//! Local game-data backups. Callers must hold the launch/maintenance guard and
//! run these synchronous operations off the async runtime.

use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, Metadata},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
};
use tempfile::{Builder, NamedTempFile, TempDir};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const FORMAT_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_OPTIONS_BYTES: u64 = 8 * 1024 * 1024;
const MAX_MARKER_BYTES: u64 = 256 * 1024;
const MAX_ENTRIES: usize = 200_000;
const MAX_EXPANDED_BYTES: u64 = 4 * 1024 * 1024 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 4096;
const MAX_DEPTH: usize = 64;
const RESTORE_MARKER: &str = ".train-launcher-restore.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupScope {
    Settings,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupProfile {
    pub id: String,
    pub name: String,
    pub minecraft_version: String,
    pub mod_loader: Option<String>,
    #[serde(default)]
    pub managed_mod_filenames: Vec<String>,
    #[serde(default)]
    pub managed_resource_pack_filenames: Vec<String>,
    #[serde(default)]
    pub enabled_resource_packs: Vec<String>,
    #[serde(default)]
    pub last_server_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: String,
    pub profile: BackupProfile,
    pub scope: BackupScope,
    pub created_at: String,
    pub size_bytes: u64,
    pub automatic: bool,
}

#[derive(Debug, Serialize)]
pub struct BackupList {
    pub backups: Vec<BackupInfo>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RestoreResult {
    pub backup: BackupInfo,
    pub safety_backup: Option<BackupInfo>,
    pub warnings: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format_version: u32,
    id: String,
    profile: BackupProfile,
    scope: BackupScope,
    created_at: String,
    automatic: bool,
}

impl Manifest {
    fn info(&self, size_bytes: u64) -> BackupInfo {
        BackupInfo {
            id: self.id.clone(),
            profile: self.profile.clone(),
            scope: self.scope,
            created_at: self.created_at.clone(),
            size_bytes,
            automatic: self.automatic,
        }
    }
}

struct Entry {
    relative: PathBuf,
    directory: bool,
    size: u64,
    index: usize,
}

type Archive = ZipArchive<BufReader<File>>;

fn error(message: impl Into<String>) -> CoreError {
    CoreError::Backup(message.into())
}

fn context(action: &str, path: &Path, cause: impl std::fmt::Display) -> CoreError {
    error(format!("{action}「{}」: {cause}", path.display()))
}

fn valid_id(id: &str) -> Result<(), CoreError> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(error(
            "バックアップIDが不正です。バックアップ一覧から選び直してください。",
        ));
    }
    Ok(())
}

fn valid_component(name: &str) -> Result<(), CoreError> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        || matches!(
            stem.as_str(),
            "COM¹" | "COM²" | "COM³" | "LPT¹" | "LPT²" | "LPT³"
        );
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|ch| ch.is_control() || "/\\:<>\"|?*".contains(ch))
        || device
    {
        return Err(error(format!(
            "安全でないファイル名「{name}」が含まれています。"
        )));
    }
    Ok(())
}

fn validate_profile(profile: &BackupProfile) -> Result<(), CoreError> {
    if profile.minecraft_version.trim().is_empty()
        || profile.minecraft_version != profile.minecraft_version.trim()
        || matches!(
            profile.minecraft_version.as_str(),
            "latest-release" | "latest-snapshot"
        )
    {
        return Err(error(
            "Minecraftのバージョンが未確定です。具体的なバージョンを選んでください。",
        ));
    }
    for name in profile
        .managed_mod_filenames
        .iter()
        .chain(&profile.managed_resource_pack_filenames)
    {
        valid_component(name)?;
    }
    Ok(())
}

fn normalized_loader(loader: Option<&str>) -> Option<String> {
    match loader.map(str::trim).map(str::to_ascii_lowercase) {
        None => None,
        Some(value) if value.is_empty() || value == "vanilla" || value == "none" => None,
        other => other,
    }
}

fn excluded(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "assets"
            | "libraries"
            | "versions"
            | "runtime"
            | "logs"
            | "crash-reports"
            | "train-launcher-process.log"
            | "backups"
            | "webcache"
            | "webcache2"
    ) || name.starts_with("launcher_")
        || name.starts_with(".train-launcher-")
}

fn regular_metadata(path: &Path) -> Result<Metadata, CoreError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|cause| context("ファイル情報を確認できません", path, cause))?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.file_type().is_symlink() || reparse || (!metadata.is_file() && !metadata.is_dir()) {
        return Err(context(
            "リンク・ジャンクション・特殊ファイルは処理できません",
            path,
            "通常のファイルまたはフォルダに変更してください",
        ));
    }
    Ok(metadata)
}

fn optional_metadata(path: &Path) -> Result<Option<Metadata>, CoreError> {
    match fs::symlink_metadata(path) {
        Ok(_) => regular_metadata(path).map(Some),
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(cause) => Err(context("ファイル情報を確認できません", path, cause)),
    }
}

// Resolve a not-yet-created target without treating missing ancestors as paths
// relative to a different working directory. The explicitly selected root may
// itself be a link; descendants are checked separately and never followed.
fn resolved_path(path: &Path) -> Result<PathBuf, CoreError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|cause| error(format!("作業フォルダを確認できません: {cause}")))?
            .join(path)
    };
    let mut ancestor = absolute.as_path();
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => {
                let name = ancestor.file_name().ok_or_else(|| {
                    context("フォルダを解決できません", path, "パスを確認してください")
                })?;
                missing.push(name.to_os_string());
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| error("フォルダの親がありません。"))?;
            }
            Err(cause) => return Err(context("フォルダを確認できません", ancestor, cause)),
        }
    }
    let mut resolved = fs::canonicalize(ancestor)
        .map_err(|cause| context("フォルダを解決できません", ancestor, cause))?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn within(path: &Path, parent: &Path) -> bool {
    #[cfg(windows)]
    {
        let path: Vec<_> = path.components().collect();
        let parent: Vec<_> = parent.components().collect();
        path.len() >= parent.len()
            && path.iter().zip(parent.iter()).all(|(left, right)| {
                left.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
            })
    }
    #[cfg(not(windows))]
    {
        path.starts_with(parent)
    }
}

fn game_root(game_dir: &Path, backups_dir: &Path) -> Result<PathBuf, CoreError> {
    let root = resolved_path(game_dir)?;
    if root.parent().is_none() || root.file_name().is_none() {
        return Err(error(
            "ドライブやファイルシステムのルートはゲームフォルダに指定できません。",
        ));
    }
    let cwd = resolved_path(Path::new("."))?;
    if within(&cwd, &root) {
        return Err(error(
            "現在の作業フォルダまたはその親はバックアップ・復元できません。",
        ));
    }
    if let Some(home) = dirs::home_dir() {
        let home = resolved_path(&home)?;
        if within(&home, &root) {
            return Err(error(
                "ホームフォルダまたはその親はバックアップ・復元できません。",
            ));
        }
    }
    if let Some(metadata) = optional_metadata(&root)? {
        if !metadata.is_dir() {
            return Err(context(
                "ゲームフォルダではありません",
                &root,
                "フォルダを選んでください",
            ));
        }
        match fs::symlink_metadata(root.join(".git")) {
            Ok(_) => return Err(error("Gitリポジトリを含むゲームフォルダは処理できません。")),
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => {}
            Err(cause) => return Err(context(".gitを確認できません", &root, cause)),
        }
    }
    if within(&resolved_path(backups_dir)?, &root) {
        return Err(error(
            "バックアップ保存先がゲームフォルダ内にあります。再帰的なコピーを防ぐため、別のフォルダを選んでください。",
        ));
    }
    Ok(root)
}

fn children(directory: &Path) -> Result<Vec<PathBuf>, CoreError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)
        .map_err(|cause| context("フォルダを読み取れません", directory, cause))?
    {
        let entry = entry.map_err(|cause| context("フォルダを読み取れません", directory, cause))?;
        entries.push(entry.path());
        if entries.len() > MAX_ENTRIES {
            return Err(error(
                "ファイル数が上限を超えています。フォルダを分けてください。",
            ));
        }
    }
    entries.sort();
    Ok(entries)
}

fn filename(path: &Path) -> Result<&str, CoreError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| context("ファイル名を読み取れません", path, "UTF-8の名前が必要です"))
}

fn included_top_entries(root: &Path) -> Result<Vec<PathBuf>, CoreError> {
    let mut included = Vec::new();
    for path in children(root)? {
        regular_metadata(&path)?;
        let name = filename(&path)?;
        if name.eq_ignore_ascii_case(".git") {
            return Err(context(
                "Gitリポジトリは処理できません",
                &path,
                ".gitが含まれています",
            ));
        }
        if !excluded(name) {
            valid_component(name)?;
            included.push(path);
        }
    }
    Ok(included)
}

fn scan_tree(
    root: &Path,
    path: &Path,
    entries: &mut Vec<Entry>,
    total: &mut u64,
    depth: usize,
) -> Result<(), CoreError> {
    let metadata = regular_metadata(path)?;
    let name = filename(path)?;
    valid_component(name)?;
    if name.eq_ignore_ascii_case(".git") {
        return Err(context(
            "Gitリポジトリは処理できません",
            path,
            ".gitが含まれています",
        ));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|cause| context("パスが不正です", path, cause))?;
    if depth > MAX_DEPTH || relative.as_os_str().len() > MAX_PATH_BYTES {
        return Err(context(
            "パスが長すぎます",
            path,
            "フォルダ構造を浅くしてください",
        ));
    }
    if entries.len() >= MAX_ENTRIES - 2 {
        return Err(error("バックアップのファイル数が上限を超えています。"));
    }
    let size = if metadata.is_file() {
        metadata.len()
    } else {
        0
    };
    *total = total
        .checked_add(size)
        .ok_or_else(|| error("データサイズが大きすぎます。"))?;
    if *total > MAX_EXPANDED_BYTES {
        return Err(error(
            "バックアップの展開サイズ上限（4 TiB）を超えています。",
        ));
    }
    entries.push(Entry {
        relative: relative.to_path_buf(),
        directory: metadata.is_dir(),
        size,
        index: 0,
    });
    if metadata.is_dir() {
        for child in children(path)? {
            scan_tree(root, &child, entries, total, depth + 1)?;
        }
    }
    Ok(())
}

fn snapshot_entries(root: &Path, scope: BackupScope) -> Result<Vec<Entry>, CoreError> {
    let mut entries = Vec::new();
    let mut total = 0;
    match scope {
        BackupScope::Settings => {
            let options = root.join("options.txt");
            let metadata = optional_metadata(&options)?.ok_or_else(|| {
                error("options.txtがありません。Minecraftを一度起動して設定を保存してください。")
            })?;
            if !metadata.is_file() || metadata.len() == 0 {
                return Err(error("options.txtが空または通常のファイルではありません。Minecraftを一度起動してください。"));
            }
            if metadata.len() > MAX_OPTIONS_BYTES {
                return Err(error("options.txtが大きすぎます（上限8 MiB）。"));
            }
            scan_tree(root, &options, &mut entries, &mut total, 1)?;
        }
        BackupScope::Full => {
            for path in included_top_entries(root)? {
                scan_tree(root, &path, &mut entries, &mut total, 1)?;
            }
        }
    }
    Ok(entries)
}

fn sync_directory(path: &Path) -> Result<(), CoreError> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|cause| context("フォルダをディスクに保存できません", path, cause))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| error("保存先の親フォルダがありません。"))?;
    let mut file: NamedTempFile = Builder::new()
        .prefix(".train-launcher-write-")
        .tempfile_in(parent)
        .map_err(|cause| context("一時ファイルを作成できません", parent, cause))?;
    file.write_all(bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|cause| context("ファイルをディスクに保存できません", path, cause))?;
    file.persist(path)
        .map_err(|cause| context("ファイルを置き換えられません", path, cause.error))?;
    sync_directory(parent)
}

/// Creates an immutable ZIP. Excluded launcher/shared entries are never opened.
pub fn create_backup(
    backups_dir: &Path,
    game_dir: &Path,
    profile: &BackupProfile,
    scope: BackupScope,
    automatic: bool,
) -> Result<BackupInfo, CoreError> {
    create_backup_internal(backups_dir, game_dir, profile, scope, automatic, false)
}

fn create_backup_internal(
    backups_dir: &Path,
    game_dir: &Path,
    profile: &BackupProfile,
    scope: BackupScope,
    automatic: bool,
    allow_interrupted: bool,
) -> Result<BackupInfo, CoreError> {
    validate_profile(profile)?;
    let root = game_root(game_dir, backups_dir)?;
    if !allow_interrupted {
        ensure_restore_complete(&root)?;
    }
    let entries = snapshot_entries(&root, scope)?;
    fs::create_dir_all(backups_dir)
        .map_err(|cause| context("バックアップ保存先を作成できません", backups_dir, cause))?;
    let backups_root = resolved_path(backups_dir)?;
    let prefix = format!("backup_{}_", chrono::Utc::now().format("%Y%m%dT%H%M%S%3f"));
    let mut output = Builder::new()
        .prefix(&prefix)
        .suffix(".pending")
        .tempfile_in(&backups_root)
        .map_err(|cause| {
            context(
                "バックアップ用ファイルを作成できません",
                &backups_root,
                cause,
            )
        })?;
    let id = filename(output.path())?
        .strip_suffix(".pending")
        .ok_or_else(|| error("バックアップIDを生成できません。"))?
        .to_owned();
    valid_id(&id)?;
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        id: id.clone(),
        profile: profile.clone(),
        scope,
        created_at: chrono::Utc::now().to_rfc3339(),
        automatic,
    };
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|cause| error(format!("バックアップ情報を書き出せません: {cause}")))?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(error("プロファイルのバックアップ情報が大きすぎます。"));
    }
    {
        let writer = BufWriter::new(output.as_file_mut());
        let mut archive = ZipWriter::new(writer);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true)
            .unix_permissions(0o600);
        archive
            .start_file("manifest.json", options)
            .map_err(|cause| error(format!("ZIP情報を書き込めません: {cause}")))?;
        archive
            .write_all(&manifest_bytes)
            .map_err(|cause| error(format!("ZIP情報を書き込めません: {cause}")))?;
        archive
            .add_directory("data/", options.unix_permissions(0o700))
            .map_err(|cause| error(format!("ZIPフォルダを書き込めません: {cause}")))?;
        for entry in entries {
            let path = root.join(&entry.relative);
            let metadata = regular_metadata(&path)?;
            if metadata.is_dir() != entry.directory
                || (!entry.directory && metadata.len() != entry.size)
            {
                return Err(context(
                    "バックアップ中にファイルが変更されました",
                    &path,
                    "再試行してください",
                ));
            }
            let relative = entry
                .relative
                .components()
                .map(|component| {
                    component
                        .as_os_str()
                        .to_str()
                        .ok_or_else(|| error("UTF-8以外のファイル名は保存できません。"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            let name = format!("data/{relative}");
            if entry.directory {
                archive
                    .add_directory(format!("{name}/"), options.unix_permissions(0o700))
                    .map_err(|cause| context("ZIPフォルダを書き込めません", &path, cause))?;
            } else {
                let input = File::open(&path)
                    .map_err(|cause| context("ファイルを開けません", &path, cause))?;
                #[cfg(unix)]
                let options = {
                    use std::os::unix::fs::PermissionsExt;
                    options.unix_permissions(metadata.permissions().mode() & 0o777)
                };
                archive
                    .start_file(name, options)
                    .map_err(|cause| context("ZIPファイルを書き込めません", &path, cause))?;
                let copied = io::copy(
                    &mut BufReader::new(input).take(entry.size + 1),
                    &mut archive,
                )
                .map_err(|cause| context("バックアップを書き込めません", &path, cause))?;
                if copied != entry.size {
                    return Err(context(
                        "バックアップ中にファイルサイズが変わりました",
                        &path,
                        "再試行してください",
                    ));
                }
            }
        }
        archive
            .finish()
            .map_err(|cause| error(format!("ZIPを完了できません: {cause}")))?
            .flush()
            .map_err(|cause| error(format!("ZIPを保存できません: {cause}")))?;
    }
    output.as_file().sync_all().map_err(|cause| {
        context(
            "バックアップをディスクに保存できません",
            output.path(),
            cause,
        )
    })?;
    {
        // Reject case-folding collisions before publishing an archive that
        // would be unsafe or ambiguous when restored on another platform.
        let (mut verification, _) = open_archive(output.path())?;
        inspect_archive(&mut verification, &id)?;
    }
    let size = output
        .as_file()
        .metadata()
        .map_err(|cause| context("バックアップサイズを確認できません", output.path(), cause))?
        .len();
    let destination = backups_root.join(format!("{id}.zip"));
    output.persist_noclobber(&destination).map_err(|cause| {
        context(
            "バックアップを確定できません（既存データは上書きしません）",
            &destination,
            cause.error,
        )
    })?;
    sync_directory(&backups_root)?;
    Ok(manifest.info(size))
}

fn read_bounded(reader: impl Read, limit: u64, description: &str) -> Result<Vec<u8>, CoreError> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|cause| {
            error(format!(
                "{description}を読み取れません（破損の可能性があります）: {cause}"
            ))
        })?;
    if bytes.len() as u64 > limit {
        return Err(error(format!("{description}がサイズ上限を超えています。")));
    }
    Ok(bytes)
}

fn archive_path(name: &str, directory: bool) -> Result<PathBuf, CoreError> {
    if name.len() > MAX_PATH_BYTES || name.contains('\\') || name.starts_with('/') {
        return Err(error(format!("ZIPのパス「{name}」が不正です。")));
    }
    let name = if directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    let components: Vec<_> = name.split('/').collect();
    if components.len() > MAX_DEPTH + 1 || components.first() != Some(&"data") {
        return Err(error(format!(
            "ZIPに許可されていないパス「{name}」があります。"
        )));
    }
    if components.len() == 1 && !directory {
        return Err(error("ZIPのdataはフォルダである必要があります。"));
    }
    let mut relative = PathBuf::new();
    for (index, component) in components.iter().skip(1).enumerate() {
        valid_component(component)?;
        if component.eq_ignore_ascii_case(".git") || (index == 0 && excluded(component)) {
            return Err(error(format!(
                "ZIPに復元対象外のパス「{name}」があります。"
            )));
        }
        relative.push(component);
    }
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(error(format!("ZIPの相対パス「{name}」が不正です。")));
    }
    Ok(relative)
}

fn inspect_archive(archive: &mut Archive, id: &str) -> Result<(Manifest, Vec<Entry>), CoreError> {
    if archive.len() > MAX_ENTRIES {
        return Err(error("ZIPのファイル数が上限を超えています。"));
    }
    let mut manifest = None;
    let mut entries = Vec::new();
    let mut explicit = HashSet::new();
    let mut paths = HashMap::<String, (String, bool)>::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|cause| error(format!("ZIPの項目を読み取れません: {cause}")))?;
        let name = std::str::from_utf8(file.name_raw())
            .map_err(|_| error("ZIPのファイル名がUTF-8ではありません。"))?
            .to_owned();
        let directory = file.is_dir();
        if let Some(mode) = file.unix_mode() {
            let kind = mode & 0o170000;
            if kind != 0 && kind != if directory { 0o040000 } else { 0o100000 } {
                return Err(error(format!(
                    "ZIPのリンク・特殊ファイル「{name}」は復元できません。"
                )));
            }
        }
        if file.compressed_size() > MAX_EXPANDED_BYTES || (directory && file.size() != 0) {
            return Err(error(format!("ZIPの項目「{name}」のサイズが不正です。")));
        }
        total = total
            .checked_add(file.size())
            .ok_or_else(|| error("ZIPのサイズが不正です。"))?;
        if total > MAX_EXPANDED_BYTES {
            return Err(error("ZIPの展開サイズ上限（4 TiB）を超えています。"));
        }
        if name == "manifest.json" {
            if manifest.is_some() || directory || file.size() > MAX_MANIFEST_BYTES {
                return Err(error(
                    "ZIPのmanifest.jsonが重複・不正・大きすぎる状態です。",
                ));
            }
            let bytes = read_bounded(&mut file, MAX_MANIFEST_BYTES, "バックアップ情報")?;
            manifest = Some(serde_json::from_slice::<Manifest>(&bytes).map_err(|cause| {
                error(format!(
                    "バックアップ情報（manifest.json）が壊れています: {cause}"
                ))
            })?);
            continue;
        }
        let relative = archive_path(&name, directory)?;
        let normalized = name.trim_end_matches('/').to_lowercase();
        if !explicit.insert(normalized) {
            return Err(error(format!("ZIPのパス「{name}」が重複しています。")));
        }
        let components: Vec<_> = relative.components().collect();
        let mut spelling = String::new();
        for (part_index, part) in components.iter().enumerate() {
            if !spelling.is_empty() {
                spelling.push('/');
            }
            spelling.push_str(&part.as_os_str().to_string_lossy());
            let is_directory = part_index + 1 < components.len() || directory;
            let key = spelling.to_lowercase();
            if let Some((previous, was_directory)) = paths.get(&key) {
                if previous != &spelling || *was_directory != is_directory {
                    return Err(error(format!(
                        "ZIPのファイル名・フォルダ名が衝突しています: {name}"
                    )));
                }
            } else {
                paths.insert(key, (spelling.clone(), is_directory));
                if paths.len() > MAX_ENTRIES * 2 {
                    return Err(error("ZIPのフォルダ数が上限を超えています。"));
                }
            }
        }
        entries.push(Entry {
            relative,
            directory,
            size: file.size(),
            index,
        });
    }
    let manifest = manifest.ok_or_else(|| error("ZIPにmanifest.jsonがありません。"))?;
    if manifest.format_version != FORMAT_VERSION {
        return Err(error(format!(
            "未対応のバックアップ形式（{}）です。ランチャーの対応バージョンを確認してください。",
            manifest.format_version
        )));
    }
    valid_id(&manifest.id)?;
    if manifest.id != id {
        return Err(error(
            "バックアップIDとZIP内の情報が一致しません。ファイル名を変更しないでください。",
        ));
    }
    validate_profile(&manifest.profile)?;
    chrono::DateTime::parse_from_rfc3339(&manifest.created_at)
        .map_err(|cause| error(format!("バックアップの作成日時が不正です: {cause}")))?;
    if manifest.scope == BackupScope::Settings {
        let data: Vec<_> = entries
            .iter()
            .filter(|entry| !entry.relative.as_os_str().is_empty())
            .collect();
        if data.len() != 1
            || data[0].relative != Path::new("options.txt")
            || data[0].directory
            || data[0].size == 0
            || data[0].size > MAX_OPTIONS_BYTES
        {
            return Err(error(
                "設定バックアップには空でないoptions.txtのみが必要です（上限8 MiB）。",
            ));
        }
    }
    Ok((manifest, entries))
}

fn open_archive(path: &Path) -> Result<(Archive, u64), CoreError> {
    let metadata = regular_metadata(path)?;
    if !metadata.is_file() {
        return Err(context(
            "バックアップがファイルではありません",
            path,
            "ZIPを選んでください",
        ));
    }
    let file =
        File::open(path).map_err(|cause| context("バックアップを開けません", path, cause))?;
    let archive = ZipArchive::new(BufReader::new(file))
        .map_err(|cause| context("ZIPが壊れているか未対応の形式です", path, cause))?;
    Ok((archive, metadata.len()))
}

/// Lists manifest metadata without decompressing large worlds. Restore verifies
/// every entry's bytes/CRC before touching existing game data.
pub fn list_backups(backups_dir: &Path) -> Result<BackupList, CoreError> {
    let mut result = BackupList {
        backups: Vec::new(),
        warnings: Vec::new(),
    };
    if optional_metadata(backups_dir)?.is_none() {
        return Ok(result);
    }
    for path in children(backups_dir)? {
        if path.extension().and_then(|extension| extension.to_str()) != Some("zip") {
            continue;
        }
        let read = || -> Result<BackupInfo, CoreError> {
            let id = path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| error("バックアップのファイル名を読み取れません。"))?;
            valid_id(id)?;
            let (mut archive, size) = open_archive(&path)?;
            let (manifest, _) = inspect_archive(&mut archive, id)?;
            Ok(manifest.info(size))
        };
        match read() {
            Ok(info) => result.backups.push(info),
            Err(cause) => result.warnings.push(format!(
                "「{}」を一覧に読み込めません: {cause}",
                path.display()
            )),
        }
    }
    result.backups.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(result)
}

fn extract_archive(
    archive: &mut Archive,
    entries: &[Entry],
    stage: &Path,
) -> Result<(), CoreError> {
    fs::create_dir(stage)
        .map_err(|cause| context("復元用フォルダを作成できません", stage, cause))?;
    let mut total = 0u64;
    for entry in entries {
        let path = stage.join(&entry.relative);
        let mut input = archive
            .by_index(entry.index)
            .map_err(|cause| error(format!("ZIPの展開を開始できません: {cause}")))?;
        if entry.directory {
            fs::create_dir_all(&path)
                .map_err(|cause| context("展開先を作成できません", &path, cause))?;
            // Even directories are read to EOF so malformed streams/CRC fail.
            let count = io::copy(&mut (&mut input).take(1), &mut io::sink())
                .map_err(|cause| context("ZIPフォルダが壊れています", &path, cause))?;
            if count != 0 {
                return Err(context(
                    "ZIPフォルダにデータがあります",
                    &path,
                    "不正なZIPです",
                ));
            }
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|cause| context("展開先を作成できません", parent, cause))?;
        }
        let output = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|cause| context("展開ファイルを作成できません", &path, cause))?;
        let mut output = BufWriter::new(output);
        let copied =
            io::copy(&mut (&mut input).take(entry.size + 1), &mut output).map_err(|cause| {
                context(
                    "ZIPが壊れています（展開・チェックサム検証に失敗）",
                    &path,
                    cause,
                )
            })?;
        if copied != entry.size {
            return Err(context(
                "ZIPの展開サイズが一致しません",
                &path,
                "バックアップが壊れています",
            ));
        }
        total = total
            .checked_add(copied)
            .ok_or_else(|| error("ZIPのサイズが不正です。"))?;
        if total > MAX_EXPANDED_BYTES {
            return Err(error("ZIPの展開サイズ上限を超えています。"));
        }
        output
            .flush()
            .and_then(|()| output.get_ref().sync_all())
            .map_err(|cause| context("展開ファイルを保存できません", &path, cause))?;
        #[cfg(unix)]
        if let Some(mode) = input.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(mode & 0o777))
                .map_err(|cause| context("展開ファイルの権限を保存できません", &path, cause))?;
        }
    }
    sync_directory(stage)
}

fn read_options(path: &Path) -> Result<Option<String>, CoreError> {
    let Some(metadata) = optional_metadata(path)? else {
        return Ok(None);
    };
    if !metadata.is_file() || metadata.len() > MAX_OPTIONS_BYTES {
        return Err(context(
            "options.txtが不正または大きすぎます",
            path,
            "上限8 MiBの通常ファイルが必要です",
        ));
    }
    let file = File::open(path).map_err(|cause| context("設定を開けません", path, cause))?;
    let bytes = read_bounded(file, MAX_OPTIONS_BYTES, "options.txt")?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|cause| context("options.txtがUTF-8ではありません", path, cause))
}

fn safety_backup(
    backups_dir: &Path,
    root: &Path,
    target: &BackupProfile,
    scope: BackupScope,
) -> Result<Option<BackupInfo>, CoreError> {
    let has_data = match scope {
        BackupScope::Settings => match read_options(&root.join("options.txt"))? {
            Some(options) => crate::game_settings::has_importable_settings(&options)?,
            None => false,
        },
        BackupScope::Full => !included_top_entries(root)?.is_empty(),
    };
    if has_data {
        create_backup_internal(
            backups_dir,
            root,
            target,
            scope,
            true,
            scope == BackupScope::Full,
        )
        .map(Some)
    } else {
        Ok(None)
    }
}

fn read_marker(root: &Path) -> Result<Option<Vec<u8>>, CoreError> {
    let path = root.join(RESTORE_MARKER);
    let Some(metadata) = optional_metadata(&path)? else {
        return Ok(None);
    };
    if !metadata.is_file() || metadata.len() > MAX_MARKER_BYTES {
        return Err(context(
            "復元中断記録が不正です",
            &path,
            "復旧データを保管してから記録を確認してください",
        ));
    }
    let file =
        File::open(&path).map_err(|cause| context("復元中断記録を開けません", &path, cause))?;
    read_bounded(file, MAX_MARKER_BYTES, "復元中断記録").map(Some)
}

/// A full-restore marker is intentionally fail-closed, even if truncated.
pub fn ensure_restore_complete(game_dir: &Path) -> Result<(), CoreError> {
    let marker = game_dir.join(RESTORE_MARKER);
    match fs::symlink_metadata(&marker) {
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(context("復元中断記録を確認できません", &marker, cause)),
        Ok(_) => Err(context(
            "フル復元の中断記録が残っているため起動・設定取込・バックアップを停止しました",
            &marker,
            "この記録に復旧用フォルダと安全バックアップIDがあります。復旧データを削除せず、バックアップ画面からフル復元をやり直してください",
        )),
    }
}

fn cleanup(path: &Path, warnings: &mut Vec<String>) {
    if let Err(cause) = fs::remove_dir_all(path) {
        warnings.push(format!("処理は完了しましたが復旧用フォルダ「{}」を削除できません。内容を確認して手動で削除してください: {cause}", path.display()));
    }
}

fn restore_settings<F>(
    work: TempDir,
    root: &Path,
    backup: BackupInfo,
    safety_backup: Option<BackupInfo>,
    merged: String,
    original: Option<String>,
    on_restored: F,
) -> Result<RestoreResult, CoreError>
where
    F: FnOnce(&BackupInfo) -> Result<(), CoreError>,
{
    if let Some(original) = &original {
        atomic_write(
            &work.path().join("original-options.txt"),
            original.as_bytes(),
        )?;
    }
    let work = work.keep();
    let path = root.join("options.txt");
    let apply = atomic_write(&path, merged.as_bytes()).and_then(|()| on_restored(&backup));
    if let Err(cause) = apply {
        let rollback = match &original {
            Some(original) => atomic_write(&path, original.as_bytes()),
            None => match fs::remove_file(&path) {
                Ok(()) => sync_directory(root),
                Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(cause) => Err(context("設定を元に戻せません", &path, cause)),
            },
        };
        if let Err(rollback_error) = rollback {
            return Err(error(format!("設定の復元に失敗: {cause}。元に戻す処理も失敗: {rollback_error}。復旧データ「{}」と安全バックアップを保管してください。", work.display())));
        }
        let mut warnings = Vec::new();
        cleanup(&work, &mut warnings);
        return Err(error(format!(
            "設定の復元に失敗し、元の設定に戻しました: {cause}。{}",
            warnings.join(" ")
        )));
    }
    let mut warnings = Vec::new();
    cleanup(&work, &mut warnings);
    Ok(RestoreResult {
        backup,
        safety_backup,
        warnings,
    })
}

#[derive(Serialize)]
struct RestoreMarker<'a> {
    format_version: u32,
    backup_id: &'a str,
    safety_backup_id: Option<&'a str>,
    recovery_directory: String,
    previous_marker: Option<String>,
    original_entries: Vec<String>,
    replacement_entries: Vec<String>,
}

fn move_entry(from: &Path, to: &Path) -> Result<(), CoreError> {
    if optional_metadata(to)?.is_some() {
        return Err(context(
            "移動先が既に存在します",
            to,
            "既存データは上書きしません",
        ));
    }
    fs::rename(from, to).map_err(|cause| {
        context(
            "ファイルを移動できません",
            from,
            format!("{}: {cause}", to.display()),
        )
    })
}

fn restore_full<F>(
    work: TempDir,
    root: &Path,
    backup: BackupInfo,
    safety_backup: Option<BackupInfo>,
    previous: Option<Vec<u8>>,
    on_restored: F,
) -> Result<RestoreResult, CoreError>
where
    F: FnOnce(&BackupInfo) -> Result<(), CoreError>,
{
    let original = included_top_entries(root)?;
    let staged = children(&work.path().join("data"))?;
    let rollback = work.path().join("rollback");
    let failed = work.path().join("failed");
    fs::create_dir(&rollback)
        .and_then(|()| fs::create_dir(&failed))
        .map_err(|cause| context("復旧用フォルダを作成できません", work.path(), cause))?;
    let mut warnings = Vec::new();
    let previous_path = if let Some(bytes) = &previous {
        let mut history = Builder::new()
            .prefix(".train-launcher-previous-restore-")
            .suffix(".json")
            .tempfile_in(root)
            .map_err(|cause| context("前回の中断記録を保管できません", root, cause))?;
        history
            .write_all(bytes)
            .and_then(|()| history.as_file().sync_all())
            .map_err(|cause| context("前回の中断記録を保存できません", history.path(), cause))?;
        let (_, path) = history
            .keep()
            .map_err(|cause| error(format!("前回の中断記録を保管できません: {cause}")))?;
        warnings.push(format!("前回中断した復元の記録「{}」と、その記録が示す復旧フォルダは削除せず保持しました。必要なデータを確認してください。", path.display()));
        Some(path)
    } else {
        None
    };
    let marker = RestoreMarker {
        format_version: FORMAT_VERSION,
        backup_id: &backup.id,
        safety_backup_id: safety_backup.as_ref().map(|info| info.id.as_str()),
        recovery_directory: work.path().display().to_string(),
        previous_marker: previous_path
            .as_ref()
            .map(|path| path.display().to_string()),
        original_entries: original
            .iter()
            .map(|path| filename(path).map(str::to_owned))
            .collect::<Result<_, _>>()?,
        replacement_entries: staged
            .iter()
            .map(|path| filename(path).map(str::to_owned))
            .collect::<Result<_, _>>()?,
    };
    let bytes = serde_json::to_vec(&marker)
        .map_err(|cause| error(format!("復元中断記録を作成できません: {cause}")))?;
    if bytes.len() as u64 > MAX_MARKER_BYTES {
        return Err(error(
            "復元の最上位ファイル数が多すぎます。フォルダにまとめてください。",
        ));
    }
    let marker_path = root.join(RESTORE_MARKER);
    // From this point, a TempDir destructor must never erase the only originals.
    let work = work.keep();
    if let Err(cause) = atomic_write(&marker_path, &bytes) {
        return Err(error(format!("復元中断記録を保存できません（ゲームデータは未移動）: {cause}。作業フォルダ「{}」を保管して確認してください。", work.display())));
    }
    let mut moved = Vec::new();
    let mut installed = Vec::new();
    let apply = (|| -> Result<(), CoreError> {
        for path in &original {
            let name = filename(path)?.to_owned();
            move_entry(path, &rollback.join(&name))?;
            moved.push(name);
        }
        sync_directory(&rollback)?;
        for path in &staged {
            let name = filename(path)?.to_owned();
            move_entry(path, &root.join(&name))?;
            installed.push(name);
        }
        sync_directory(root)?;
        on_restored(&backup)
    })();
    if let Err(cause) = apply {
        let mut failures = Vec::new();
        for name in installed.iter().rev() {
            if let Err(cause) = move_entry(&root.join(name), &failed.join(name)) {
                failures.push(cause.to_string());
            }
        }
        for name in moved.iter().rev() {
            if let Err(cause) = move_entry(&rollback.join(name), &root.join(name)) {
                failures.push(cause.to_string());
            }
        }
        if failures.is_empty() {
            if let Err(cause) = sync_directory(root) {
                failures.push(cause.to_string());
            }
        }
        if failures.is_empty() {
            let reset = match &previous {
                Some(bytes) => atomic_write(&marker_path, bytes),
                None => fs::remove_file(&marker_path)
                    .map_err(|cause| context("復元中断記録を解除できません", &marker_path, cause))
                    .and_then(|()| sync_directory(root)),
            };
            if let Err(cause) = reset {
                failures.push(cause.to_string());
            }
        }
        if !failures.is_empty() {
            return Err(error(format!("フル復元に失敗: {cause}。元に戻す処理も完了できません: {}。復旧データ「{}」と安全バックアップを削除せず、フル復元をやり直してください。", failures.join(" / "), work.display())));
        }
        cleanup(&work, &mut warnings);
        return Err(error(format!(
            "フル復元に失敗し、元のゲームデータに戻しました: {cause}。{}",
            warnings.join(" ")
        )));
    }
    // The callback is the commit point for profile bookkeeping. Cleanup after
    // it is nonfatal; if marker removal fails, retain recovery and block launch.
    if let Err(cause) = fs::remove_file(&marker_path)
        .map_err(CoreError::from)
        .and_then(|()| sync_directory(root))
    {
        warnings.push(format!("ゲームデータとプロファイルは復元済みですが、中断記録「{}」を解除できません: {cause}。復旧フォルダ「{}」を保持しました。起動前に記録を確認し、必要ならフル復元をやり直してください。", marker_path.display(), work.display()));
    } else {
        cleanup(&work, &mut warnings);
    }
    Ok(RestoreResult {
        backup,
        safety_backup,
        warnings,
    })
}

/// Verifies/extracts first, snapshots the current state, then installs files.
/// The callback must atomically persist ONLY full-scope game bookkeeping.
/// A callback error rolls files back; a successful callback is the commit point.
pub fn restore_backup<F>(
    backups_dir: &Path,
    backup_id: &str,
    game_dir: &Path,
    target: &BackupProfile,
    on_restored: F,
) -> Result<RestoreResult, CoreError>
where
    F: FnOnce(&BackupInfo) -> Result<(), CoreError>,
{
    valid_id(backup_id)?;
    validate_profile(target)?;
    let (mut archive, size) = open_archive(&backups_dir.join(format!("{backup_id}.zip")))?;
    let (manifest, entries) = inspect_archive(&mut archive, backup_id)?;
    if manifest.profile.minecraft_version != target.minecraft_version {
        return Err(error(format!("Minecraftのバージョンが一致しません（バックアップ: {} / 復元先: {}）。同じバージョンを選んでください。", manifest.profile.minecraft_version, target.minecraft_version)));
    }
    if manifest.scope == BackupScope::Full
        && normalized_loader(manifest.profile.mod_loader.as_deref())
            != normalized_loader(target.mod_loader.as_deref())
    {
        return Err(error(
            "フル復元には同じMODローダーが必要です。復元先のローダーを確認してください。",
        ));
    }
    let root = game_root(game_dir, backups_dir)?;
    if manifest.scope == BackupScope::Settings {
        ensure_restore_complete(&root)?;
    }
    let previous = if manifest.scope == BackupScope::Full {
        read_marker(&root)?
    } else {
        None
    };
    fs::create_dir_all(&root)
        .map_err(|cause| context("ゲームフォルダを作成できません", &root, cause))?;
    let work = Builder::new()
        .prefix(".train-launcher-restore-")
        .tempdir_in(&root)
        .map_err(|cause| {
            context(
                "同じファイルシステムに復元用フォルダを作成できません",
                &root,
                cause,
            )
        })?;
    extract_archive(&mut archive, &entries, &work.path().join("data"))?;
    let backup = manifest.info(size);
    match manifest.scope {
        BackupScope::Settings => {
            let source = read_options(&work.path().join("data").join("options.txt"))?
                .ok_or_else(|| error("バックアップにoptions.txtがありません。"))?;
            let original = read_options(&root.join("options.txt"))?;
            let merged =
                crate::game_settings::merge_options(&source, original.as_deref().unwrap_or(""))?;
            let safety = safety_backup(backups_dir, &root, target, manifest.scope)?;
            restore_settings(work, &root, backup, safety, merged, original, on_restored)
        }
        BackupScope::Full => {
            let safety = safety_backup(backups_dir, &root, target, manifest.scope)?;
            restore_full(work, &root, backup, safety, previous, on_restored)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const SOURCE_OPTIONS: &str = "version:3700\nmusic:0.25\nkey_key.forward:key.keyboard.i\nresourcePacks:[\"vanilla\",\"file/source.zip\"]\nincompatibleResourcePacks:[\"file/source.zip\"]\nlastServer:source.invalid\n";
    const TARGET_OPTIONS: &str = "version:3700\nmusic:1.0\nkey_key.forward:key.keyboard.w\nresourcePacks:[\"vanilla\",\"file/target.zip\"]\nincompatibleResourcePacks:[]\nlastServer:target.invalid\n";

    struct Fixture {
        _directory: TempDir,
        source: PathBuf,
        target: PathBuf,
        backups: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = Builder::new()
                .prefix("backup-test-")
                .tempdir_in(std::env::current_dir().unwrap())
                .unwrap();
            let source = directory.path().join("source");
            let target = directory.path().join("target");
            let backups = directory.path().join("archives");
            fs::create_dir(&source).unwrap();
            fs::create_dir(&target).unwrap();
            fs::create_dir(&backups).unwrap();
            Self {
                _directory: directory,
                source,
                target,
                backups,
            }
        }

        fn backup(&self, scope: BackupScope) -> BackupInfo {
            create_backup(
                &self.backups,
                &self.source,
                &profile("source"),
                scope,
                false,
            )
            .unwrap()
        }

        fn restore(&self, info: &BackupInfo) -> Result<RestoreResult, CoreError> {
            restore_backup(
                &self.backups,
                &info.id,
                &self.target,
                &profile("target"),
                |_| Ok(()),
            )
        }

        fn archive(&self, info: &BackupInfo) -> PathBuf {
            self.backups.join(format!("{}.zip", info.id))
        }
    }

    fn profile(id: &str) -> BackupProfile {
        BackupProfile {
            id: id.into(),
            name: format!("Profile {id}"),
            minecraft_version: "1.20.4".into(),
            mod_loader: None,
            managed_mod_filenames: vec!["managed.jar".into()],
            managed_resource_pack_filenames: vec!["pack.zip".into()],
            enabled_resource_packs: vec!["vanilla".into(), "file/pack.zip".into()],
            last_server_address: Some("server.invalid".into()),
        }
    }

    fn put(root: &Path, relative: &str, bytes: impl AsRef<[u8]>) {
        let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn bytes(root: &Path, relative: &str) -> Vec<u8> {
        fs::read(root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR))).unwrap()
    }

    fn archive_contents(path: &Path) -> HashMap<String, Vec<u8>> {
        let (mut archive, _) = open_archive(path).unwrap();
        let mut entries = HashMap::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let mut data = Vec::new();
            file.read_to_end(&mut data).unwrap();
            entries.insert(file.name().to_owned(), data);
        }
        entries
    }

    fn manifest(id: &str, scope: BackupScope) -> serde_json::Value {
        serde_json::to_value(Manifest {
            format_version: FORMAT_VERSION,
            id: id.into(),
            profile: profile("source"),
            scope,
            created_at: "2026-09-12T12:00:00Z".into(),
            automatic: false,
        })
        .unwrap()
    }

    fn custom_zip(root: &Path, id: &str, metadata: &[u8], entries: &[(&str, &[u8])]) -> PathBuf {
        let path = root.join(format!("{id}.zip"));
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file("manifest.json", options).unwrap();
        writer.write_all(metadata).unwrap();
        for (name, contents) in entries {
            if name.ends_with('/') {
                writer.add_directory(*name, options).unwrap();
            } else {
                writer.start_file(*name, options).unwrap();
                writer.write_all(contents).unwrap();
            }
        }
        writer.finish().unwrap();
        path
    }

    #[test]
    fn settings_round_trip_preserves_target_packs_server_and_unrelated_files() {
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        put(&fixture.source, "saves/source.dat", b"source world");
        put(&fixture.target, "options.txt", TARGET_OPTIONS);
        put(&fixture.target, "saves/current.dat", b"current world");
        put(&fixture.target, "mods/local.jar", b"local mod");
        let backup = fixture.backup(BackupScope::Settings);
        let original_archive = fs::read(fixture.archive(&backup)).unwrap();
        let entries = archive_contents(&fixture.archive(&backup));
        assert_eq!(entries.len(), 3);
        assert_eq!(entries["data/options.txt"], SOURCE_OPTIONS.as_bytes());

        let result = fixture.restore(&backup).unwrap();
        let restored = String::from_utf8(bytes(&fixture.target, "options.txt")).unwrap();
        assert!(restored.contains("music:0.25"));
        assert!(restored.contains("key_key.forward:key.keyboard.i"));
        assert!(restored.contains("resourcePacks:[\"vanilla\",\"file/target.zip\"]"));
        assert!(restored.contains("incompatibleResourcePacks:[]"));
        assert!(restored.contains("lastServer:target.invalid"));
        assert!(!restored.contains("source.zip"));
        assert_eq!(
            bytes(&fixture.target, "saves/current.dat"),
            b"current world"
        );
        assert_eq!(bytes(&fixture.target, "mods/local.jar"), b"local mod");
        assert!(!fixture.target.join("saves").join("source.dat").exists());
        assert_eq!(
            bytes(&fixture.source, "options.txt"),
            SOURCE_OPTIONS.as_bytes()
        );
        assert_eq!(
            fs::read(fixture.archive(&backup)).unwrap(),
            original_archive
        );
        assert!(result.warnings.is_empty());
        let safety = result.safety_backup.unwrap();
        assert!(safety.automatic);
        assert_eq!(safety.profile.id, "target");
        assert_eq!(safety.scope, BackupScope::Settings);
        assert_eq!(
            archive_contents(&fixture.archive(&safety))["data/options.txt"],
            TARGET_OPTIONS.as_bytes()
        );
    }

    #[test]
    fn settings_restore_accepts_destinations_without_client_preferences() {
        let protected = "version:3700\nresourcePacks:[\"file/target.zip\"]\nincompatibleResourcePacks:[]\nlastServer:target.invalid\n";
        for original in [None, Some(""), Some("\u{feff}\n"), Some(protected)] {
            let fixture = Fixture::new();
            put(&fixture.source, "options.txt", SOURCE_OPTIONS);
            if let Some(original) = original {
                put(&fixture.target, "options.txt", original);
            }
            put(&fixture.target, "saves/current.dat", b"current world");
            let backup = fixture.backup(BackupScope::Settings);
            let result = fixture.restore(&backup).unwrap();
            assert!(result.safety_backup.is_none());
            let restored = String::from_utf8(bytes(&fixture.target, "options.txt")).unwrap();
            assert!(restored.contains("music:0.25\n"));
            assert!(restored.contains("key_key.forward:key.keyboard.i\n"));
            if original == Some(protected) {
                for line in protected.lines() {
                    assert!(restored.lines().any(|restored_line| restored_line == line));
                }
            }
            assert_eq!(
                bytes(&fixture.target, "saves/current.dat"),
                b"current world"
            );
            assert_eq!(list_backups(&fixture.backups).unwrap().backups.len(), 1);
        }
    }

    #[test]
    fn full_round_trip_replaces_only_included_entries_and_preserves_shared_data() {
        let fixture = Fixture::new();
        let included = [
            "options.txt",
            "saves/world/level.dat",
            "mods/managed.jar",
            "resourcepacks/pack.zip",
            "shaderpacks/shader.zip",
            "config/mod.toml",
            "arbitrary-mod-data/nested/value.bin",
        ];
        let preserved = [
            "assets/objects/object",
            "libraries/library.jar",
            "versions/version.json",
            "runtime/bin/java",
            "logs/latest.log",
            "crash-reports/report.txt",
            "train-launcher-process.log",
            "launcher_accounts.json",
            "launcher_msa_credentials.bin",
            "launcher_profiles.json",
            "launcher_preferences.json",
            "webcache/Cookies",
            "webcache2/Local Storage/token",
            "backups/previous.zip",
            ".train-launcher-abandoned/original.dat",
        ];
        for name in included {
            put(&fixture.source, name, format!("source:{name}"));
        }
        for name in preserved {
            put(&fixture.source, name, "source excluded");
            put(&fixture.target, name, format!("target:{name}"));
        }
        fs::create_dir(fixture.source.join("empty-folder")).unwrap();
        put(&fixture.target, "options.txt", TARGET_OPTIONS);
        put(&fixture.target, "saves/old-world/level.dat", "old world");
        put(&fixture.target, "current-extra.txt", "extra");
        let backup = fixture.backup(BackupScope::Full);
        let original_archive = fs::read(fixture.archive(&backup)).unwrap();
        let entries = archive_contents(&fixture.archive(&backup));
        for name in included {
            assert!(entries.contains_key(&format!("data/{name}")));
        }
        for name in preserved {
            assert!(!entries.contains_key(&format!("data/{name}")));
        }
        let callback_called = Cell::new(false);
        let result = restore_backup(
            &fixture.backups,
            &backup.id,
            &fixture.target,
            &profile("other-profile"),
            |info| {
                callback_called.set(true);
                assert_eq!(info.profile.id, "source");
                assert!(fixture.target.join(RESTORE_MARKER).is_file());
                assert_eq!(bytes(&fixture.target, "options.txt"), b"source:options.txt");
                Ok(())
            },
        )
        .unwrap();
        assert!(callback_called.get());
        for name in included {
            assert_eq!(
                bytes(&fixture.target, name),
                format!("source:{name}").as_bytes()
            );
            assert_eq!(
                bytes(&fixture.source, name),
                format!("source:{name}").as_bytes()
            );
        }
        for name in preserved {
            assert_eq!(
                bytes(&fixture.target, name),
                format!("target:{name}").as_bytes()
            );
            assert_eq!(bytes(&fixture.source, name), b"source excluded");
        }
        assert!(fixture.target.join("empty-folder").is_dir());
        assert!(!fixture.target.join("current-extra.txt").exists());
        assert!(!fixture.target.join("saves").join("old-world").exists());
        ensure_restore_complete(&fixture.target).unwrap();
        assert_eq!(
            fs::read(fixture.archive(&backup)).unwrap(),
            original_archive
        );
        let safety = result.safety_backup.unwrap();
        assert!(safety.automatic);
        assert_eq!(safety.profile.id, "other-profile");
        let safety_entries = archive_contents(&fixture.archive(&safety));
        assert_eq!(safety_entries["data/current-extra.txt"], b"extra");
        assert!(!safety_entries.contains_key("data/launcher_accounts.json"));
    }

    #[test]
    fn settings_require_launched_nonempty_options_and_safe_file_type() {
        let fixture = Fixture::new();
        for contents in [None, Some("")] {
            if let Some(contents) = contents {
                put(&fixture.source, "options.txt", contents);
            }
            let result = create_backup(
                &fixture.backups,
                &fixture.source,
                &profile("source"),
                BackupScope::Settings,
                false,
            );
            assert!(result.unwrap_err().to_string().contains("起動"));
        }
        fs::remove_file(fixture.source.join("options.txt")).unwrap();
        fs::create_dir(fixture.source.join("options.txt")).unwrap();
        assert!(create_backup(
            &fixture.backups,
            &fixture.source,
            &profile("source"),
            BackupScope::Settings,
            false
        )
        .is_err());
        assert!(list_backups(&fixture.backups).unwrap().backups.is_empty());
    }

    #[test]
    fn restore_into_empty_or_missing_target_needs_no_safety_snapshot() {
        for scope in [BackupScope::Settings, BackupScope::Full] {
            let fixture = Fixture::new();
            put(&fixture.source, "options.txt", SOURCE_OPTIONS);
            let backup = fixture.backup(scope);
            fs::remove_dir(&fixture.target).unwrap();
            let result = fixture.restore(&backup).unwrap();
            assert!(result.safety_backup.is_none());
            assert!(fixture.target.join("options.txt").is_file());
            assert_eq!(list_backups(&fixture.backups).unwrap().backups.len(), 1);
        }
    }

    #[test]
    fn empty_full_snapshot_can_clear_game_data_without_removing_shared_entries() {
        let fixture = Fixture::new();
        put(&fixture.source, "launcher_accounts.json", "do not back up");
        let backup = fixture.backup(BackupScope::Full);
        put(&fixture.target, "options.txt", TARGET_OPTIONS);
        put(&fixture.target, "assets/object", "shared");
        let result = fixture.restore(&backup).unwrap();
        assert!(result.safety_backup.is_some());
        assert!(!fixture.target.join("options.txt").exists());
        assert_eq!(bytes(&fixture.target, "assets/object"), b"shared");
        assert!(fixture.target.is_dir());
    }

    #[test]
    fn versions_and_loaders_are_verified_before_changes_or_safety_snapshot() {
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        put(&fixture.target, "options.txt", TARGET_OPTIONS);
        let backup = fixture.backup(BackupScope::Full);
        let called = Cell::new(false);
        for (version, loader) in [("1.21", None), ("1.20.4", Some("fabric"))] {
            let mut target = profile("target");
            target.minecraft_version = version.into();
            target.mod_loader = loader.map(str::to_owned);
            assert!(restore_backup(
                &fixture.backups,
                &backup.id,
                &fixture.target,
                &target,
                |_| {
                    called.set(true);
                    Ok(())
                }
            )
            .is_err());
            assert_eq!(
                bytes(&fixture.target, "options.txt"),
                TARGET_OPTIONS.as_bytes()
            );
        }
        assert!(!called.get());
        assert_eq!(list_backups(&fixture.backups).unwrap().backups.len(), 1);
        assert!(!fixture.target.join(RESTORE_MARKER).exists());

        let mut target = profile("target");
        target.mod_loader = Some(" Vanilla ".into());
        restore_backup(
            &fixture.backups,
            &backup.id,
            &fixture.target,
            &target,
            |_| Ok(()),
        )
        .unwrap();
        let settings = fixture.backup(BackupScope::Settings);
        target.mod_loader = Some("fabric".into());
        restore_backup(
            &fixture.backups,
            &settings.id,
            &fixture.target,
            &target,
            |_| Ok(()),
        )
        .unwrap();
    }

    #[test]
    fn settings_options_data_version_is_verified_before_snapshot_or_mutation() {
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        put(&fixture.target, "options.txt", "version:9999\nmusic:1\n");
        let backup = fixture.backup(BackupScope::Settings);
        assert!(fixture.restore(&backup).is_err());
        assert_eq!(
            bytes(&fixture.target, "options.txt"),
            b"version:9999\nmusic:1\n"
        );
        assert_eq!(list_backups(&fixture.backups).unwrap().backups.len(), 1);
    }

    #[test]
    fn callback_failure_rolls_back_full_files_and_retains_safety_snapshot() {
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        put(&fixture.source, "mods/new.jar", "new");
        put(&fixture.target, "options.txt", TARGET_OPTIONS);
        put(&fixture.target, "saves/original/level.dat", "original");
        put(&fixture.target, "launcher_accounts.json", "credentials");
        let backup = fixture.backup(BackupScope::Full);
        let failure = restore_backup(
            &fixture.backups,
            &backup.id,
            &fixture.target,
            &profile("target"),
            |_| {
                assert!(fixture.target.join("mods").join("new.jar").is_file());
                Err(error("profile save failed"))
            },
        )
        .unwrap_err();
        assert!(failure.to_string().contains("元のゲームデータに戻しました"));
        assert_eq!(
            bytes(&fixture.target, "options.txt"),
            TARGET_OPTIONS.as_bytes()
        );
        assert_eq!(
            bytes(&fixture.target, "saves/original/level.dat"),
            b"original"
        );
        assert_eq!(
            bytes(&fixture.target, "launcher_accounts.json"),
            b"credentials"
        );
        assert!(!fixture.target.join("mods").exists());
        ensure_restore_complete(&fixture.target).unwrap();
        let backups = list_backups(&fixture.backups).unwrap().backups;
        assert_eq!(backups.len(), 2);
        assert_eq!(backups.iter().filter(|backup| backup.automatic).count(), 1);
        assert!(!children(&fixture.target)
            .unwrap()
            .iter()
            .any(|path| filename(path)
                .unwrap()
                .starts_with(".train-launcher-restore-")));
    }

    #[test]
    fn callback_failure_rolls_back_settings_with_or_without_an_original() {
        for existing in [true, false] {
            let fixture = Fixture::new();
            put(&fixture.source, "options.txt", SOURCE_OPTIONS);
            if existing {
                put(&fixture.target, "options.txt", TARGET_OPTIONS);
            }
            let backup = fixture.backup(BackupScope::Settings);
            assert!(restore_backup(
                &fixture.backups,
                &backup.id,
                &fixture.target,
                &profile("target"),
                |_| Err(error("callback failed"))
            )
            .is_err());
            if existing {
                assert_eq!(
                    bytes(&fixture.target, "options.txt"),
                    TARGET_OPTIONS.as_bytes()
                );
            } else {
                assert!(!fixture.target.join("options.txt").exists());
            }
            assert_eq!(
                list_backups(&fixture.backups).unwrap().backups.len(),
                if existing { 2 } else { 1 }
            );
        }
    }

    #[test]
    fn failed_rollback_preserves_originals_and_blocks_launch_until_explicit_full_restore() {
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        put(&fixture.target, "original.txt", "only original copy");
        let backup = fixture.backup(BackupScope::Full);
        let failure = restore_backup(
            &fixture.backups,
            &backup.id,
            &fixture.target,
            &profile("target"),
            |_| {
                put(
                    &fixture.target,
                    "original.txt",
                    "conflicting newly created file",
                );
                Err(error("simulate failed metadata save and concurrent file"))
            },
        )
        .unwrap_err();
        assert!(failure.to_string().contains("元に戻す処理も完了できません"));
        assert!(ensure_restore_complete(&fixture.target).is_err());
        let marker: serde_json::Value =
            serde_json::from_slice(&bytes(&fixture.target, RESTORE_MARKER)).unwrap();
        let recovery = PathBuf::from(marker["recovery_directory"].as_str().unwrap());
        assert_eq!(
            bytes(&recovery, "rollback/original.txt"),
            b"only original copy"
        );
        let safety_id = marker["safety_backup_id"].as_str().unwrap();
        assert!(fixture.backups.join(format!("{safety_id}.zip")).is_file());
        let result = fixture.restore(&backup).unwrap();
        assert!(!result.warnings.is_empty());
        ensure_restore_complete(&fixture.target).unwrap();
        assert_eq!(
            bytes(&recovery, "rollback/original.txt"),
            b"only original copy"
        );
    }

    #[test]
    fn interrupted_marker_blocks_normal_operations_and_survives_a_failed_retry() {
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        put(&fixture.target, "options.txt", TARGET_OPTIONS);
        let full = fixture.backup(BackupScope::Full);
        let settings = fixture.backup(BackupScope::Settings);
        let previous =
            br#"{"recovery_directory":"keep-original-data","safety_backup_id":"old_backup"}"#;
        put(&fixture.target, RESTORE_MARKER, previous);
        put(
            &fixture.target,
            ".train-launcher-abandoned/rollback/value",
            "original recovery",
        );
        assert!(ensure_restore_complete(&fixture.target).is_err());
        assert!(fixture.restore(&settings).is_err());
        for scope in [BackupScope::Settings, BackupScope::Full] {
            assert!(create_backup(
                &fixture.backups,
                &fixture.target,
                &profile("target"),
                scope,
                false
            )
            .is_err());
        }
        assert!(restore_backup(
            &fixture.backups,
            &full.id,
            &fixture.target,
            &profile("target"),
            |_| Err(error("save failed"))
        )
        .is_err());
        assert_eq!(bytes(&fixture.target, RESTORE_MARKER), previous);
        assert_eq!(
            bytes(&fixture.target, "options.txt"),
            TARGET_OPTIONS.as_bytes()
        );
        let result = fixture.restore(&full).unwrap();
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("前回中断")));
        ensure_restore_complete(&fixture.target).unwrap();
        assert_eq!(
            bytes(&fixture.target, ".train-launcher-abandoned/rollback/value"),
            b"original recovery"
        );
        assert!(children(&fixture.target).unwrap().iter().any(|path| {
            filename(path)
                .unwrap()
                .starts_with(".train-launcher-previous-restore-")
                && fs::read(path).unwrap() == previous
        }));
    }

    #[test]
    fn truncated_marker_is_fail_closed_but_explicit_full_restore_can_recover() {
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        let backup = fixture.backup(BackupScope::Full);
        put(&fixture.target, RESTORE_MARKER, b"{");
        assert!(ensure_restore_complete(&fixture.target).is_err());
        let result = fixture.restore(&backup).unwrap();
        assert!(result.safety_backup.is_none());
        assert!(!result.warnings.is_empty());
        ensure_restore_complete(&fixture.target).unwrap();
    }

    #[test]
    fn backup_ids_cannot_be_paths_and_generated_ids_are_unique() {
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        for id in [
            "",
            "../outside",
            "..\\outside",
            "C:\\outside",
            "/absolute",
            "name.zip",
            "name:stream",
            "a/b",
            ".",
        ] {
            assert!(
                restore_backup(
                    &fixture.backups,
                    id,
                    &fixture.target,
                    &profile("target"),
                    |_| Ok(())
                )
                .is_err(),
                "{id}"
            );
        }
        assert!(valid_id(&"x".repeat(97)).is_err());
        let first = fixture.backup(BackupScope::Settings);
        let second = fixture.backup(BackupScope::Settings);
        valid_id(&first.id).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(list_backups(&fixture.backups).unwrap().backups.len(), 2);
    }

    #[test]
    fn malformed_and_unsupported_archives_warn_without_hiding_valid_backups() {
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        let good = fixture.backup(BackupScope::Settings);
        custom_zip(&fixture.backups, "bad_json", b"{", &[]);
        let mut unsupported = manifest("unsupported", BackupScope::Full);
        unsupported["format_version"] = 99.into();
        custom_zip(
            &fixture.backups,
            "unsupported",
            &serde_json::to_vec(&unsupported).unwrap(),
            &[],
        );
        custom_zip(
            &fixture.backups,
            "wrong_id",
            &serde_json::to_vec(&manifest("other_id", BackupScope::Full)).unwrap(),
            &[],
        );
        put(&fixture.backups, "corrupt.zip", "not a zip");
        put(&fixture.backups, "ignored.pending", "partial zip");
        put(
            &fixture.backups,
            ".train-launcher-write-ignored",
            "partial zip",
        );
        let result = list_backups(&fixture.backups).unwrap();
        assert_eq!(result.backups.len(), 1);
        assert_eq!(result.backups[0].id, good.id);
        assert_eq!(result.warnings.len(), 4);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("未対応")));
        for id in ["bad_json", "unsupported", "wrong_id", "corrupt"] {
            assert!(restore_backup(
                &fixture.backups,
                id,
                &fixture.target,
                &profile("target"),
                |_| Ok(())
            )
            .is_err());
        }
        assert!(children(&fixture.target).unwrap().is_empty());
    }

    #[test]
    fn corrupt_payload_is_fully_checked_before_target_mutation_or_safety_snapshot() {
        let fixture = Fixture::new();
        let payload = b"unique_payload_for_crc_corruption";
        let path = custom_zip(
            &fixture.backups,
            "bad_crc",
            &serde_json::to_vec(&manifest("bad_crc", BackupScope::Full)).unwrap(),
            &[("data/world.dat", payload)],
        );
        let mut archive = fs::read(&path).unwrap();
        let position = archive
            .windows(payload.len())
            .position(|bytes| bytes == payload)
            .unwrap();
        archive[position] ^= 1;
        fs::write(&path, &archive).unwrap();
        put(&fixture.target, "options.txt", TARGET_OPTIONS);
        let called = Cell::new(false);
        assert!(restore_backup(
            &fixture.backups,
            "bad_crc",
            &fixture.target,
            &profile("target"),
            |_| {
                called.set(true);
                Ok(())
            }
        )
        .is_err());
        assert!(!called.get());
        assert_eq!(
            bytes(&fixture.target, "options.txt"),
            TARGET_OPTIONS.as_bytes()
        );
        assert!(!fixture.target.join(RESTORE_MARKER).exists());
        assert_eq!(children(&fixture.target).unwrap().len(), 1);
        assert_eq!(children(&fixture.backups).unwrap().len(), 1);
        assert_eq!(fs::read(&path).unwrap(), archive);
        archive.truncate(archive.len() - 10);
        fs::write(&path, archive).unwrap();
        assert!(restore_backup(
            &fixture.backups,
            "bad_crc",
            &fixture.target,
            &profile("target"),
            |_| Ok(())
        )
        .is_err());
    }

    #[test]
    fn traversal_excluded_names_and_windows_aliases_are_rejected() {
        let fixture = Fixture::new();
        put(&fixture.target, "options.txt", TARGET_OPTIONS);
        let metadata = serde_json::to_vec(&manifest("unsafe", BackupScope::Full)).unwrap();
        for name in [
            "../outside",
            "/absolute",
            "C:/absolute",
            "data/../outside",
            "data/./file",
            "data/sub\\file",
            "data/file:stream",
            "data//file",
            "data/NUL.txt",
            "data/trailing.",
            "data/trailing ",
            "data/.git/config",
            "data/assets/objects/asset",
            "data/libraries/shared.jar",
            "data/versions/shared.json",
            "data/runtime/java",
            "data/logs/log",
            "data/crash-reports/report",
            "data/launcher_accounts.json",
            "data/webcache/Cookies",
            "data/webcache2/Local Storage/token",
            "data/LAUNCHER_Profiles.json",
            "data/backups/archive.zip",
            "data/.train-launcher-restore.json",
            "data/train-launcher-process.log",
        ] {
            custom_zip(&fixture.backups, "unsafe", &metadata, &[(name, b"unsafe")]);
            assert!(
                restore_backup(
                    &fixture.backups,
                    "unsafe",
                    &fixture.target,
                    &profile("target"),
                    |_| Ok(())
                )
                .is_err(),
                "{name}"
            );
            assert_eq!(
                bytes(&fixture.target, "options.txt"),
                TARGET_OPTIONS.as_bytes()
            );
        }
        for entries in [
            vec![
                ("data/Name.txt", b"a".as_slice()),
                ("data/name.txt", b"b".as_slice()),
            ],
            vec![
                ("data/parent", b"a".as_slice()),
                ("data/parent/child", b"b".as_slice()),
            ],
            vec![
                ("data/Parent/a", b"a".as_slice()),
                ("data/parent/b", b"b".as_slice()),
            ],
        ] {
            custom_zip(&fixture.backups, "unsafe", &metadata, &entries);
            assert!(restore_backup(
                &fixture.backups,
                "unsafe",
                &fixture.target,
                &profile("target"),
                |_| Ok(())
            )
            .is_err());
        }
        assert_eq!(
            bytes(&fixture.target, "options.txt"),
            TARGET_OPTIONS.as_bytes()
        );
    }

    #[test]
    fn archive_symlinks_are_rejected_without_writing_the_link_target() {
        let fixture = Fixture::new();
        let path = fixture.backups.join("link.zip");
        let mut writer = ZipWriter::new(File::create(path).unwrap());
        writer
            .start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&serde_json::to_vec(&manifest("link", BackupScope::Full)).unwrap())
            .unwrap();
        writer
            .add_symlink("data/link", "../../outside", SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();
        assert!(restore_backup(
            &fixture.backups,
            "link",
            &fixture.target,
            &profile("target"),
            |_| Ok(())
        )
        .unwrap_err()
        .to_string()
        .contains("リンク"));
        assert!(children(&fixture.target).unwrap().is_empty());
    }

    #[test]
    fn managed_filenames_are_validated_on_create_list_and_restore() {
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        for name in [
            "../credentials",
            "sub/file.jar",
            "sub\\file.jar",
            "C:stream",
            "NUL",
            "",
            "..",
        ] {
            let mut source = profile("source");
            source.managed_mod_filenames = vec![name.into()];
            assert!(create_backup(
                &fixture.backups,
                &fixture.source,
                &source,
                BackupScope::Settings,
                false
            )
            .is_err());
            let mut metadata = manifest("tampered", BackupScope::Full);
            metadata["profile"]["managed_resource_pack_filenames"] = serde_json::json!([name]);
            custom_zip(
                &fixture.backups,
                "tampered",
                &serde_json::to_vec(&metadata).unwrap(),
                &[],
            );
            assert!(restore_backup(
                &fixture.backups,
                "tampered",
                &fixture.target,
                &profile("target"),
                |_| Ok(())
            )
            .is_err());
            assert_eq!(list_backups(&fixture.backups).unwrap().warnings.len(), 1);
        }
    }

    #[test]
    fn optional_bookkeeping_defaults_support_older_metadata() {
        let fixture = Fixture::new();
        let mut metadata = manifest("defaults", BackupScope::Full);
        let object = metadata["profile"].as_object_mut().unwrap();
        for name in [
            "managed_mod_filenames",
            "managed_resource_pack_filenames",
            "enabled_resource_packs",
            "last_server_address",
        ] {
            object.remove(name);
        }
        custom_zip(
            &fixture.backups,
            "defaults",
            &serde_json::to_vec(&metadata).unwrap(),
            &[],
        );
        let result = restore_backup(
            &fixture.backups,
            "defaults",
            &fixture.target,
            &profile("target"),
            |_| Ok(()),
        )
        .unwrap();
        assert!(result.backup.profile.managed_mod_filenames.is_empty());
        assert!(result
            .backup
            .profile
            .managed_resource_pack_filenames
            .is_empty());
        assert!(result.backup.profile.enabled_resource_packs.is_empty());
        assert!(result.backup.profile.last_server_address.is_none());
    }

    #[test]
    fn settings_archive_cannot_smuggle_world_data_or_empty_options() {
        let fixture = Fixture::new();
        let metadata = serde_json::to_vec(&manifest("settings", BackupScope::Settings)).unwrap();
        for entries in [
            vec![("data/options.txt", b"".as_slice())],
            vec![
                ("data/options.txt", SOURCE_OPTIONS.as_bytes()),
                ("data/world.dat", b"world".as_slice()),
            ],
            vec![("data/OPTIONS.TXT", SOURCE_OPTIONS.as_bytes())],
        ] {
            custom_zip(&fixture.backups, "settings", &metadata, &entries);
            assert!(restore_backup(
                &fixture.backups,
                "settings",
                &fixture.target,
                &profile("target"),
                |_| Ok(())
            )
            .is_err());
        }
        assert!(children(&fixture.target).unwrap().is_empty());
    }

    #[test]
    fn broad_game_roots_git_and_backup_recursion_are_rejected() {
        let fixture = Fixture::new();
        let cwd = std::env::current_dir().unwrap();
        for path in [cwd.as_path(), cwd.parent().unwrap()] {
            assert!(create_backup(
                &fixture.backups,
                path,
                &profile("source"),
                BackupScope::Full,
                false
            )
            .is_err());
        }
        put(&fixture.source, ".git/config", "git config");
        assert!(create_backup(
            &fixture.backups,
            &fixture.source,
            &profile("source"),
            BackupScope::Full,
            false
        )
        .is_err());
        fs::remove_dir_all(fixture.source.join(".git")).unwrap();
        assert!(create_backup(
            &fixture.source.join("nested-archives"),
            &fixture.source,
            &profile("source"),
            BackupScope::Full,
            false
        )
        .is_err());
        put(&fixture.source, "nested/.git/config", "nested repository");
        assert!(create_backup(
            &fixture.backups,
            &fixture.source,
            &profile("source"),
            BackupScope::Full,
            false
        )
        .is_err());
    }

    #[test]
    fn manifest_and_path_limits_are_enforced() {
        let fixture = Fixture::new();
        let oversized = vec![b' '; MAX_MANIFEST_BYTES as usize + 1];
        custom_zip(&fixture.backups, "oversized", &oversized, &[]);
        assert!(restore_backup(
            &fixture.backups,
            "oversized",
            &fixture.target,
            &profile("target"),
            |_| Ok(())
        )
        .is_err());
        let too_deep = format!("data/{}file", "nested/".repeat(MAX_DEPTH + 1));
        assert!(archive_path(&too_deep, false).is_err());
        assert!(archive_path(&format!("data/{}", "x".repeat(MAX_PATH_BYTES)), false).is_err());
        let mut metadata = manifest("bad_date", BackupScope::Full);
        metadata["created_at"] = "yesterday".into();
        custom_zip(
            &fixture.backups,
            "bad_date",
            &serde_json::to_vec(&metadata).unwrap(),
            &[],
        );
        assert!(restore_backup(
            &fixture.backups,
            "bad_date",
            &fixture.target,
            &profile("target"),
            |_| Ok(())
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn explicit_root_symlink_is_allowed_but_nested_and_archive_symlinks_are_not() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        let chosen_root = fixture._directory.path().join("chosen-root");
        symlink(&fixture.source, &chosen_root).unwrap();
        let backup = create_backup(
            &fixture.backups,
            &chosen_root,
            &profile("source"),
            BackupScope::Full,
            false,
        )
        .unwrap();
        put(&fixture.source, "nested/ordinary", "safe");
        symlink(
            fixture.target.clone(),
            fixture.source.join("nested").join("link"),
        )
        .unwrap();
        let failure = create_backup(
            &fixture.backups,
            &chosen_root,
            &profile("source"),
            BackupScope::Full,
            false,
        )
        .unwrap_err();
        assert!(failure.to_string().contains("link"));
        symlink(&fixture.source, fixture.target.join("link")).unwrap();
        assert!(fixture.restore(&backup).is_err());
        fs::remove_file(fixture.target.join("link")).unwrap();
        let aliased_archive = fixture.backups.join("archive_link.zip");
        symlink(fixture.archive(&backup), &aliased_archive).unwrap();
        assert!(list_backups(&fixture.backups)
            .unwrap()
            .warnings
            .iter()
            .any(|warning| warning.contains("archive_link")));
        assert!(restore_backup(
            &fixture.backups,
            "archive_link",
            &fixture.target,
            &profile("target"),
            |_| Ok(())
        )
        .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_reparse_points_are_rejected_when_symlink_creation_is_available() {
        use std::os::windows::fs::symlink_dir;
        let fixture = Fixture::new();
        put(&fixture.source, "options.txt", SOURCE_OPTIONS);
        let link = fixture.source.join("linked-directory");
        match symlink_dir(&fixture.target, &link) {
            Ok(()) => {}
            Err(cause)
                if matches!(cause.raw_os_error(), Some(1 | 50 | 1314))
                    || matches!(
                        cause.kind(),
                        io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
                    ) =>
            {
                eprintln!("Windowsリンク検証をスキップ: この環境ではリンク作成が許可されていません: {cause}");
                return;
            }
            Err(cause) => panic!("cannot create test symlink: {cause}"),
        }
        let failure = create_backup(
            &fixture.backups,
            &fixture.source,
            &profile("source"),
            BackupScope::Full,
            false,
        )
        .unwrap_err();
        assert!(failure.to_string().contains("リンク"));
        assert!(failure.to_string().contains("linked-directory"));
        fs::remove_dir(link).unwrap();
    }
}
