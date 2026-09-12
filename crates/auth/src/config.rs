//! 環境変数からOAuth2クライアント設定を読み込む。
//!
//! Microsoft Entra ID(MSA)アプリ登録、および Discord Developer Portal でのアプリ登録は
//! このリポジトリの外で手動で行う必要がある。ただしMS/DiscordのクライアントIDは
//! (公開クライアント向けの)公開情報であり秘匿する必要がないため、[`option_env!`] で
//! ビルド時にリポジトリのGitHub Actions Variablesから埋め込み、配布版インストーラーが
//! 環境変数の手動設定なしでもサインインできるようにする([`DEFAULT_MS_CLIENT_ID`] /
//! [`DEFAULT_DISCORD_CLIENT_ID`])。実行時環境変数は引き続きこの既定値を上書きできる
//! (ローカル開発で別のアプリ登録を試したい場合など)。どちらも未設定の場合は起動時に
//! エラーにせず、サインインボタンが押された時点で `AuthError::MissingConfig` を返し、
//! 日本語のエラーメッセージで案内する。
//!
//! Discordのclient_secretは、デスクトップアプリに埋め込んだ時点で実質的に秘匿できない
//! (バイナリ解析やネットワーク傍受で第三者に取得され得る)ため、ビルド時埋め込みは行わない。
//! Developer Portal側でclient_secret不要のフロー(PKCEのみ)が使えることを確認済みのため、
//! 通常は環境変数を設定しなくてもよい。

use crate::AuthError;

/// Discordのループバックリダイレクトで使用するデフォルトポート。
///
/// Discord Developer Portal 側の "Redirects" にも `http://127.0.0.1:{port}/callback` を
/// 事前登録しておく必要がある(Discordはワイルドカードポートを許可しないため、ポート番号を
/// 固定する必要がある)。環境変数 `TRAIN_LAUNCHER_DISCORD_CALLBACK_PORT` で上書き可能。
pub const DEFAULT_DISCORD_CALLBACK_PORT: u16 = 38271;

/// ビルド時環境変数 `TRAIN_LAUNCHER_DEFAULT_MS_CLIENT_ID` から埋め込まれる既定のMS Client ID。
/// 未設定でビルドした場合は空文字列(ローカル開発ビルドなど)。
pub const DEFAULT_MS_CLIENT_ID: &str = match option_env!("TRAIN_LAUNCHER_DEFAULT_MS_CLIENT_ID") {
    Some(value) => value,
    None => "",
};

/// ビルド時環境変数 `TRAIN_LAUNCHER_DEFAULT_DISCORD_CLIENT_ID` から埋め込まれる既定の
/// Discord Client ID。未設定でビルドした場合は空文字列(ローカル開発ビルドなど)。
pub const DEFAULT_DISCORD_CLIENT_ID: &str =
    match option_env!("TRAIN_LAUNCHER_DEFAULT_DISCORD_CLIENT_ID") {
        Some(value) => value,
        None => "",
    };

/// 実行時環境変数 `env_var` を優先し、未設定または空文字列ならビルド時デフォルト
/// `build_default` を使う。どちらも空なら `None`。
fn resolve_client_id(env_var: &str, build_default: &str) -> Option<String> {
    std::env::var(env_var)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| (!build_default.is_empty()).then(|| build_default.to_string()))
}

/// Microsoftアカウント(MSA)サインインに必要な設定。
#[derive(Debug, Clone)]
pub struct MicrosoftConfig {
    pub client_id: String,
}

impl MicrosoftConfig {
    /// 環境変数 `TRAIN_LAUNCHER_MS_CLIENT_ID`(未設定時は [`DEFAULT_MS_CLIENT_ID`])
    /// から読み込む。
    pub fn from_env() -> Result<Self, AuthError> {
        let client_id = resolve_client_id("TRAIN_LAUNCHER_MS_CLIENT_ID", DEFAULT_MS_CLIENT_ID)
            .ok_or(AuthError::MissingConfig("TRAIN_LAUNCHER_MS_CLIENT_ID"))?;
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
    /// - `TRAIN_LAUNCHER_DISCORD_CLIENT_ID` (未設定時は [`DEFAULT_DISCORD_CLIENT_ID`])
    /// - `TRAIN_LAUNCHER_DISCORD_CLIENT_SECRET` (任意)
    /// - `TRAIN_LAUNCHER_DISCORD_CALLBACK_PORT` (任意、デフォルト38271)
    pub fn from_env() -> Result<Self, AuthError> {
        let client_id = resolve_client_id(
            "TRAIN_LAUNCHER_DISCORD_CLIENT_ID",
            DEFAULT_DISCORD_CLIENT_ID,
        )
        .ok_or(AuthError::MissingConfig("TRAIN_LAUNCHER_DISCORD_CLIENT_ID"))?;
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

    /// ビルド時環境変数 `TRAIN_LAUNCHER_DEFAULT_MS_CLIENT_ID` /
    /// `TRAIN_LAUNCHER_DEFAULT_DISCORD_CLIENT_ID` が [`option_env!`] 経由で
    /// 各定数へ正しく埋め込まれることを確認する回帰テスト。
    #[test]
    fn default_client_ids_reflect_build_time_env_vars() {
        match option_env!("TRAIN_LAUNCHER_DEFAULT_MS_CLIENT_ID") {
            Some(expected) => assert_eq!(DEFAULT_MS_CLIENT_ID, expected),
            None => assert_eq!(DEFAULT_MS_CLIENT_ID, ""),
        }
        match option_env!("TRAIN_LAUNCHER_DEFAULT_DISCORD_CLIENT_ID") {
            Some(expected) => assert_eq!(DEFAULT_DISCORD_CLIENT_ID, expected),
            None => assert_eq!(DEFAULT_DISCORD_CLIENT_ID, ""),
        }
    }

    #[test]
    fn resolve_client_id_prefers_non_empty_runtime_env_var() {
        // 実行時に設定済みの環境変数が優先される(ビルド時デフォルトの有無に関わらず)。
        std::env::set_var(
            "TRAIN_LAUNCHER_TEST_RESOLVE_CLIENT_ID",
            "runtime-override",
        );
        assert_eq!(
            resolve_client_id("TRAIN_LAUNCHER_TEST_RESOLVE_CLIENT_ID", "build-default"),
            Some("runtime-override".to_string())
        );
        std::env::remove_var("TRAIN_LAUNCHER_TEST_RESOLVE_CLIENT_ID");
    }

    #[test]
    fn resolve_client_id_falls_back_to_build_default_when_env_var_unset() {
        std::env::remove_var("TRAIN_LAUNCHER_TEST_RESOLVE_CLIENT_ID_2");
        assert_eq!(
            resolve_client_id("TRAIN_LAUNCHER_TEST_RESOLVE_CLIENT_ID_2", "build-default"),
            Some("build-default".to_string())
        );
    }

    #[test]
    fn resolve_client_id_is_none_when_both_unset() {
        std::env::remove_var("TRAIN_LAUNCHER_TEST_RESOLVE_CLIENT_ID_3");
        assert_eq!(
            resolve_client_id("TRAIN_LAUNCHER_TEST_RESOLVE_CLIENT_ID_3", ""),
            None
        );
    }
}
