//! TRAiN Launcher デスクトップアプリ(Tauri)本体。
//!
//! フロントエンド(React)から `@tauri-apps/api` の `invoke()` 経由で呼び出す
//! Tauri commandsをここに定義する。各commandは `crates/*` のスタブ実装を呼び出しており、
//! 現時点では「未実装」エラーを返すが、crate間の配線(呼び出し経路)自体は疎通済み。

/// Discordでサインインする。
///
/// TODO: `train_launcher_auth::discord::sign_in` の実装完了後、実際のOAuth2フロー
/// (ブラウザを開いてリダイレクトを受け取る等)をここから起動する。
#[tauri::command]
async fn sign_in_with_discord() -> Result<String, String> {
    match train_launcher_auth::discord::sign_in("TODO_DISCORD_CLIENT_ID").await {
        Ok(_token) => Ok("Discordでサインインしました".to_string()),
        Err(err) => Err(format!("Discordサインインは未実装です ({err})")),
    }
}

/// Microsoftアカウント(MSA)でサインインする。
///
/// TODO: `train_launcher_auth::msa::sign_in` → `train_launcher_auth::xbox::exchange_microsoft_token`
/// の実装完了後、実際のMSA→XboxLive→XSTS→Minecraftトークン変換フローをここから起動する。
#[tauri::command]
async fn sign_in_with_microsoft() -> Result<String, String> {
    match train_launcher_auth::msa::sign_in("TODO_MSA_CLIENT_ID").await {
        Ok(_token) => Ok("Microsoftアカウントでサインインしました".to_string()),
        Err(err) => Err(format!("Microsoftサインインは未実装です ({err})")),
    }
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
            list_profiles,
            list_member_servers,
            resolve_mod_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
