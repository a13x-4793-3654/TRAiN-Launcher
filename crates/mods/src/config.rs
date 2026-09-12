//! CurseForge APIキーの読み込み。
//!
//! CurseForgeの公開APIキーは発行者に紐づく秘密情報のため、ソースコードには埋め込まず
//! 環境変数から読み込む(`train_launcher_auth::config` のDiscord/MSAクライアントID読み込みと
//! 同じ方針)。

use crate::ModsError;

/// CurseForge APIキーを保持する環境変数名。
///
/// APIキーは <https://console.curseforge.com/#/api-keys> で発行する。
pub const CURSEFORGE_API_KEY_ENV_VAR: &str = "TRAIN_LAUNCHER_CURSEFORGE_API_KEY";

/// 環境変数からCurseForge APIキーを読み込む。未設定の場合は [`ModsError::ApiKeyMissing`] を返す。
pub fn curseforge_api_key_from_env() -> Result<String, ModsError> {
    std::env::var(CURSEFORGE_API_KEY_ENV_VAR).map_err(|_| ModsError::ApiKeyMissing)
}
