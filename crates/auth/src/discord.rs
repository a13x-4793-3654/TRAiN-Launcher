//! Discord OAuth2 サインイン(標準的な認可コードフロー)。
//!
//! Discord Developer Portal でのアプリ登録(client_id / client_secret / redirect_uri)が
//! 別途必要。サインイン後、Discordユーザーの所属サーバー情報を
//! `train_launcher_server_api::TrainApiClient` 経由でTRAiNバックエンドに問い合わせる想定。

use oauth2::ClientId;

use crate::AuthError;

/// Discord OAuth2 のエンドポイント。
pub const DISCORD_AUTH_URL: &str = "https://discord.com/api/oauth2/authorize";
pub const DISCORD_TOKEN_URL: &str = "https://discord.com/api/oauth2/token";

/// Discordサインインで得られるトークン(スタブ)。
#[derive(Debug, Clone)]
pub struct DiscordToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

/// Discord OAuth2の認可コードフローでサインインを行う。
///
/// TODO: ローカルループバックサーバでのリダイレクト受信、PKCE、トークン交換を実装する。
pub async fn sign_in(client_id: &str) -> Result<DiscordToken, AuthError> {
    let _client_id = ClientId::new(client_id.to_string());
    Err(AuthError::NotImplemented("discord::sign_in"))
}
