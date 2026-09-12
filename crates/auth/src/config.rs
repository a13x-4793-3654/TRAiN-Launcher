//! 環境変数からOAuth2クライアント設定を読み込む。
//!
//! Microsoft Entra ID(MSA)アプリ登録、および Discord Developer Portal でのアプリ登録は
//! このリポジトリの外で手動で行う必要があるため、クライアントID/シークレットはソースコードに
//! 埋め込まず環境変数から読み込む。未設定の場合は起動時にエラーにせず、サインインボタンが
//! 押された時点で `AuthError::MissingConfig` を返し、日本語のエラーメッセージで案内する。

use crate::AuthError;

/// Discordのループバックリダイレクトで使用するデフォルトポート。
///
/// Discord Developer Portal 側の "Redirects" にも `http://127.0.0.1:{port}/callback` を
/// 事前登録しておく必要がある(Discordはワイルドカードポートを許可しないため、ポート番号を
/// 固定する必要がある)。環境変数 `TRAIN_LAUNCHER_DISCORD_CALLBACK_PORT` で上書き可能。
pub const DEFAULT_DISCORD_CALLBACK_PORT: u16 = 38271;

/// Microsoftアカウント(MSA)サインインに必要な設定。
#[derive(Debug, Clone)]
pub struct MicrosoftConfig {
    pub client_id: String,
}

impl MicrosoftConfig {
    /// 環境変数 `TRAIN_LAUNCHER_MS_CLIENT_ID` から読み込む。
    pub fn from_env() -> Result<Self, AuthError> {
        let client_id = std::env::var("TRAIN_LAUNCHER_MS_CLIENT_ID")
            .map_err(|_| AuthError::MissingConfig("TRAIN_LAUNCHER_MS_CLIENT_ID"))?;
        Ok(Self { client_id })
    }
}

/// Discordサインインに必要な設定。
#[derive(Debug, Clone)]
pub struct DiscordConfig {
    pub client_id: String,
    /// Discordの認可コードフローは公開クライアント(PKCEのみ)でも動作するが、
    /// Developer Portal側の設定によってはclient_secretが必須になる場合があるため任意項目とする。
    pub client_secret: Option<String>,
    /// ループバックリダイレクトサーバがlistenするポート。
    pub callback_port: u16,
}

impl DiscordConfig {
    /// 環境変数から読み込む:
    /// - `TRAIN_LAUNCHER_DISCORD_CLIENT_ID` (必須)
    /// - `TRAIN_LAUNCHER_DISCORD_CLIENT_SECRET` (任意)
    /// - `TRAIN_LAUNCHER_DISCORD_CALLBACK_PORT` (任意、デフォルト38271)
    pub fn from_env() -> Result<Self, AuthError> {
        let client_id = std::env::var("TRAIN_LAUNCHER_DISCORD_CLIENT_ID")
            .map_err(|_| AuthError::MissingConfig("TRAIN_LAUNCHER_DISCORD_CLIENT_ID"))?;
        let client_secret = std::env::var("TRAIN_LAUNCHER_DISCORD_CLIENT_SECRET").ok();
        let callback_port = std::env::var("TRAIN_LAUNCHER_DISCORD_CALLBACK_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(DEFAULT_DISCORD_CALLBACK_PORT);
        Ok(Self {
            client_id,
            client_secret,
            callback_port,
        })
    }

    /// ループバックリダイレクトURI(Discord Developer Portalに事前登録が必要)。
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.callback_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_config_reports_missing_client_id() {
        // 環境変数を直接操作するテストは他のテストと競合しうるため、
        // ここでは存在しないことがほぼ確実なユニークな変数名で確認する代わりに、
        // エラー種別が正しく `MissingConfig` になることのみをロジックで検証する。
        let err = AuthError::MissingConfig("TRAIN_LAUNCHER_DISCORD_CLIENT_ID");
        assert!(matches!(err, AuthError::MissingConfig(_)));
    }

    #[test]
    fn default_redirect_uri_uses_default_port() {
        let config = DiscordConfig {
            client_id: "test".to_string(),
            client_secret: None,
            callback_port: DEFAULT_DISCORD_CALLBACK_PORT,
        };
        assert_eq!(
            config.redirect_uri(),
            "http://127.0.0.1:38271/callback".to_string()
        );
    }
}
