//! プロファイル(起動構成)管理。
//!
//! プロファイルは「どのMinecraftバージョンを・どのModローダーで・どのサーバーに接続して
//! 起動するか」をまとめた設定単位。TRAiNの所属サーバーから自動取得した設定、または
//! ユーザーが手動で作成した設定のいずれからも生成できる想定。
//!
//! 保存先は `paths::default_launcher_root()` 直下の `profiles.json` (単純なJSON配列)。

use serde::{Deserialize, Serialize};

use crate::paths::default_launcher_root;
use crate::CoreError;

/// 1つの起動プロファイル。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub minecraft_version: String,
    /// TODO: Forge/Fabric等のModローダー起動には未対応。現状は表示・保存のみ。
    #[serde(default)]
    pub mod_loader: Option<String>,
    /// TRAiNの所属サーバーから自動取得したプロファイルの場合、そのサーバーID。
    #[serde(default)]
    pub server_id: Option<String>,
    /// 未指定の場合はPATH上の `java` を使用する。
    #[serde(default)]
    pub java_path: Option<String>,
    /// JVMヒープ最大値(MB)。未指定の場合は `-Xmx` を付与しない(JVM既定値に任せる)。
    #[serde(default)]
    pub max_memory_mb: Option<u32>,
}

fn profiles_file_path() -> std::path::PathBuf {
    default_launcher_root().join("profiles.json")
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

/// 保存済みプロファイル一覧を取得する。保存ファイルが無い場合は空一覧を返す
/// (プロファイル未作成は正常な初期状態であり、エラーとして扱わない)。
pub fn list_profiles() -> Result<Vec<Profile>, CoreError> {
    load_all()
}

/// 新規プロファイルを作成する。同じIDが既に存在する場合はエラーを返す。
pub fn create_profile(profile: Profile) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    if profiles.iter().any(|existing| existing.id == profile.id) {
        return Err(CoreError::ProfileAlreadyExists(profile.id));
    }
    profiles.push(profile);
    save_all(&profiles)
}

/// プロファイルを削除する。存在しないIDの場合はエラーを返す。
pub fn delete_profile(id: &str) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    let original_len = profiles.len();
    profiles.retain(|profile| profile.id != id);
    if profiles.len() == original_len {
        return Err(CoreError::ProfileNotFound(id.to_string()));
    }
    save_all(&profiles)
}
