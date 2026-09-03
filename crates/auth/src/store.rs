//! OSネイティブの資格情報ストア(keyring crate)を使ったトークン永続化。
//!
//! Windows Credential Manager / macOS Keychain / Linux Secret Service にJSONシリアライズした
//! `TokenRecord` を1エントリとして保存する。サービス名は固定で `"train-launcher"`、
//! ユーザー名にプロバイダ識別子(`"discord"` / `"microsoft"`)を使う。

use serde::{Deserialize, Serialize};

use crate::AuthError;

const SERVICE_NAME: &str = "train-launcher";

/// サインインプロバイダの識別子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Discord,
    Microsoft,
}

impl Provider {
    fn keyring_user(self) -> &'static str {
        match self {
            Provider::Discord => "discord",
            Provider::Microsoft => "microsoft",
        }
    }
}

/// 保存対象のトークン情報。アクセストークン/リフレッシュトークン/有効期限(UNIX秒)に加え、
/// UIに表示するための表示名(Discordユーザー名やMinecraftプレイヤー名)も保持する。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRecord {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// アクセストークンの有効期限(UNIX epoch秒)。不明な場合は `None`。
    pub expires_at: Option<i64>,
    /// UI表示用のアカウント表示名。
    pub display_name: Option<String>,
}

/// トークンを資格情報ストアに保存する(既存エントリは上書き)。
pub fn save_token(provider: Provider, record: &TokenRecord) -> Result<(), AuthError> {
    let entry = keyring::Entry::new(SERVICE_NAME, provider.keyring_user())?;
    let json = serde_json::to_string(record)?;
    entry.set_password(&json)?;
    Ok(())
}

/// 保存済みのトークンを読み込む。エントリが存在しない場合は `Ok(None)` を返す
/// (未サインイン状態は正常なケースであり、エラーとして扱わない)。
pub fn load_token(provider: Provider) -> Result<Option<TokenRecord>, AuthError> {
    let entry = keyring::Entry::new(SERVICE_NAME, provider.keyring_user())?;
    match entry.get_password() {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// 保存済みのトークンを削除する(サインアウト)。エントリが存在しない場合も成功扱いとする。
pub fn delete_token(provider: Provider) -> Result<(), AuthError> {
    let entry = keyring::Entry::new(SERVICE_NAME, provider.keyring_user())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.into()),
    }
}
