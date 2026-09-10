//! OSネイティブの資格情報ストア(keyring crate)を使ったトークン永続化。
//!
//! Windows Credential Manager / macOS Keychain / Linux Secret Service にJSONシリアライズした
//! `TokenRecord` を保存する。サービス名は固定で `"train-launcher"`、
//! ユーザー名にプロバイダ識別子(`"discord"` / `"microsoft"`)を使う。
//!
//! Windows Credential Managerの1エントリあたりの容量制限(パスワードがUTF-16で
//! 2560文字を超えられない)を回避するため、`access_token` と
//! `refresh_token`/`expires_at`/`display_name` を別々の2エントリに分割して保存する
//! (MinecraftのアクセストークンとMSAのリフレッシュトークンはどちらも長大なJWT/不透明文字列
//! であり、1エントリにまとめると容量制限を超えることがあるため)。

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

    /// `refresh_token`/`expires_at`/`display_name` を保存する2つ目のエントリのユーザー名。
    fn keyring_user_meta(self) -> &'static str {
        match self {
            Provider::Discord => "discord-meta",
            Provider::Microsoft => "microsoft-meta",
        }
    }
}

/// `access_token` 以外の付随情報。容量の小さい2つ目のエントリとして保存する。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TokenMeta {
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    display_name: Option<String>,
    /// Minecraftプレイヤーとしての UUID (Microsoftプロバイダのみ)。
    /// `crates/core` のゲーム起動時に `auth_uuid` 起動引数として使用する。
    uuid: Option<String>,
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
    /// Minecraftプレイヤーとしての UUID (Microsoftプロバイダのみ、ゲーム起動に必要)。
    #[serde(default)]
    pub uuid: Option<String>,
}

/// トークンを資格情報ストアに保存する(既存エントリは上書き)。
///
/// `access_token` と付随情報(`refresh_token`/`expires_at`/`display_name`/`uuid`)を別エントリに
/// 分割して保存する(理由は本モジュールのドキュメントコメントを参照)。
pub fn save_token(provider: Provider, record: &TokenRecord) -> Result<(), AuthError> {
    let entry = keyring::Entry::new(SERVICE_NAME, provider.keyring_user())?;
    entry.set_password(&record.access_token)?;

    let meta = TokenMeta {
        refresh_token: record.refresh_token.clone(),
        expires_at: record.expires_at,
        display_name: record.display_name.clone(),
        uuid: record.uuid.clone(),
    };
    let meta_entry = keyring::Entry::new(SERVICE_NAME, provider.keyring_user_meta())?;
    meta_entry.set_password(&serde_json::to_string(&meta)?)?;
    Ok(())
}

/// 保存済みのトークンを読み込む。エントリが存在しない場合は `Ok(None)` を返す
/// (未サインイン状態は正常なケースであり、エラーとして扱わない)。
pub fn load_token(provider: Provider) -> Result<Option<TokenRecord>, AuthError> {
    let entry = keyring::Entry::new(SERVICE_NAME, provider.keyring_user())?;
    let access_token = match entry.get_password() {
        Ok(password) => password,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    let meta_entry = keyring::Entry::new(SERVICE_NAME, provider.keyring_user_meta())?;
    let meta = match meta_entry.get_password() {
        Ok(json) => serde_json::from_str::<TokenMeta>(&json)?,
        // 旧バージョン(分割前)からの移行時など、付随情報エントリが無い場合は空扱いにする。
        Err(keyring::Error::NoEntry) => TokenMeta::default(),
        Err(err) => return Err(err.into()),
    };

    Ok(Some(TokenRecord {
        access_token,
        refresh_token: meta.refresh_token,
        expires_at: meta.expires_at,
        display_name: meta.display_name,
        uuid: meta.uuid,
    }))
}

/// 保存済みのトークンを削除する(サインアウト)。エントリが存在しない場合も成功扱いとする。
pub fn delete_token(provider: Provider) -> Result<(), AuthError> {
    let entry = keyring::Entry::new(SERVICE_NAME, provider.keyring_user())?;
    let primary = match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AuthError::from(err)),
    };

    let meta_entry = keyring::Entry::new(SERVICE_NAME, provider.keyring_user_meta())?;
    let meta = match meta_entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AuthError::from(err)),
    };

    primary.and(meta)
}
