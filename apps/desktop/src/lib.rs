//! TRAiN Launcher デスクトップアプリ(Tauri)本体。
//!
//! フロントエンド(React)から `@tauri-apps/api` の `invoke()` 経由で呼び出す
//! Tauri commandsをここに定義する。認証系commandは `train_launcher_auth` の実装を呼び出し、
//! 取得したトークンはOSの資格情報ストア(keyring)に保存する。
//! 実際にサインインを試すには Microsoft Entra ID / Discord Developer Portal でのアプリ登録と、
//! 対応する環境変数(`TRAIN_LAUNCHER_MS_CLIENT_ID` 等、詳細はREADME参照)の設定が必要。

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;
use train_launcher_auth::store::{self, Provider, TokenRecord};
use train_launcher_auth::{config, discord, msa, xbox};

/// フロントエンドへ返すサインイン結果。
#[derive(Debug, Clone, Serialize)]
struct SignInResult {
    display_name: String,
}

/// MSAデバイスコードフロー中にフロントエンドへemitするイベント名。
const MSA_DEVICE_CODE_EVENT: &str = "msa://device-code";

/// フロントエンドへemitするMSAデバイスコード情報(verification_uri/user_code)。
#[derive(Debug, Clone, Serialize)]
struct MsaDeviceCodePayload {
    verification_uri: String,
    user_code: String,
    expires_in_secs: u64,
}

/// Discordでサインインする。
///
/// 環境変数 `TRAIN_LAUNCHER_DISCORD_CLIENT_ID` (必須) / `TRAIN_LAUNCHER_DISCORD_CLIENT_SECRET`
/// (任意) / `TRAIN_LAUNCHER_DISCORD_CALLBACK_PORT` (任意) が必要。認可URLが用意できた時点で
/// システムブラウザを自動的に開く。取得したトークンはkeyringに保存する。
#[tauri::command]
async fn sign_in_with_discord(app_handle: AppHandle) -> Result<SignInResult, String> {
    let config = config::DiscordConfig::from_env().map_err(|err| err.to_string())?;

    let token = discord::sign_in(&config, move |url| {
        if let Err(err) = app_handle.opener().open_url(&url, None::<&str>) {
            // ブラウザを自動的に開けなくても致命的ではない(ユーザーがURLを手動で開ける可能性がある)
            // ため、ログ出力のみに留めてフロー自体は継続する。
            eprintln!("failed to open browser for Discord sign-in: {err}");
        }
    })
    .await
    .map_err(|err| err.to_string())?;

    store::save_token(
        Provider::Discord,
        &TokenRecord {
            access_token: token.access_token.clone(),
            refresh_token: token.refresh_token.clone(),
            expires_at: token.expires_at,
            display_name: Some(token.username.clone()),
        },
    )
    .map_err(|err| err.to_string())?;

    Ok(SignInResult {
        display_name: token.username,
    })
}

/// Microsoftアカウント(MSA)でサインインする。
///
/// 環境変数 `TRAIN_LAUNCHER_MS_CLIENT_ID` が必要。デバイスコードフローのため、
/// verification_uri/user_codeを `msa://device-code` イベントでフロントエンドへ通知し、
/// ユーザーがブラウザ側で認可するのを待つ。MSAトークン取得後、続けてXboxLive/XSTSを経由して
/// Minecraftトークンへ変換し、Minecraftプレイヤー名を取得する。
#[tauri::command]
async fn sign_in_with_microsoft(app_handle: AppHandle) -> Result<SignInResult, String> {
    let config = config::MicrosoftConfig::from_env().map_err(|err| err.to_string())?;

    let msa_token = msa::sign_in(&config, move |info| {
        let payload = MsaDeviceCodePayload {
            verification_uri: info.verification_uri,
            user_code: info.user_code,
            expires_in_secs: info.expires_in_secs,
        };
        if let Err(err) = app_handle.emit(MSA_DEVICE_CODE_EVENT, payload) {
            eprintln!("failed to emit MSA device code event: {err}");
        }
    })
    .await
    .map_err(|err| err.to_string())?;

    let minecraft_token = xbox::exchange_microsoft_token(&msa_token.access_token)
        .await
        .map_err(|err| err.to_string())?;

    let display_name = minecraft_token
        .username
        .clone()
        .unwrap_or_else(|| "Minecraftプレイヤー(ユーザー名取得失敗)".to_string());

    store::save_token(
        Provider::Microsoft,
        &TokenRecord {
            access_token: minecraft_token.access_token,
            refresh_token: msa_token.refresh_token,
            expires_at: msa_token.expires_at,
            display_name: Some(display_name.clone()),
        },
    )
    .map_err(|err| err.to_string())?;

    Ok(SignInResult { display_name })
}

/// Discordのサインアウト(保存済みトークンの削除)。
#[tauri::command]
fn sign_out_discord() -> Result<(), String> {
    store::delete_token(Provider::Discord).map_err(|err| err.to_string())
}

/// Microsoftのサインアウト(保存済みトークンの削除)。
#[tauri::command]
fn sign_out_microsoft() -> Result<(), String> {
    store::delete_token(Provider::Microsoft).map_err(|err| err.to_string())
}

/// 現在のサインイン状態を取得する(アプリ起動時のセッション復元UX用)。
#[derive(Debug, Clone, Serialize)]
struct AuthStatus {
    discord_display_name: Option<String>,
    microsoft_display_name: Option<String>,
}

#[tauri::command]
fn get_auth_status() -> Result<AuthStatus, String> {
    let discord_display_name = store::load_token(Provider::Discord)
        .map_err(|err| err.to_string())?
        .and_then(|record| record.display_name);
    let microsoft_display_name = store::load_token(Provider::Microsoft)
        .map_err(|err| err.to_string())?
        .and_then(|record| record.display_name);

    Ok(AuthStatus {
        discord_display_name,
        microsoft_display_name,
    })
}

/// 保存済みプロファイル一覧を取得する(スタブ)。
///
/// TODO: `train_launcher_core::profile` の実装完了後、実際のプロファイル一覧を返す。
#[tauri::command]
fn list_profiles() -> Result<Vec<String>, String> {
    train_launcher_core::profile::list_profiles()
        .map(|profiles| profiles.into_iter().map(|p| p.name).collect())
        .map_err(|err| err.to_string())
}

/// Discordサインイン後の所属サーバー一覧を取得する(スタブ、モック実装を使用)。
///
/// TODO: TRAiN API仕様確定後、`MockTrainApiClient` を実際のHTTPクライアント実装に置き換える。
#[tauri::command]
async fn list_member_servers() -> Result<Vec<String>, String> {
    use train_launcher_server_api::{MockTrainApiClient, TrainApiClient};

    let client = MockTrainApiClient;
    client
        .get_member_servers("mock-discord-user")
        .await
        .map(|servers| servers.into_iter().map(|s| s.name).collect())
        .map_err(|err| err.to_string())
}

/// URL指定でModを解決する(スタブ、Modrinth経由)。
///
/// TODO: `train_launcher_mods` の実装完了後、依存Modも含めた解決結果を返す。
#[tauri::command]
async fn resolve_mod_url(url: String) -> Result<(), String> {
    train_launcher_mods::modrinth::resolve_from_url(&url)
        .await
        .map_err(|err| err.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            sign_in_with_discord,
            sign_in_with_microsoft,
            sign_out_discord,
            sign_out_microsoft,
            get_auth_status,
            list_profiles,
            list_member_servers,
            resolve_mod_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
