//! XboxLive → XSTS → Minecraft トークン変換 (`minecraft-msa-auth` crateを使用)。
//!
//! 実際のフローは以下のような形になる想定(TODO、crateのドキュメント例を参照):
//!
//! ```ignore
//! let mc_flow = MinecraftAuthorizationFlow::new(reqwest::Client::new());
//! let mc_token = mc_flow.exchange_microsoft_token(msa_access_token).await?;
//! ```

use minecraft_msa_auth::MinecraftAuthorizationFlow;

use crate::AuthError;

/// Minecraftの認証済みトークン(スタブ)。
#[derive(Debug, Clone)]
pub struct MinecraftToken {
    pub access_token: String,
    pub uuid: Option<String>,
}

/// Microsoftアクセストークンを XboxLive/XSTS を経由して Minecraft トークンへ変換する。
///
/// TODO: `minecraft-msa-auth` crateの `MinecraftAuthorizationFlow::exchange_microsoft_token`
/// を実装し、得られたMinecraftトークン・UUID・プロフィール名を `MinecraftToken` にマッピングする。
pub async fn exchange_microsoft_token(
    _msa_access_token: &str,
) -> Result<MinecraftToken, AuthError> {
    let _flow = MinecraftAuthorizationFlow::new(reqwest::Client::new());
    Err(AuthError::NotImplemented(
        "xbox::exchange_microsoft_token",
    ))
}
