//! アプリ全体の設定(Javaランタイムパス・ゲームディレクトリの上書きなど)。
//!
//! プロファイル単位ではなくランチャー全体に適用される設定を扱う。プロファイル側で
//! 個別に値が指定されている場合はそちらが優先され、ここでの設定は「未指定時の既定値」
//! として使われる想定([`crate::profile::Profile::java_path`] など)。
//!
//! 保存先は [`crate::paths::default_launcher_root`] 直下の `settings.json`。

use serde::{Deserialize, Serialize};

use crate::paths::default_launcher_root;
use crate::CoreError;

/// アプリ全体の設定。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppSettings {
    /// 未指定のプロファイルで使用するJava実行ファイルのパス。
    /// 必要なJavaバージョン・アーキテクチャに適合する場合に優先する。
    /// 未指定または不適合の場合はインストール済みJavaを検索し、無ければ自動取得する。
    #[serde(default)]
    pub java_path: Option<String>,
    /// ゲームディレクトリ(バージョンjar・ライブラリ・アセット・Mod・リソースパック等の
    /// 保存先)の上書き先。`None` の場合は [`crate::paths::default_minecraft_root`]
    /// (公式Minecraft Launcherと共有する `.minecraft` 相当)を使用する。
    #[serde(default)]
    pub game_directory: Option<String>,
}

fn settings_file_path() -> std::path::PathBuf {
    default_launcher_root().join("settings.json")
}

/// 保存済み設定を読み込む。未保存の場合は全項目 `None` の既定値を返す。
pub fn load_settings() -> Result<AppSettings, CoreError> {
    match std::fs::read(settings_file_path()) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(AppSettings::default()),
        Err(err) => Err(err.into()),
    }
}

/// 設定を保存する。
pub fn save_settings(settings: &AppSettings) -> Result<(), CoreError> {
    let path = settings_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(settings)?)?;
    Ok(())
}
