//! train-launcher-auth
//!
//! Microsoftアカウント(MSA)→XboxLive→XSTS→Minecraftトークンの認証チェーン、
//! およびDiscord OAuth2サインインを担当するクレート。
//!
//! `msa` / `discord` サインインフローは実装済み。ただし実際に動作させるには
//! Microsoft Entra ID / Discord Developer Portal でのアプリ登録が別途必要であり、
//! このクレート自体はUI(Tauri)に依存しないよう、進行状況はコールバック関数
//! (`on_device_code` / `on_authorize_url`)経由で呼び出し側に通知する設計になっている。

pub mod config;
pub mod discord;
pub mod msa;
pub mod store;
pub mod xbox;

pub use error::AuthError;

mod error {
    /// train-launcher-auth 全体で使用するエラー型。
    #[derive(Debug, thiserror::Error)]
    pub enum AuthError {
        #[error("not implemented yet: {0}")]
        NotImplemented(&'static str),
        #[error("http error: {0}")]
        Http(#[from] reqwest::Error),
        /// 必要な環境変数(クライアントID等)が設定されていない場合。
        #[error("認証設定が不足しています: {0} を環境変数に設定してください")]
        MissingConfig(&'static str),
        /// OAuth2トークンエンドポイントとのやり取りで発生したエラー。
        #[error("oauth2 error: {0}")]
        Oauth(String),
        /// Minecraft/XboxLive認証チェーンで発生したエラー。
        #[error("minecraft authentication error: {0}")]
        Minecraft(String),
        /// ローカルループバックサーバ(Discordリダイレクト受信用)のI/Oエラー。
        #[error("loopback server error: {0}")]
        LoopbackIo(#[from] std::io::Error),
        /// CSRFトークン(state)が一致しない場合。リプレイ/CSRF攻撃の可能性がある。
        #[error("state mismatch (possible CSRF)")]
        StateMismatch,
        /// OSの資格情報ストア(keyring)とのやり取りで発生したエラー。
        #[error("credential store error: {0}")]
        Keyring(#[from] keyring::Error),
        /// トークンのシリアライズ/デシリアライズに失敗した場合。
        #[error("token (de)serialization error: {0}")]
        Serde(#[from] serde_json::Error),
    }
}
