//! TRAiN Launcher デスクトップアプリ(Tauri)本体。
//!
//! フロントエンド(React)から `@tauri-apps/api` の `invoke()` 経由で呼び出す
//! Tauri commandsをここに定義する。認証系commandは `train_launcher_auth` の実装を呼び出し、
//! 取得したトークンはOSの資格情報ストア(keyring)に保存する。
//! 実際にサインインを試すには Microsoft Entra ID / Discord Developer Portal でのアプリ登録と、
//! 対応する環境変数(`TRAIN_LAUNCHER_MS_CLIENT_ID` 等、詳細はREADME参照)の設定が必要。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_opener::OpenerExt;
use train_launcher_auth::store::{self, Provider, TokenRecord};
use train_launcher_auth::{config, discord, msa, xbox};

/// フロントエンドへ返すサインイン結果。
#[derive(Debug, Clone, Serialize)]
struct SignInResult {
    display_name: String,
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
            uuid: None,
        },
    )
    .map_err(|err| err.to_string())?;

    Ok(SignInResult {
        display_name: token.username,
    })
}

/// 埋め込みWebViewのMSAサインインポップアップの結果。
enum MsaSignInOutcome {
    /// リダイレクトから認可コード/stateを取得できた。
    Code { code: String, state: String },
    /// ユーザーがポップアップを閉じた(またはタイムアウト前に破棄された)。
    Cancelled,
    /// Microsoft側がエラーを返した(例: ユーザーが同意画面でキャンセルした場合の `access_denied`)。
    Error(String),
}

/// MSAサインイン用の埋め込みWebViewポップアップ("msa-signin"というラベルのウィンドウ)を開き、
/// [`msa::MSA_NATIVE_CLIENT_REDIRECT_URI`] へのナビゲーションを監視して結果を待つ。
///
/// Minecraft公式ランチャーと同様の、システムブラウザを介さないポップアップ型サインインUXを
/// 実現するための実装。リダイレクト先への実際のHTTPリクエストは発生させず(`on_navigation`が
/// `false` を返してナビゲーションをブロックする)、URLに含まれる `code`/`state`/`error` クエリ
/// パラメータのみを横取りする。
async fn open_msa_signin_popup(
    app_handle: &AppHandle,
    authorize_url: &str,
) -> Result<MsaSignInOutcome, String> {
    let url = authorize_url
        .parse::<tauri::Url>()
        .map_err(|err| format!("invalid authorize url: {err}"))?;

    let (tx, rx) = tokio::sync::oneshot::channel::<MsaSignInOutcome>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    let tx_navigation = tx.clone();
    let window = WebviewWindowBuilder::new(app_handle, "msa-signin", WebviewUrl::External(url))
        .title("Microsoftアカウントでサインイン")
        .inner_size(500.0, 650.0)
        .on_navigation(move |nav_url| {
            let nav_str = nav_url.as_str();
            if !nav_str.starts_with(msa::MSA_NATIVE_CLIENT_REDIRECT_URI) {
                return true;
            }
            if let Some(sender) = tx_navigation.lock().unwrap().take() {
                let mut code = None;
                let mut state = None;
                let mut error = None;
                for (key, value) in nav_url.query_pairs() {
                    match key.as_ref() {
                        "code" => code = Some(value.into_owned()),
                        "state" => state = Some(value.into_owned()),
                        "error_description" | "error" => error = Some(value.into_owned()),
                        _ => {}
                    }
                }
                let outcome = match (code, state, error) {
                    (Some(code), Some(state), _) => MsaSignInOutcome::Code { code, state },
                    (_, _, Some(error)) => MsaSignInOutcome::Error(error),
                    _ => MsaSignInOutcome::Error("認可コードを取得できませんでした".to_string()),
                };
                let _ = sender.send(outcome);
            }
            false
        })
        .build()
        .map_err(|err| err.to_string())?;

    let tx_close = tx.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            if let Some(sender) = tx_close.lock().unwrap().take() {
                let _ = sender.send(MsaSignInOutcome::Cancelled);
            }
        }
    });

    let outcome = tokio::time::timeout(Duration::from_secs(300), rx)
        .await
        .map_err(|_| "サインインがタイムアウトしました".to_string())?
        .unwrap_or(MsaSignInOutcome::Cancelled);

    // 認可コード取得済みの場合、ポップアップはまだ開いたままなので明示的に閉じる
    // (すでに閉じられている場合は失敗しても無視して問題ない)。
    let _ = window.close();

    Ok(outcome)
}

/// Microsoftアカウント(MSA)でサインインする。
///
/// 環境変数 `TRAIN_LAUNCHER_MS_CLIENT_ID` が必要。Minecraft公式ランチャーと同様、埋め込み
/// WebViewのポップアップでMicrosoftのサインイン画面を表示する(認可コードフロー + PKCE)。
/// MSAトークン取得後、続けてXboxLive/XSTSを経由してMinecraftトークンへ変換し、
/// Minecraftプレイヤー名/UUIDを取得する。
#[tauri::command]
async fn sign_in_with_microsoft(app_handle: AppHandle) -> Result<SignInResult, String> {
    let config = config::MicrosoftConfig::from_env().map_err(|err| err.to_string())?;
    let request = msa::build_authorization_request(&config);

    let outcome = open_msa_signin_popup(&app_handle, &request.authorize_url).await?;
    let (code, state) = match outcome {
        MsaSignInOutcome::Code { code, state } => (code, state),
        MsaSignInOutcome::Cancelled => return Err("サインインがキャンセルされました".to_string()),
        MsaSignInOutcome::Error(message) => return Err(message),
    };

    let msa_token = msa::exchange_authorization_code(&config, request, code, state)
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
            uuid: minecraft_token.uuid,
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

/// 保存済みプロファイル一覧を取得する。
#[tauri::command]
fn list_profiles() -> Result<Vec<train_launcher_core::profile::Profile>, String> {
    train_launcher_core::profile::list_profiles().map_err(|err| err.to_string())
}

/// 新規プロファイルを作成する。同じIDが既に存在する場合はエラーを返す。
#[tauri::command]
fn create_profile(profile: train_launcher_core::profile::Profile) -> Result<(), String> {
    train_launcher_core::profile::create_profile(profile).map_err(|err| err.to_string())
}

/// 既存プロファイルを更新する。存在しないIDの場合はエラーを返す。
#[tauri::command]
fn update_profile(profile: train_launcher_core::profile::Profile) -> Result<(), String> {
    train_launcher_core::profile::update_profile(profile).map_err(|err| err.to_string())
}

/// プロファイルを削除する。存在しないIDの場合はエラーを返す。
#[tauri::command]
fn delete_profile(id: String) -> Result<(), String> {
    train_launcher_core::profile::delete_profile(&id).map_err(|err| err.to_string())
}

/// Mojangのバージョンマニフェストから、選択可能なMinecraftバージョン一覧を取得する。
///
/// プロファイル作成/編集画面でのバージョン選択(Combobox)用。
#[tauri::command]
async fn list_minecraft_versions(
) -> Result<Vec<train_launcher_core::version_manifest::VersionEntry>, String> {
    train_launcher_core::version_manifest::fetch_version_manifest()
        .await
        .map(|manifest| manifest.versions)
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
/// TODO: `train_launcher_mods` の実装完了後、依存Modも含めた依存関係も返す。
#[tauri::command]
async fn resolve_mod_url(url: String) -> Result<(), String> {
    train_launcher_mods::modrinth::resolve_from_url(&url)
        .await
        .map_err(|err| err.to_string())
}

/// ゲーム終了時にフロントエンドへemitするイベント名。
const GAME_EXITED_EVENT: &str = "game://exited";

/// フロントエンドへemitするゲーム終了情報。
#[derive(Debug, Clone, Serialize)]
struct GameExitedPayload {
    /// プロセスの終了コード。強制終了などで取得できない場合は `None`。
    exit_code: Option<i32>,
}

/// ダウンロード/起動の進行状況をフロントエンドへemitするイベント名。
const LAUNCH_PROGRESS_EVENT: &str = "launch://progress";

/// フロントエンドへemitする進行状況情報。
///
/// `phase` はフロントエンド側での分岐用の機械可読な識別子、`phase_label` は
/// そのまま画面に表示できる日本語ラベル。`total` が0の場合は件数未確定を表す。
#[derive(Debug, Clone, Serialize)]
struct LaunchProgressPayload {
    phase: &'static str,
    phase_label: String,
    completed: usize,
    total: usize,
}

impl From<train_launcher_core::download::DownloadProgress> for LaunchProgressPayload {
    fn from(progress: train_launcher_core::download::DownloadProgress) -> Self {
        use train_launcher_core::download::DownloadPhase;
        let (phase, base_label) = match progress.phase {
            DownloadPhase::FetchingManifest => ("fetching_manifest", "バージョン情報を確認中"),
            DownloadPhase::ClientJar => ("client_jar", "クライアント本体をダウンロード中"),
            DownloadPhase::Libraries => ("libraries", "ライブラリをダウンロード中"),
            DownloadPhase::Assets => ("assets", "アセットをダウンロード中"),
        };
        let phase_label = if progress.total > 0 {
            format!("{base_label} ({}/{})", progress.completed, progress.total)
        } else {
            base_label.to_string()
        };
        LaunchProgressPayload {
            phase,
            phase_label,
            completed: progress.completed,
            total: progress.total,
        }
    }
}

/// 指定プロファイルのMinecraftをダウンロード(未取得分のみ)した上で起動する。
///
/// Minecraft自体の起動にはMicrosoftアカウントでのサインインが必須(Discordサインインのみ
/// では起動できない)。ダウンロードは初回のみ発生し、2回目以降はSHA1が一致するファイルは
/// スキップされる。ダウンロード・起動の進行状況は `launch://progress` イベントで随時
/// フロントエンドへ通知する。起動後はプロセスの終了を待たずに即座に制御を返し、終了時に
/// `game://exited` イベントをフロントエンドへemitする。
///
/// ゲームデータは公式Minecraft Launcherと共有するディレクトリ
/// (`train_launcher_core::paths::default_minecraft_root`、`.minecraft` 相当)に保存する。
#[tauri::command]
async fn launch_minecraft(app_handle: AppHandle, profile_id: String) -> Result<(), String> {
    let token = store::load_token(Provider::Microsoft)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "Microsoftアカウントでサインインしてください".to_string())?;
    let uuid = token.uuid.clone().ok_or_else(|| {
        "MinecraftのUUIDが取得できていません。サインアウトして再度サインインしてください"
            .to_string()
    })?;
    let username = token
        .display_name
        .clone()
        .unwrap_or_else(|| "Player".to_string());

    let profile =
        train_launcher_core::profile::get_profile(&profile_id).map_err(|err| err.to_string())?;

    // 公式Minecraft Launcherと同じ `.minecraft` 相当のディレクトリを共有する
    // (二重ダウンロードを避け、どちらのランチャーからでも同じファイルを再利用できるようにする)。
    let launcher_root = train_launcher_core::paths::default_minecraft_root();

    let progress_handle = app_handle.clone();
    let on_progress: train_launcher_core::download::ProgressCallback =
        std::sync::Arc::new(move |progress| {
            let payload = LaunchProgressPayload::from(progress);
            if let Err(err) = progress_handle.emit(LAUNCH_PROGRESS_EVENT, payload) {
                eprintln!("failed to emit launch progress event: {err}");
            }
        });

    train_launcher_core::download::download_version_files(
        &profile.minecraft_version,
        &launcher_root,
        on_progress,
    )
    .await
    .map_err(|err| err.to_string())?;

    let _ = app_handle.emit(
        LAUNCH_PROGRESS_EVENT,
        LaunchProgressPayload {
            phase: "launching",
            phase_label: "起動中...".to_string(),
            completed: 0,
            total: 0,
        },
    );

    let auth = train_launcher_core::launch::LaunchAuth {
        username,
        uuid,
        access_token: token.access_token,
    };

    let mut child = train_launcher_core::launch::launch(&profile, &launcher_root, &auth)
        .await
        .map_err(|err| err.to_string())?;

    let _ = app_handle.emit(
        LAUNCH_PROGRESS_EVENT,
        LaunchProgressPayload {
            phase: "launched",
            phase_label: "起動しました".to_string(),
            completed: 1,
            total: 1,
        },
    );

    tauri::async_runtime::spawn(async move {
        let exit_code = match child.wait().await {
            Ok(status) => status.code(),
            Err(_) => None,
        };
        if let Err(err) = app_handle.emit(GAME_EXITED_EVENT, GameExitedPayload { exit_code }) {
            eprintln!("failed to emit game exited event: {err}");
        }
    });

    Ok(())
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
            create_profile,
            update_profile,
            delete_profile,
            list_minecraft_versions,
            list_member_servers,
            resolve_mod_url,
            launch_minecraft,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
