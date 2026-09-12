//! XboxLive → XSTS → Minecraft トークン変換 (`minecraft-msa-auth` crateを使用)。
//!
//! Microsoftアクセストークンを `minecraft_msa_auth::MinecraftAuthorizationFlow` でMinecraft
//! アクセストークンに変換したうえで、Minecraft Services API からプレイヤーのプロフィール
//! (ユーザー名・実際のMinecraft UUID)を取得する。
//!
//! 注意: `MinecraftAuthenticationResponse::username()` はXboxアカウントのUUIDであり、
//! Minecraftプレイヤーとしてのユーザー名/UUIDとは異なるため、別途プロフィールAPIを呼ぶ必要がある。

use minecraft_msa_auth::MinecraftAuthorizationFlow;
use serde::Deserialize;

use crate::AuthError;

const MINECRAFT_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// Minecraftの認証済みトークン。
#[derive(Debug, Clone)]
pub struct MinecraftToken {
    pub access_token: String,
    pub uuid: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MinecraftProfileResponse {
    id: String,
    name: String,
}

/// Microsoftアクセストークンを XboxLive/XSTS を経由して Minecraft トークンへ変換し、
/// 続けてMinecraftプレイヤーのプロフィール(ユーザー名・UUID)を取得する。
pub async fn exchange_microsoft_token(msa_access_token: &str) -> Result<MinecraftToken, AuthError> {
    let http_client = reqwest::Client::new();
    let flow = MinecraftAuthorizationFlow::new(http_client.clone());

    let mc_auth = flow
        .exchange_microsoft_token(msa_access_token)
        .await
        .map_err(|err| AuthError::Minecraft(err.to_string()))?;

    let access_token = mc_auth.access_token().as_ref().to_string();

    // プロフィール取得の失敗(サーバー障害等)はサインイン全体を失敗させるほどではないため、
    // 取得できなければ `username`/`uuid` を `None` のままにしてトークンだけ返す。
    let profile = http_client
        .get(MINECRAFT_PROFILE_URL)
        .bearer_auth(&access_token)
        .send()
        .await
        .ok()
        .filter(|resp| resp.status().is_success());

    let (uuid, username) = match profile {
        Some(resp) => match resp.json::<MinecraftProfileResponse>().await {
            Ok(profile) => (Some(profile.id), Some(profile.name)),
            Err(_) => (None, None),
        },
        None => (None, None),
    };

    Ok(MinecraftToken {
        access_token,
        uuid,
        username,
    })
}
