//! プロファイル(起動構成)管理。
//!
//! プロファイルは「どのMinecraftバージョンを・どのModローダーで・どのサーバーに接続して
//! 起動するか」をまとめた設定単位。TRAiNの所属サーバーから自動取得した設定、または
//! ユーザーが手動で作成した設定のいずれからも生成できる想定。
//!
//! 保存先は `paths::default_launcher_root()` 直下の `profiles.json` (単純なJSON配列)。
//!
//! `.minecraft` フォルダは公式Minecraft Launcherと共有しているため
//! ([`crate::paths::default_minecraft_root`])、プロファイルについても双方向に連携する:
//! - TRAiN Launcherで作成/更新/削除したプロファイルは、公式ランチャーの
//!   `launcher_profiles.json` にも反映し、公式ランチャー側の起動構成一覧にも表示されるようにする。
//! - 逆に公式ランチャー側で作成済みのプロファイルも [`list_profiles`] の結果に取り込み、
//!   TRAiN Launcher側でも表示・編集・起動できるようにする([`ProfileSource::Official`])。
//! 公式ランチャー側との連携(`launcher_profiles.json`の読み書き)に失敗しても、TRAiN側の
//! `profiles.json` への保存自体は成功させる(標準エラー出力にログを残すのみに留める)。

use serde::{Deserialize, Serialize};

use crate::launcher_profiles::{self, OfficialProfile};
use crate::paths::{default_launcher_root, effective_minecraft_root};
use crate::CoreError;

/// プロファイルの管理元。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSource {
    /// TRAiN Launcherの `profiles.json` で管理されているプロファイル。
    #[default]
    Train,
    /// 公式Minecraft Launcherの `launcher_profiles.json` にのみ存在し、TRAiN側では
    /// まだ保存されていないプロファイル(表示専用の取り込みであり、編集/起動すると
    /// TRAiN側にも取り込まれ`Train`として保存される)。
    Official,
}

/// 1つの起動プロファイル。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub minecraft_version: String,
    /// Modローダーの種類(`"fabric"` / `"quilt"` / `"forge"` / `"neoforge"`、
    /// 大文字小文字は区別しない)。指定されている場合、起動時に
    /// `minecraft_version`(バニラ版)を基準にローダーを自動導入し、
    /// 導入後の派生バージョンで起動する(未対応の文字列の場合は起動時にエラーになる)。
    #[serde(default)]
    pub mod_loader: Option<String>,
    /// 導入するローダーの具体的なバージョン(例: `"0.19.5"`)。未指定の場合は
    /// 安定版の最新(Forgeは`recommended`)を自動選択する。`mod_loader` が
    /// `None` の場合は無視される。
    #[serde(default)]
    pub mod_loader_version: Option<String>,
    /// TRAiNの所属サーバーから自動取得したプロファイルの場合、そのサーバーID。
    #[serde(default)]
    pub server_id: Option<String>,
    /// このプロファイル専用のゲームディレクトリ(Mod・リソースパック・セーブデータ・
    /// `options.txt` 等の実際の保存先)。未指定の場合は全プロファイル共通の `.minecraft`
    /// 相当ディレクトリ([`crate::paths::effective_minecraft_root`])を使う。
    ///
    /// バージョンjar・ライブラリ・アセットは指定の有無に関わらず常に共通ディレクトリ側を
    /// 再利用する(ディスク容量節約のため、これらはこのフィールドの影響を受けない)。
    /// 公式Minecraft Launcherの `launcher_profiles.json` が持つ同名の `gameDir` 概念と
    /// 対応しており、双方向に同期される。
    #[serde(default)]
    pub game_dir: Option<String>,
    /// 未指定の場合はPATH上の `java` を使用する。
    #[serde(default)]
    pub java_path: Option<String>,
    /// JVMヒープ最大値(MB)。未指定の場合は `-Xmx` を付与しない(JVM既定値に任せる)。
    #[serde(default)]
    pub max_memory_mb: Option<u32>,
    #[serde(default)]
    pub source: ProfileSource,
    /// 最終起動日時(ISO 8601、UTC)。ホーム画面の「最近使ったプロファイル」表示に使用する。
    /// 未起動の場合は `None`。
    #[serde(default)]
    pub last_launched_at: Option<String>,
}

impl Profile {
    /// Mod・リソースパック・セーブデータ等の実際の保存先ディレクトリを解決する。
    ///
    /// `game_dir` が指定されていればそれを、未指定(または空文字列)の場合は
    /// `shared_minecraft_root`(全プロファイル共通の `.minecraft` 相当ディレクトリ)を返す。
    /// バージョンjar・ライブラリ・アセットの解決には使わないこと
    /// (それらは常に `shared_minecraft_root` 側、[`crate::launch::build_launch_command`]
    /// 参照)。
    pub fn effective_game_dir(&self, shared_minecraft_root: &std::path::Path) -> std::path::PathBuf {
        match self.game_dir.as_deref().map(str::trim) {
            Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
            _ => shared_minecraft_root.to_path_buf(),
        }
    }
}

fn profiles_file_path() -> std::path::PathBuf {
    default_launcher_root().join("profiles.json")
}

/// 設定で上書きされていればその値を、なければ既定の `.minecraft` 相当ディレクトリを返す。
fn minecraft_root() -> std::path::PathBuf {
    let settings = crate::settings::load_settings().unwrap_or_default();
    effective_minecraft_root(settings.game_directory.as_deref())
}

fn load_all() -> Result<Vec<Profile>, CoreError> {
    match std::fs::read(profiles_file_path()) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err.into()),
    }
}

fn save_all(profiles: &[Profile]) -> Result<(), CoreError> {
    let path = profiles_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(profiles)?)?;
    Ok(())
}

/// `javaArgs` 文字列から `-Xmx` 値をMB単位で抽出する(見つからない/解釈できない場合は `None`)。
/// 例: `"-Xmx4096M -Xms4096M"` → `Some(4096)`、`"-Xmx4G"` → `Some(4096)`。
fn parse_max_memory_mb(java_args: &str) -> Option<u32> {
    for token in java_args.split_whitespace() {
        if let Some(rest) = token.strip_prefix("-Xmx") {
            if rest.is_empty() {
                continue;
            }
            let (digits, unit) = rest.split_at(rest.len() - 1);
            if let Ok(value) = digits.parse::<u32>() {
                match unit.to_ascii_uppercase().as_str() {
                    "M" => return Some(value),
                    "G" => return Some(value.saturating_mul(1024)),
                    _ => {}
                }
            }
        }
    }
    None
}

fn official_to_profile(official: OfficialProfile) -> Profile {
    let max_memory_mb = official.java_args.as_deref().and_then(parse_max_memory_mb);
    Profile {
        id: official.id,
        name: official.name,
        minecraft_version: official.last_version_id.unwrap_or_default(),
        mod_loader: None,
        mod_loader_version: None,
        server_id: None,
        game_dir: official.game_dir,
        java_path: official.java_dir,
        max_memory_mb,
        source: ProfileSource::Official,
        last_launched_at: None,
    }
}

/// TRAiN Launcherのプロファイルを公式ランチャーの `launcher_profiles.json` にも反映する。
/// 失敗してもTRAiN側の保存自体は失敗させたくないため、エラーはログ出力のみに留める。
fn sync_to_official_launcher(profile: &Profile) {
    if let Err(err) = launcher_profiles::upsert_profile(&minecraft_root(), profile) {
        eprintln!("failed to sync profile to launcher_profiles.json: {err}");
    }
}

/// 保存済みプロファイル一覧を取得する。
///
/// TRAiN Launcher自身の `profiles.json` に加えて、公式ランチャーの
/// `launcher_profiles.json` に存在し、まだTRAiN側に取り込まれていないプロファイルも
/// (`ProfileSource::Official` として)一覧に含める。
pub fn list_profiles() -> Result<Vec<Profile>, CoreError> {
    let mut profiles = load_all()?;
    let known_ids: std::collections::HashSet<String> =
        profiles.iter().map(|profile| profile.id.clone()).collect();

    for official in launcher_profiles::read_all(&minecraft_root())? {
        if !known_ids.contains(&official.id) {
            profiles.push(official_to_profile(official));
        }
    }
    Ok(profiles)
}

/// 新規プロファイルを作成する。同じIDが既に存在する場合はエラーを返す。
pub fn create_profile(profile: Profile) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    if profiles.iter().any(|existing| existing.id == profile.id) {
        return Err(CoreError::ProfileAlreadyExists(profile.id));
    }
    profiles.push(profile.clone());
    save_all(&profiles)?;
    sync_to_official_launcher(&profile);
    Ok(())
}

/// IDを指定して1件のプロファイルを取得する。TRAiN側に無い場合は公式ランチャー側
/// (`launcher_profiles.json`)も参照する。どちらにも存在しない場合はエラーを返す。
pub fn get_profile(id: &str) -> Result<Profile, CoreError> {
    if let Some(profile) = load_all()?.into_iter().find(|profile| profile.id == id) {
        return Ok(profile);
    }
    let official = launcher_profiles::read_all(&minecraft_root())?
        .into_iter()
        .find(|official| official.id == id);
    match official {
        Some(official) => Ok(official_to_profile(official)),
        None => Err(CoreError::ProfileNotFound(id.to_string())),
    }
}

/// 既存プロファイルを更新する(IDは変更不可)。
///
/// 対象IDがTRAiN側の `profiles.json` にまだ存在しない場合(公式ランチャー側のみに
/// 存在するプロファイルをTRAiN側で編集した場合など)は、新規追加として取り込む。
pub fn update_profile(profile: Profile) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    match profiles.iter().position(|existing| existing.id == profile.id) {
        Some(index) => profiles[index] = profile.clone(),
        None => profiles.push(profile.clone()),
    }
    save_all(&profiles)?;
    sync_to_official_launcher(&profile);
    Ok(())
}

/// プロファイルを削除する。TRAiN側・公式ランチャー側のどちらか一方にのみ存在する場合も
/// 削除する。どちらにも存在しない場合はエラーを返す。
pub fn delete_profile(id: &str) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    let original_len = profiles.len();
    profiles.retain(|profile| profile.id != id);
    let existed_in_train_store = profiles.len() != original_len;
    if existed_in_train_store {
        save_all(&profiles)?;
    }

    let removed_from_official = launcher_profiles::remove_profile(&minecraft_root(), id)
        .unwrap_or_else(|err| {
            eprintln!("failed to remove profile from launcher_profiles.json: {err}");
            false
        });

    if !existed_in_train_store && !removed_from_official {
        return Err(CoreError::ProfileNotFound(id.to_string()));
    }
    Ok(())
}

/// 指定プロファイルの最終起動日時を現在時刻(UTC)で更新する。
///
/// ホーム画面の「最近使ったプロファイル」表示のために、`launch_minecraft`/
/// `join_train_server` からの起動成功時に呼び出される。対象IDがTRAiN側の
/// `profiles.json` にまだ存在しない場合(公式ランチャー側のみに存在するプロファイルを
/// 起動した場合など)は、[`get_profile`] で解決した内容を新規に取り込んで保存する。
pub fn mark_launched(id: &str) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    let now = chrono::Utc::now().to_rfc3339();
    match profiles.iter().position(|profile| profile.id == id) {
        Some(index) => profiles[index].last_launched_at = Some(now),
        None => {
            let mut profile = get_profile(id)?;
            profile.last_launched_at = Some(now);
            profiles.push(profile);
        }
    }
    save_all(&profiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn sample_profile(game_dir: Option<&str>) -> Profile {
        Profile {
            id: "test".to_string(),
            name: "test".to_string(),
            minecraft_version: "1.20.4".to_string(),
            mod_loader: None,
            mod_loader_version: None,
            server_id: None,
            game_dir: game_dir.map(str::to_string),
            java_path: None,
            max_memory_mb: None,
            source: ProfileSource::Train,
            last_launched_at: None,
        }
    }

    #[test]
    fn effective_game_dir_falls_back_to_shared_root_when_unset() {
        let profile = sample_profile(None);
        let shared_root = Path::new("/shared/.minecraft");
        assert_eq!(profile.effective_game_dir(shared_root), shared_root);
    }

    #[test]
    fn effective_game_dir_falls_back_to_shared_root_when_blank() {
        let profile = sample_profile(Some("   "));
        let shared_root = Path::new("/shared/.minecraft");
        assert_eq!(profile.effective_game_dir(shared_root), shared_root);
    }

    #[test]
    fn effective_game_dir_uses_override_when_set() {
        let profile = sample_profile(Some("/custom/profile-dir"));
        let shared_root = Path::new("/shared/.minecraft");
        assert_eq!(
            profile.effective_game_dir(shared_root),
            Path::new("/custom/profile-dir")
        );
    }
}
