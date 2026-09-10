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

/// アプリ全体の設定(Javaパス/ゲームディレクトリの上書き)を取得する。
#[tauri::command]
fn get_app_settings() -> Result<train_launcher_core::settings::AppSettings, String> {
    train_launcher_core::settings::load_settings().map_err(|err| err.to_string())
}

/// アプリ全体の設定を保存する。
#[tauri::command]
fn save_app_settings(
    settings: train_launcher_core::settings::AppSettings,
) -> Result<(), String> {
    train_launcher_core::settings::save_settings(&settings).map_err(|err| err.to_string())
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

/// 指定Minecraftバージョン向けに選択可能なModローダーの一覧
/// (`"fabric"`, `"quilt"`, `"forge"`, `"neoforge"`)。
///
/// 実際にそのバージョンにローダーが提供されているかまでは確認しない
/// (フロントエンドの選択肢を固定するための静的な一覧)。
#[tauri::command]
fn list_supported_mod_loaders() -> Vec<&'static str> {
    vec!["fabric", "quilt", "forge", "neoforge"]
}

/// 指定Modローダー・Minecraftバージョンの組み合わせで選択可能なローダーバージョン一覧を、
/// 新しい順で取得する(プロファイル作成/編集画面のCombobox用)。
#[tauri::command]
async fn list_mod_loader_versions(
    loader: String,
    game_version: String,
) -> Result<Vec<train_launcher_core::mod_loader::LoaderVersionInfo>, String> {
    let kind = train_launcher_core::mod_loader::ModLoaderKind::parse(&loader)
        .ok_or_else(|| format!("未対応のModローダーです: {loader}"))?;
    train_launcher_core::mod_loader::list_loader_versions(kind, &game_version)
        .await
        .map_err(|err| err.to_string())
}

/// Discordサインイン後の所属サーバー一覧を取得する。
///
/// 環境変数 `TRAIN_LAUNCHER_API_BASE_URL` が設定されていれば実際のTRAiN APIへ、未設定なら
/// モック実装(`MockTrainApiClient`)へフォールバックする(`train_launcher_server_api::create_client`)。
#[tauri::command]
async fn list_member_servers() -> Result<Vec<train_launcher_server_api::MemberServer>, String> {
    let discord_token = store::load_token(Provider::Discord)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "Discordアカウントでサインインしてください".to_string())?;
    // TODO: TRAiN API仕様確定後、Discordの実ユーザーIDをトークンに保持して使用する
    // (現状は表示名を仮のユーザー識別子として使っている)。
    let discord_user_id = discord_token.display_name.clone().unwrap_or_default();

    let client = train_launcher_server_api::create_client(Some(discord_token.access_token));
    client
        .get_member_servers(&discord_user_id)
        .await
        .map_err(|err| err.to_string())
}

/// 指定サーバーの設定(接続先・Minecraftバージョン・Modローダー・Mod/リソースパックURL一覧)
/// を取得する(「所属サーバー詳細」画面用。参加前にMod構成を確認できるようにする)。
#[tauri::command]
async fn get_server_config(
    server_id: String,
) -> Result<train_launcher_server_api::ServerConfig, String> {
    let discord_access_token = store::load_token(Provider::Discord)
        .map_err(|err| err.to_string())?
        .map(|record| record.access_token);
    let client = train_launcher_server_api::create_client(discord_access_token);
    client
        .get_server_config(&server_id)
        .await
        .map_err(|err| err.to_string())
}


/// フロントエンドへ返す、解決済みMod/リソースパックファイルの情報。
#[derive(Debug, Clone, Serialize)]
struct ResolvedModPayload {
    provider: String,
    project_name: String,
    filename: String,
    /// リクエストされたURL自体が解決された結果は `false`、依存関係として解決された
    /// 結果は `true`。
    is_dependency: bool,
}

impl From<(bool, train_launcher_mods::resolver::ResolvedFile)> for ResolvedModPayload {
    fn from((is_dependency, resolved): (bool, train_launcher_mods::resolver::ResolvedFile)) -> Self {
        let provider = match resolved.provider {
            train_launcher_mods::resolver::ModProvider::Modrinth => "modrinth",
            train_launcher_mods::resolver::ModProvider::CurseForge => "curseforge",
        };
        ResolvedModPayload {
            provider: provider.to_string(),
            project_name: resolved.project_name,
            filename: resolved.filename,
            is_dependency,
        }
    }
}

/// 現在設定されているCurseForge APIキー(環境変数から読み込み、未設定なら `None`)。
///
/// CurseForgeのURLを解決する場合にのみ必要。Modrinthのみで完結する場合は不要。
fn optional_curseforge_api_key() -> Option<String> {
    train_launcher_mods::config::curseforge_api_key_from_env().ok()
}

/// 実際に使用するゲームディレクトリを返す。設定画面で上書きされていればそちらを、
/// なければ公式Minecraft Launcherと共有する既定の `.minecraft` 相当ディレクトリを返す。
fn minecraft_root() -> std::path::PathBuf {
    let settings = train_launcher_core::settings::load_settings().unwrap_or_default();
    train_launcher_core::paths::effective_minecraft_root(settings.game_directory.as_deref())
}

/// Mod・リソースパックのインストール先ディレクトリを解決する。
///
/// `profile_id` が指定されている場合はそのプロファイルの
/// [`train_launcher_core::profile::Profile::effective_game_dir`] を、未指定の場合は
/// 全プロファイル共通の `.minecraft` 相当ディレクトリ([`minecraft_root`])を返す。
fn resolve_game_dir(profile_id: Option<&str>) -> Result<std::path::PathBuf, String> {
    match profile_id {
        Some(id) => {
            let profile =
                train_launcher_core::profile::get_profile(id).map_err(|err| err.to_string())?;
            Ok(profile.effective_game_dir(&minecraft_root()))
        }
        None => Ok(minecraft_root()),
    }
}

/// 指定ディレクトリ内のファイル名一覧を返す(ディレクトリが存在しない場合は空リスト)。
async fn list_directory_files(dir: &std::path::Path) -> Result<Vec<String>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = tokio::fs::read_dir(dir).await.map_err(|err| err.to_string())?;
    let mut filenames = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|err| err.to_string())? {
        if entry.file_type().await.map(|ft| ft.is_file()).unwrap_or(false) {
            if let Some(name) = entry.file_name().to_str() {
                filenames.push(name.to_string());
            }
        }
    }
    filenames.sort();
    Ok(filenames)
}

/// 指定ディレクトリ内のファイルを削除する。パストラバーサル対策として、`filename` に
/// 区切り文字(`/`、`\`)や `..` が含まれる場合はエラーを返す。
async fn remove_installed_file(dir: &std::path::Path, filename: &str) -> Result<(), String> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err("不正なファイル名です".to_string());
    }
    tokio::fs::remove_file(dir.join(filename))
        .await
        .map_err(|err| err.to_string())
}

/// URL指定でModを解決する(プレビューのみ、ダウンロードは行わない)。
///
/// 依存関係(`Required`)も再帰的に解決し、リクエストしたMod自身を先頭とした一覧を返す。
#[tauri::command]
async fn resolve_mod_url(
    url: String,
    minecraft_version: Option<String>,
) -> Result<Vec<ResolvedModPayload>, String> {
    use train_launcher_mods::resolver::{resolve_dependencies, ModReference, ResolveFilter};

    let filter = ResolveFilter {
        minecraft_version,
        mod_loader: None,
    };
    let resolved = resolve_dependencies(
        vec![ModReference { url }],
        &filter,
        optional_curseforge_api_key().as_deref(),
    )
    .await
    .map_err(|err| err.to_string())?;

    Ok(resolved
        .into_iter()
        .enumerate()
        .map(|(index, file)| ResolvedModPayload::from((index > 0, file)))
        .collect())
}

/// 複数URLをまとめて解決する(「Mod導入ウィザード」用)。URLごとに依存関係を解決した上で、
/// 提供元+プロジェクトIDが重複するファイル(同じMod・共通の依存Modなど)は1件にまとめる。
///
/// 戻り値は、いずれかのURLに対して直接指定されたファイルは `is_dependency: false`、
/// 依存関係としてのみ解決されたファイルは `is_dependency: true` となる。
async fn resolve_mod_urls_merged(
    urls: &[String],
    minecraft_version: Option<&str>,
) -> Result<Vec<(bool, train_launcher_mods::resolver::ResolvedFile)>, String> {
    use std::collections::HashSet;
    use train_launcher_mods::resolver::{resolve_dependencies, ModReference, ResolveFilter};

    let filter = ResolveFilter {
        minecraft_version: minecraft_version.map(|value| value.to_string()),
        mod_loader: None,
    };
    let curseforge_api_key = optional_curseforge_api_key();

    let mut merged = Vec::new();
    let mut seen = HashSet::new();
    for url in urls {
        let resolved = resolve_dependencies(
            vec![ModReference { url: url.clone() }],
            &filter,
            curseforge_api_key.as_deref(),
        )
        .await
        .map_err(|err| err.to_string())?;

        for (index, file) in resolved.into_iter().enumerate() {
            let key = (file.provider, file.project_id.clone());
            if !seen.insert(key) {
                continue;
            }
            merged.push((index > 0, file));
        }
    }
    Ok(merged)
}

/// 複数URLをまとめて解決する(プレビューのみ、ダウンロードは行わない)。
/// 「Mod導入ウィザード」で、複数のModを一括で導入内容の確認をする際に使用する。
#[tauri::command]
async fn resolve_mod_urls(
    urls: Vec<String>,
    minecraft_version: Option<String>,
) -> Result<Vec<ResolvedModPayload>, String> {
    let merged = resolve_mod_urls_merged(&urls, minecraft_version.as_deref()).await?;
    Ok(merged.into_iter().map(ResolvedModPayload::from).collect())
}

/// 複数URLをまとめてインストールする(「Mod導入ウィザード」用)。依存関係も含めて
/// ダウンロードし、重複するファイルは1回だけダウンロードする。
///
/// `profile_id` が指定されている場合はそのプロファイル専用のゲームディレクトリへ、
/// 未指定の場合は全プロファイル共通の `.minecraft/mods` ディレクトリへインストールする
/// ([`resolve_game_dir`])。
#[tauri::command]
async fn install_mods(
    urls: Vec<String>,
    minecraft_version: Option<String>,
    profile_id: Option<String>,
) -> Result<Vec<String>, String> {
    use train_launcher_mods::resolver::download_resolved_file;

    let merged = resolve_mod_urls_merged(&urls, minecraft_version.as_deref()).await?;
    let dest_dir = resolve_game_dir(profile_id.as_deref())?.join("mods");
    let mut installed = Vec::new();
    for (_, file) in &merged {
        download_resolved_file(file, &dest_dir)
            .await
            .map_err(|err| err.to_string())?;
        installed.push(file.filename.clone());
    }
    Ok(installed)
}

/// URL指定でModをインストールする(依存Modも含めてダウンロードする)。
///
/// `profile_id` が指定されている場合はそのプロファイル専用のゲームディレクトリへ、
/// 未指定の場合は全プロファイル共通の `.minecraft/mods` ディレクトリへインストールする
/// ([`resolve_game_dir`])。
#[tauri::command]
async fn install_mod(
    url: String,
    minecraft_version: Option<String>,
    profile_id: Option<String>,
) -> Result<Vec<String>, String> {
    use train_launcher_mods::resolver::{
        download_resolved_file, resolve_dependencies, ModReference, ResolveFilter,
    };

    let filter = ResolveFilter {
        minecraft_version,
        mod_loader: None,
    };
    let resolved = resolve_dependencies(
        vec![ModReference { url }],
        &filter,
        optional_curseforge_api_key().as_deref(),
    )
    .await
    .map_err(|err| err.to_string())?;

    let dest_dir = resolve_game_dir(profile_id.as_deref())?.join("mods");
    let mut installed = Vec::new();
    for file in &resolved {
        download_resolved_file(file, &dest_dir)
            .await
            .map_err(|err| err.to_string())?;
        installed.push(file.filename.clone());
    }
    Ok(installed)
}

/// インストール済みMod(`mods` 直下のファイル)一覧を返す。
/// `profile_id` 省略時は全プロファイル共通のディレクトリを参照する。
#[tauri::command]
async fn list_installed_mods(profile_id: Option<String>) -> Result<Vec<String>, String> {
    let dir = resolve_game_dir(profile_id.as_deref())?.join("mods");
    list_directory_files(&dir).await
}

/// インストール済みModを削除する。`profile_id` 省略時は全プロファイル共通のディレクトリを
/// 参照する。
#[tauri::command]
async fn remove_installed_mod(
    filename: String,
    profile_id: Option<String>,
) -> Result<(), String> {
    let dir = resolve_game_dir(profile_id.as_deref())?.join("mods");
    remove_installed_file(&dir, &filename).await
}

/// URL指定でリソースパックをインストールする。
///
/// `profile_id` が指定されている場合はそのプロファイル専用のゲームディレクトリへ、
/// 未指定の場合は全プロファイル共通の `.minecraft/resourcepacks` ディレクトリへ
/// インストールする([`resolve_game_dir`])。
#[tauri::command]
async fn install_resource_pack(
    url: String,
    minecraft_version: Option<String>,
    profile_id: Option<String>,
) -> Result<String, String> {
    let dest_dir = resolve_game_dir(profile_id.as_deref())?.join("resourcepacks");
    let path = train_launcher_mods::resource_pack::install_from_url(
        &url,
        minecraft_version.as_deref(),
        optional_curseforge_api_key().as_deref(),
        &dest_dir,
    )
    .await
    .map_err(|err| err.to_string())?;

    Ok(path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string())
}

/// インストール済みリソースパック(`resourcepacks` 直下のファイル)一覧を返す。
/// `profile_id` 省略時は全プロファイル共通のディレクトリを参照する。
#[tauri::command]
async fn list_installed_resource_packs(
    profile_id: Option<String>,
) -> Result<Vec<String>, String> {
    let dir = resolve_game_dir(profile_id.as_deref())?.join("resourcepacks");
    list_directory_files(&dir).await
}

/// インストール済みリソースパックを削除する。`profile_id` 省略時は全プロファイル共通の
/// ディレクトリを参照する。
#[tauri::command]
async fn remove_installed_resource_pack(
    filename: String,
    profile_id: Option<String>,
) -> Result<(), String> {
    let dir = resolve_game_dir(profile_id.as_deref())?.join("resourcepacks");
    remove_installed_file(&dir, &filename).await
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

/// プロファイルを指定してMinecraftをダウンロード(未取得分のみ)した上で起動する
/// (`launch_minecraft`/`join_train_server` 共通の実装)。
///
/// Minecraft自体の起動にはMicrosoftアカウントでのサインインが必須(Discordサインインのみ
/// では起動できない)。ダウンロードは初回のみ発生し、2回目以降はSHA1が一致するファイルは
/// スキップされる。ダウンロード・起動の進行状況は `launch://progress` イベントで随時
/// フロントエンドへ通知する。起動後はプロセスの終了を待たずに即座に制御を返し、終了時に
/// `game://exited` イベントをフロントエンドへemitする。
///
/// ゲームデータは公式Minecraft Launcherと共有するディレクトリ
/// (`train_launcher_core::paths::default_minecraft_root`、`.minecraft` 相当)に保存する。
async fn launch_profile(
    app_handle: AppHandle,
    profile: train_launcher_core::profile::Profile,
) -> Result<(), String> {
    let mut profile = profile;
    if profile.java_path.is_none() {
        // プロファイルにJavaパスの指定が無い場合、設定画面で指定された既定のJavaパスを使う
        // (それも未指定ならPATH上の `java` を使う、という解決順は `launch::build_launch_command`
        // 側で行う)。
        let settings = train_launcher_core::settings::load_settings().unwrap_or_default();
        profile.java_path = settings.java_path;
    }

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

    // 公式Minecraft Launcherと同じ `.minecraft` 相当のディレクトリを共有する
    // (二重ダウンロードを避け、どちらのランチャーからでも同じファイルを再利用できるようにする)。
    let launcher_root = minecraft_root();

    // Modローダーが指定されている場合、`minecraft_version`(バニラ)を基準にローダーを
    // 自動導入し、以降のダウンロード/起動には導入後のバージョンID(例:
    // `fabric-loader-0.19.5-1.20.4`)を使う。未導入の場合のみ実際のインストールが走る。
    let game_version = if let Some(loader_name) = profile.mod_loader.clone() {
        let loader_kind = train_launcher_core::mod_loader::ModLoaderKind::parse(&loader_name)
            .ok_or_else(|| format!("未対応のModローダーです: {loader_name}"))?;

        let _ = app_handle.emit(
            LAUNCH_PROGRESS_EVENT,
            LaunchProgressPayload {
                phase: "installing_mod_loader",
                phase_label: "Modローダーを導入中...".to_string(),
                completed: 0,
                total: 0,
            },
        );

        let installer_java_path = profile
            .java_path
            .clone()
            .unwrap_or_else(|| "java".to_string());
        train_launcher_core::mod_loader::ensure_mod_loader_installed(
            loader_kind,
            &profile.minecraft_version,
            profile.mod_loader_version.as_deref(),
            &launcher_root,
            &installer_java_path,
        )
        .await
        .map_err(|err| err.to_string())?
    } else {
        profile.minecraft_version.clone()
    };

    let progress_handle = app_handle.clone();
    let on_progress: train_launcher_core::download::ProgressCallback =
        std::sync::Arc::new(move |progress| {
            let payload = LaunchProgressPayload::from(progress);
            if let Err(err) = progress_handle.emit(LAUNCH_PROGRESS_EVENT, payload) {
                eprintln!("failed to emit launch progress event: {err}");
            }
        });

    let resolved_version = train_launcher_core::download::download_version_files(
        &game_version,
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

    let game_dir = profile.effective_game_dir(&launcher_root);
    let mut child = train_launcher_core::launch::launch(
        &profile,
        &resolved_version,
        &launcher_root,
        &game_dir,
        &auth,
    )
    .await
    .map_err(|err| err.to_string())?;

    // ホーム画面の「最近使ったプロファイル」表示用に最終起動日時を記録する。
    // 記録に失敗してもゲーム自体の起動は継続させたいため、エラーはログ出力のみに留める。
    if let Err(err) = train_launcher_core::profile::mark_launched(&profile.id) {
        eprintln!("failed to record last launched profile: {err}");
    }

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

/// 指定プロファイルのMinecraftをダウンロード(未取得分のみ)した上で起動する。
#[tauri::command]
async fn launch_minecraft(app_handle: AppHandle, profile_id: String) -> Result<(), String> {
    let profile =
        train_launcher_core::profile::get_profile(&profile_id).map_err(|err| err.to_string())?;
    launch_profile(app_handle, profile).await
}

/// TRAiN管理サーバーへ参加する。
///
/// サーバー設定(`ServerConfig`、接続先/Minecraftバージョン/Modローダー/Mod・リソースパック
/// URL一覧)をTRAiN APIから取得し、そのサーバー専用のプロファイルを自動作成/更新した上で、
/// 指定されたMod・リソースパックを依存関係含めて解決・インストールし、最後に起動する。
/// 同じサーバーに再度参加した場合は、既存の同名プロファイルを更新する(サーバー側の
/// バージョン/Mod構成の変更を追従させるため)。
#[tauri::command]
async fn join_train_server(
    app_handle: AppHandle,
    server_id: String,
    server_name: String,
) -> Result<(), String> {
    use train_launcher_core::profile::{Profile, ProfileSource};
    use train_launcher_mods::resolver::{
        download_resolved_file, resolve_dependencies, ModReference, ResolveFilter,
    };

    let _ = app_handle.emit(
        LAUNCH_PROGRESS_EVENT,
        LaunchProgressPayload {
            phase: "fetching_server_config",
            phase_label: "サーバー設定を取得中...".to_string(),
            completed: 0,
            total: 0,
        },
    );

    let discord_access_token = store::load_token(Provider::Discord)
        .map_err(|err| err.to_string())?
        .map(|record| record.access_token);
    let client = train_launcher_server_api::create_client(discord_access_token);
    let server_config = client
        .get_server_config(&server_id)
        .await
        .map_err(|err| err.to_string())?;

    // サーバーごとに固定のプロファイルIDを使う(再度参加した場合は同じプロファイルを更新し、
    // サーバー側の設定変更をそのまま反映する)。
    // TRAiNサーバーごとに要求されるMod構成は異なるため、サーバーごとに専用のゲーム
    // ディレクトリを自動割り当てし、他のサーバー/プロファイルとMod・リソースパックが
    // 混在しないようにする(バージョンjar・ライブラリ・アセットは引き続き共通ディレクトリを
    // 再利用する)。
    let game_dir = train_launcher_core::paths::default_launcher_root()
        .join("server-profiles")
        .join(&server_id)
        .display()
        .to_string();
    let profile = Profile {
        id: format!("train-{server_id}"),
        name: server_name,
        minecraft_version: server_config.minecraft_version.clone(),
        mod_loader: server_config.mod_loader.clone(),
        // TRAiNサーバー設定には現状ローダーの具体バージョンを指定する項目が無いため、
        // 常に最新の安定版を自動選択する。
        mod_loader_version: None,
        server_id: Some(server_id),
        game_dir: Some(game_dir),
        java_path: None,
        max_memory_mb: None,
        source: ProfileSource::Train,
        last_launched_at: None,
    };
    match train_launcher_core::profile::create_profile(profile.clone()) {
        Ok(()) => {}
        Err(train_launcher_core::CoreError::ProfileAlreadyExists(_)) => {
            train_launcher_core::profile::update_profile(profile.clone())
                .map_err(|err| err.to_string())?;
        }
        Err(err) => return Err(err.to_string()),
    }

    let filter = ResolveFilter {
        minecraft_version: Some(server_config.minecraft_version.clone()),
        mod_loader: server_config.mod_loader.clone(),
    };
    let curseforge_api_key = optional_curseforge_api_key();
    let profile_game_dir = profile.effective_game_dir(&minecraft_root());

    if !server_config.mod_urls.is_empty() {
        let references = server_config
            .mod_urls
            .iter()
            .map(|url| ModReference { url: url.clone() })
            .collect();
        let resolved = resolve_dependencies(references, &filter, curseforge_api_key.as_deref())
            .await
            .map_err(|err| err.to_string())?;

        let dest_dir = profile_game_dir.join("mods");
        let total = resolved.len();
        for (index, file) in resolved.iter().enumerate() {
            let _ = app_handle.emit(
                LAUNCH_PROGRESS_EVENT,
                LaunchProgressPayload {
                    phase: "installing_mods",
                    phase_label: format!("Modを導入中({}/{total})", index + 1),
                    completed: index,
                    total,
                },
            );
            download_resolved_file(file, &dest_dir)
                .await
                .map_err(|err| err.to_string())?;
        }
    }

    if !server_config.resource_pack_urls.is_empty() {
        let dest_dir = profile_game_dir.join("resourcepacks");
        let total = server_config.resource_pack_urls.len();
        for (index, url) in server_config.resource_pack_urls.iter().enumerate() {
            let _ = app_handle.emit(
                LAUNCH_PROGRESS_EVENT,
                LaunchProgressPayload {
                    phase: "installing_resource_packs",
                    phase_label: format!("リソースパックを導入中({}/{total})", index + 1),
                    completed: index,
                    total,
                },
            );
            train_launcher_mods::resource_pack::install_from_url(
                url,
                Some(&server_config.minecraft_version),
                curseforge_api_key.as_deref(),
                &dest_dir,
            )
            .await
            .map_err(|err| err.to_string())?;
        }
    }

    launch_profile(app_handle, profile).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
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
            list_supported_mod_loaders,
            list_mod_loader_versions,
            list_member_servers,
            get_server_config,
            resolve_mod_url,
            resolve_mod_urls,
            install_mod,
            install_mods,
            list_installed_mods,
            remove_installed_mod,
            install_resource_pack,
            list_installed_resource_packs,
            remove_installed_resource_pack,
            launch_minecraft,
            join_train_server,
            get_app_settings,
            save_app_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
