//! プロファイル(起動構成)管理。
//!
//! プロファイルは「どのMinecraftバージョンを・どのModローダーで・どのサーバーに接続して
//! 起動するか」をまとめた設定単位。TRAiNの所属サーバーから自動取得した設定、または
//! ユーザーが手動で作成した設定のいずれからも生成できる想定。

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// 1つの起動プロファイル。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub minecraft_version: String,
    pub mod_loader: Option<String>,
    /// TRAiNの所属サーバーから自動取得したプロファイルの場合、そのサーバーID。
    pub server_id: Option<String>,
}

/// 保存済みプロファイル一覧を取得する。
///
/// TODO: ローカル設定ファイル (JSON) からの読み込みを実装する。
pub fn list_profiles() -> Result<Vec<Profile>, CoreError> {
    Err(CoreError::NotImplemented("profile::list_profiles"))
}

/// 新規プロファイルを作成する。
///
/// TODO: 設定ファイルへの永続化を実装する。
pub fn create_profile(_profile: Profile) -> Result<(), CoreError> {
    Err(CoreError::NotImplemented("profile::create_profile"))
}

/// プロファイルを削除する。
///
/// TODO: 設定ファイルからの削除を実装する。
pub fn delete_profile(_id: &str) -> Result<(), CoreError> {
    Err(CoreError::NotImplemented("profile::delete_profile"))
}
