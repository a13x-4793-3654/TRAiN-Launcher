//! TRAiNサーバーへ接続中に発生したMinecraftのクラッシュを検出し、
//! ユーザーの同意を得てからTRAiNへ送信する。
//!
//! 送信はユーザーが画面上の同意ボタンを押した場合([`submit_crash_report`])に限る。
//! 検出したレポートは送信・破棄されるまでメモリ上にのみ保持する(ランチャーを
//! 終了すると破棄される)。

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use train_launcher_core::crash_report::{self, DetectedCrash};
use train_launcher_server_api::CrashReportSubmission;

pub const CRASH_REPORT_DETECTED_EVENT: &str = "crash-report://detected";

/// 送信待ちのクラッシュレポート(フロントエンドへもこの形で渡す)。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingCrashReport {
    pub id: String,
    pub server_id: String,
    pub profile_name: String,
    pub kind: String,
    pub file_name: String,
    pub crashed_at: String,
    pub exit_code: Option<i32>,
    pub minecraft_version: String,
    pub mod_loader: Option<String>,
    pub launcher_version: String,
    /// 送信される本文(伏せ字・切り詰め済み)。同意画面でそのまま確認できるようにする。
    pub content: String,
    pub log_excerpt: Option<String>,
}

impl PendingCrashReport {
    fn to_submission(&self) -> CrashReportSubmission {
        CrashReportSubmission {
            kind: self.kind.clone(),
            file_name: self.file_name.clone(),
            crashed_at: self.crashed_at.clone(),
            content: self.content.clone(),
            log_excerpt: self.log_excerpt.clone(),
            exit_code: self.exit_code,
            minecraft_version: Some(self.minecraft_version.clone()),
            mod_loader: self.mod_loader.clone(),
            launcher_version: Some(self.launcher_version.clone()),
        }
    }
}

#[derive(Default)]
pub struct PendingCrashReports(Mutex<Vec<PendingCrashReport>>);

/// ゲーム終了後のクラッシュ検出に必要な起動時の情報。
pub struct CrashWatch {
    pub server_id: Option<String>,
    pub profile_name: String,
    pub minecraft_version: String,
    pub mod_loader: Option<String>,
    pub game_dir: PathBuf,
    pub launched_at: SystemTime,
    pub access_token: String,
}

impl CrashWatch {
    /// ゲーム終了後に呼び出す。TRAiNサーバー用プロファイルで、かつサーバー接続中の
    /// クラッシュが見つかった場合のみ送信待ちに登録し、フロントエンドへ通知する。
    pub async fn check_after_exit(self, app_handle: &AppHandle, exit_code: Option<i32>) {
        // 正常終了時はクラッシュとみなさない
        if exit_code == Some(0) {
            return;
        }
        let Some(server_id) = self.server_id.clone() else {
            return;
        };
        let game_dir = self.game_dir.clone();
        let since = self.launched_at;
        let token = self.access_token.clone();
        let detected = tauri::async_runtime::spawn_blocking(move || {
            crash_report::detect_crash(&game_dir, since, &[token.as_str()])
        })
        .await;
        let crash = match detected {
            Ok(Ok(Some(crash))) => crash,
            Ok(Ok(None)) => return,
            Ok(Err(err)) => {
                eprintln!("failed to read crash report: {err}");
                return;
            }
            Err(err) => {
                eprintln!("failed to detect crash report: {err}");
                return;
            }
        };
        if !crash.connected_to_server {
            return;
        }
        let pending = self.into_pending(server_id, crash, exit_code);
        let state = app_handle.state::<PendingCrashReports>();
        {
            let mut list = state.0.lock().unwrap_or_else(|err| err.into_inner());
            if list.iter().any(|item| item.file_name == pending.file_name && item.crashed_at == pending.crashed_at) {
                return;
            }
            list.push(pending.clone());
        }
        if let Err(err) = app_handle.emit(CRASH_REPORT_DETECTED_EVENT, &pending) {
            eprintln!("failed to emit crash report event: {err}");
        }
    }

    fn into_pending(self, server_id: String, crash: DetectedCrash, exit_code: Option<i32>) -> PendingCrashReport {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        PendingCrashReport {
            id: format!("{millis}-{}", crash.file_name),
            server_id,
            profile_name: self.profile_name,
            kind: crash.kind.as_str().to_string(),
            file_name: crash.file_name,
            crashed_at: crash_report::to_iso8601(crash.crashed_at),
            exit_code,
            minecraft_version: self.minecraft_version,
            mod_loader: self.mod_loader,
            launcher_version: env!("CARGO_PKG_VERSION").to_string(),
            content: crash.content,
            log_excerpt: crash.log_excerpt,
        }
    }
}

/// 送信待ちのクラッシュレポート一覧(画面の再読み込み時に同意画面を復元するため)。
#[tauri::command]
pub fn list_pending_crash_reports(state: State<'_, PendingCrashReports>) -> Vec<PendingCrashReport> {
    state.0.lock().unwrap_or_else(|err| err.into_inner()).clone()
}

/// ユーザーが同意したクラッシュレポートをTRAiNへ送信する。成功したら送信待ちから外す。
#[tauri::command]
pub async fn submit_crash_report(app_handle: AppHandle, id: String) -> Result<String, String> {
    let pending = {
        let state = app_handle.state::<PendingCrashReports>();
        let list = state.0.lock().unwrap_or_else(|err| err.into_inner());
        list.iter()
            .find(|item| item.id == id)
            .cloned()
            .ok_or_else(|| "送信するクラッシュレポートが見つかりません".to_string())?
    };
    let discord_access_token = crate::current_discord_token(&app_handle)
        .await?
        .ok_or_else(|| "Discordアカウントでサインインしてください".to_string())?
        .access_token;
    let client = train_launcher_server_api::create_client(Some(discord_access_token));
    let receipt = client
        .submit_crash_report(&pending.server_id, &pending.to_submission())
        .await
        .map_err(|err| err.to_string())?;
    remove(&app_handle, &id);
    Ok(receipt.id)
}

/// クラッシュレポートを送信せずに破棄する。
#[tauri::command]
pub fn dismiss_crash_report(app_handle: AppHandle, id: String) {
    remove(&app_handle, &id);
}

fn remove(app_handle: &AppHandle, id: &str) {
    let state = app_handle.state::<PendingCrashReports>();
    let mut list = state.0.lock().unwrap_or_else(|err| err.into_inner());
    list.retain(|item| item.id != id);
}
