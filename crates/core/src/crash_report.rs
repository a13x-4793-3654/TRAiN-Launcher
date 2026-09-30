//! クライアント側 Minecraft のクラッシュ検出と、送信前の下処理。
//!
//! ゲーム終了後に `game_dir` を調べ、今回の起動以降に書き出されたクラッシュの記録
//! (`crash-reports/crash-*-client.txt` か、JVM 自体が落ちたときの `hs_err_pid*.log`)を探す。
//! 見つかったものは TRAiN へ送る候補になるが、**送るかどうかは必ず本人に確認する**
//! (ここでは読み取りと伏せ字までしか行わない)。
//!
//! `hs_err_pid*.log` には Java の起動引数がそのまま残り、Minecraft のアクセストークン
//! (`--accessToken`)が含まれる。画面で見せる前・送る前に必ず [`redact`] を通すこと。

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 送る本文の上限(TRAiN 側の上限 1 MiB より少し小さくし、JSON のエスケープで膨らむ分を見込む)。
pub const MAX_CONTENT_BYTES: usize = 900 * 1024;
/// 添えるログの上限(TRAiN 側の上限 256 KiB より少し小さくする)。
pub const MAX_LOG_BYTES: usize = 200 * 1024;
/// 添えるログの行数(`latest.log` の末尾)。クラッシュ直前の様子が分かれば足りる。
pub const LOG_TAIL_LINES: usize = 300;
/// `latest.log` の末尾から読む量。長時間遊んだあとのログは数十 MiB になることがある。
const LOG_READ_BYTES: u64 = 1024 * 1024;

const TRUNCATED_MARKER: &str = "\n...(長すぎるため以降を省略しました)\n";

/// クラッシュの記録の種類。TRAiN API の `kind` の値と一致させる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashKind {
    /// Minecraft が書き出したクラッシュレポート(`crash-reports/crash-*-client.txt`)。
    CrashReport,
    /// JVM 自体の異常終了(`hs_err_pid*.log`)。ドライバーやネイティブライブラリ起因が多い。
    JvmCrash,
}

impl CrashKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CrashKind::CrashReport => "crash_report",
            CrashKind::JvmCrash => "jvm_crash",
        }
    }
}

/// 見つかったクラッシュ(伏せ字・切り詰め済み)。
#[derive(Debug, Clone)]
pub struct DetectedCrash {
    pub kind: CrashKind,
    pub file_name: String,
    pub path: PathBuf,
    /// ファイルの更新日時(=クラッシュした時刻とみなす)。
    pub crashed_at: SystemTime,
    pub content: String,
    pub log_excerpt: Option<String>,
    /// ログやレポートから見て、マルチプレイのサーバーへ接続中だったと考えられるか。
    ///
    /// ここでの判定は「明らかにシングルプレイ/タイトル画面でのクラッシュ」を送信候補から
    /// 外すための目安にすぎない。接続中だったかの最終判断は TRAiN 側が参加履歴で行う
    /// (SRV レコードや別名があるため、接続先のホスト名の一致はここでは確かめない)。
    pub connected_to_server: bool,
}

/// `since` 以降に書き出された、いちばん新しいクラッシュの記録を探す。
pub fn find_latest_crash_file(game_dir: &Path, since: SystemTime) -> Option<(CrashKind, PathBuf, SystemTime)> {
    let mut candidates: Vec<(CrashKind, PathBuf, SystemTime)> = Vec::new();

    let reports_dir = game_dir.join("crash-reports");
    if let Ok(entries) = fs::read_dir(&reports_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // サーバー側のクラッシュ(`-server.txt`)は、統合サーバー(シングルプレイ)のもの
            if name.starts_with("crash-") && name.ends_with("-client.txt") {
                if let Some(modified) = modified_since(&entry.path(), since) {
                    candidates.push((CrashKind::CrashReport, entry.path(), modified));
                }
            }
        }
    }

    if let Ok(entries) = fs::read_dir(game_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // JVM は作業ディレクトリ(= game_dir、launch.rs で設定)に書き出す
            if name.starts_with("hs_err_pid") && name.ends_with(".log") {
                if let Some(modified) = modified_since(&entry.path(), since) {
                    candidates.push((CrashKind::JvmCrash, entry.path(), modified));
                }
            }
        }
    }

    candidates.into_iter().max_by_key(|(_, _, modified)| *modified)
}

fn modified_since(path: &Path, since: SystemTime) -> Option<SystemTime> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let modified = metadata.modified().ok()?;
    (modified >= since).then_some(modified)
}

/// `since` 以降のクラッシュを探し、送信前の下処理(伏せ字・切り詰め)まで済ませて返す。
///
/// `secrets` には、アクセストークンなど文字列として確実に伏せたい値を渡す
/// (パターンでは拾いきれない形式で残っている場合に備える)。
pub fn detect_crash(
    game_dir: &Path,
    since: SystemTime,
    secrets: &[&str],
) -> std::io::Result<Option<DetectedCrash>> {
    let Some((kind, path, crashed_at)) = find_latest_crash_file(game_dir, since) else {
        return Ok(None);
    };

    let raw = String::from_utf8_lossy(&fs::read(&path)?).into_owned();
    let log = read_log_tail(&game_dir.join("logs").join("latest.log"));
    let connected_to_server = looks_connected_to_server(&raw, log.as_deref());

    let home = dirs::home_dir();
    let content = truncate_bytes(&redact(&raw, home.as_deref(), secrets), MAX_CONTENT_BYTES);
    let log_excerpt = log.map(|text| {
        let tail = tail_lines(&text, LOG_TAIL_LINES);
        truncate_tail_bytes(&redact(&tail, home.as_deref(), secrets), MAX_LOG_BYTES)
    });

    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    Ok(Some(DetectedCrash {
        kind,
        file_name,
        path,
        crashed_at,
        content,
        log_excerpt,
        connected_to_server,
    }))
}

fn read_log_tail(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len > LOG_READ_BYTES {
        file.seek(SeekFrom::Start(len - LOG_READ_BYTES)).ok()?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// マルチプレイのサーバーへ接続中のクラッシュらしいか。
///
/// クラッシュレポートに接続先の種類が書かれていればそれを優先し、無ければ
/// `latest.log` の最後の「接続」「シングルプレイ開始」のどちらが後かで判断する。
pub fn looks_connected_to_server(crash_content: &str, latest_log: Option<&str>) -> bool {
    // クラッシュレポートの「-- Affected level --」などに現れる
    if crash_content.contains("Non-integrated multiplayer server") {
        return true;
    }
    if crash_content.contains("Integrated singleplayer server") {
        return false;
    }

    let Some(log) = latest_log else {
        return false;
    };
    // サーバーからの切断・接続失敗・タイトル画面への復帰で「接続中」を解除する
    const DISCONNECT_MARKERS: [&str; 7] = [
        "Client disconnected with reason",
        "Couldn't connect to server",
        "Disconnected from server",
        "Lost connection",
        "disconnect.lost",
        "Starting integrated minecraft server",
        "Stopping singleplayer server",
    ];
    let mut connected = false;
    for line in log.lines() {
        // ConnectScreen が接続開始時に出す(`Connecting to example.com, 25565`)
        if line.contains("Connecting to ") {
            connected = true;
        } else if DISCONNECT_MARKERS.iter().any(|marker| line.contains(marker)) {
            connected = false;
        }
    }
    connected
}

/// 送る前・見せる前に、個人を特定しうる値と認証情報を伏せる。
///
/// - `--accessToken` などの起動引数の値
/// - JWT 形式のトークン(Minecraft / Microsoft のアクセストークン)
/// - `secrets` に渡された値そのもの
/// - ホームフォルダーのパス(Windows のユーザー名が含まれる)と、`Users` / `home` 直下の名前
/// - hs_err の環境変数欄にあるユーザー名・コンピューター名
pub fn redact(text: &str, home: Option<&Path>, secrets: &[&str]) -> String {
    let mut result = text.to_string();

    for secret in secrets {
        // 短すぎる値は通常の語と衝突するため対象にしない
        if secret.len() >= 16 {
            result = result.replace(secret, "<redacted>");
        }
    }

    result = redact_flag_values(&result, &["--accessToken", "--session", "--xuid", "--clientId"]);
    result = redact_jwts(&result);

    if let Some(home) = home {
        let home = home.to_string_lossy();
        if home.len() > 3 {
            result = replace_case_insensitive(&result, &home, "~");
            let alternate = if home.contains('\\') {
                home.replace('\\', "/")
            } else {
                home.replace('/', "\\")
            };
            result = replace_case_insensitive(&result, &alternate, "~");
            // Java は `C:\\Users\\name` のように区切りを二重にして書くことがある
            result = replace_case_insensitive(&result, &home.replace('\\', "\\\\"), "~");
        }
    }
    result = redact_user_directories(&result);
    redact_environment_lines(&result)
}

fn redact_flag_values(text: &str, flags: &[&str]) -> String {
    let mut result = text.to_string();
    for flag in flags {
        let mut output = String::with_capacity(result.len());
        let mut rest = result.as_str();
        while let Some(index) = rest.find(flag) {
            let after_flag = index + flag.len();
            output.push_str(&rest[..after_flag]);
            let tail = &rest[after_flag..];
            // `--accessTokenX` のような別名は対象外。区切りの空白(または `=`)が続くものだけ
            let separator_len = tail
                .char_indices()
                .take_while(|(_, c)| *c == ' ' || *c == '\t' || *c == '=')
                .map(|(i, c)| i + c.len_utf8())
                .last()
                .unwrap_or(0);
            if separator_len == 0 {
                rest = tail;
                continue;
            }
            output.push_str(&tail[..separator_len]);
            let value = &tail[separator_len..];
            let value_len = value
                .find(|c: char| c.is_whitespace() || c == ',' || c == '"' || c == '\'')
                .unwrap_or(value.len());
            if value_len > 0 {
                output.push_str("<redacted>");
            }
            rest = &value[value_len..];
        }
        output.push_str(rest);
        result = output;
    }
    result
}

fn is_base64url(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// `eyJ` で始まり、`.` で区切られた 3 つの base64url の並び(JWT)を伏せる。
fn redact_jwts(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find("eyJ") {
        output.push_str(&rest[..index]);
        let candidate = &rest[index..];
        let len = jwt_length(candidate);
        if len > 0 {
            output.push_str("<redacted-token>");
            rest = &candidate[len..];
        } else {
            output.push_str("eyJ");
            rest = &candidate[3..];
        }
    }
    output.push_str(rest);
    output
}

fn jwt_length(candidate: &str) -> usize {
    let mut position = 0;
    for part in 0..3 {
        let segment = candidate[position..]
            .char_indices()
            .take_while(|(_, c)| is_base64url(*c))
            .map(|(i, c)| i + c.len_utf8())
            .last()
            .unwrap_or(0);
        if segment < 8 {
            return 0;
        }
        position += segment;
        if part < 2 {
            if !candidate[position..].starts_with('.') {
                return 0;
            }
            position += 1;
        }
    }
    position
}

fn replace_case_insensitive(text: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    // ASCII の大文字小文字だけを畳むため、バイト位置は元の文字列と一致する
    let mut output = String::with_capacity(text.len());
    let mut last = 0;
    let mut search_from = 0;
    while let Some(found) = lower_text[search_from..].find(&lower_needle) {
        let start = search_from + found;
        let end = start + needle.len();
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            search_from = start + 1;
            while !lower_text.is_char_boundary(search_from) {
                search_from += 1;
            }
            continue;
        }
        output.push_str(&text[last..start]);
        output.push_str(replacement);
        last = end;
        search_from = end;
    }
    output.push_str(&text[last..]);
    output
}

/// `\Users\<名前>` や `/home/<名前>` の `<名前>` を伏せる(ホームフォルダー以外のユーザーの分も含む)。
fn redact_user_directories(text: &str) -> String {
    let mut result = text.to_string();
    for marker in ["\\Users\\", "/Users/", "/home/", "\\\\Users\\\\"] {
        let mut output = String::with_capacity(result.len());
        let mut rest = result.as_str();
        while let Some(index) = find_ascii_case_insensitive(rest, marker) {
            let after = index + marker.len();
            output.push_str(&rest[..after]);
            let tail = &rest[after..];
            let name_len = tail
                .find(|c: char| {
                    c == '\\' || c == '/' || c.is_whitespace() || c == ';' || c == ':' || c == '"' || c == '\''
                })
                .unwrap_or(tail.len());
            let name = &tail[..name_len];
            // Windows 既定の共有フォルダーは個人名ではない
            if name_len > 0 && !name.eq_ignore_ascii_case("Public") && !name.eq_ignore_ascii_case("Default") {
                output.push_str("<user>");
            } else {
                output.push_str(name);
            }
            rest = &tail[name_len..];
        }
        output.push_str(rest);
        result = output;
    }
    result
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let lower = haystack.to_ascii_lowercase();
    lower
        .find(&needle.to_ascii_lowercase())
        .filter(|index| haystack.is_char_boundary(*index))
}

/// hs_err の「Environment Variables:」欄は未知の変数に秘密が入りうるため値をすべて伏せ、
/// 欄の外でもユーザー名・端末名の値は伏せる。
fn redact_environment_lines(text: &str) -> String {
    const KEYS: [&str; 7] = [
        "USERNAME=",
        "USER=",
        "LOGNAME=",
        "COMPUTERNAME=",
        "HOSTNAME=",
        "USERDOMAIN=",
        "USERDOMAIN_ROAMINGPROFILE=",
    ];
    let mut output = String::with_capacity(text.len());
    let mut in_environment = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let content = trimmed.trim_end();
        if content.eq_ignore_ascii_case("Environment Variables:") {
            in_environment = true;
            output.push_str(line);
            continue;
        }
        if content.is_empty() || content.starts_with("---") {
            in_environment = false;
        }
        let key_len = if in_environment {
            content.find('=').map(|index| index + 1)
        } else {
            KEYS.iter()
                .find(|key| trimmed.len() >= key.len() && trimmed[..key.len()].eq_ignore_ascii_case(key))
                .map(|key| key.len())
        };
        if let Some(key_len) = key_len {
            let indent = &line[..line.len() - trimmed.len()];
            let newline = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            output.push_str(indent);
            output.push_str(&trimmed[..key_len]);
            output.push_str("<redacted>");
            output.push_str(newline);
        } else {
            output.push_str(line);
        }
    }
    output
}

fn tail_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(count);
    lines[start..].join("\n")
}

/// 先頭から `max` バイトまでに収める(文字の途中では切らない)。
fn truncate_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max.saturating_sub(TRUNCATED_MARKER.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{TRUNCATED_MARKER}", &text[..end])
}

/// 末尾から `max` バイトまでに収める(ログはクラッシュ直前の末尾のほうが重要なため)。
fn truncate_tail_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let marker = "...(長すぎるため前半を省略しました)\n";
    let mut start = text.len() - max.saturating_sub(marker.len());
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    format!("{marker}{}", &text[start..])
}

/// [`SystemTime`] を TRAiN API が受け付ける ISO8601 (UTC、ミリ秒付き) にする。
pub fn to_iso8601(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// JWT の形をしたテスト用の値(検出ツールの誤検知を避けるため実行時に組み立てる)。
    fn fake_jwt() -> String {
        let segment = "x".repeat(24);
        format!("{}{segment}.{segment}.{segment}", ["ey", "J"].concat())
    }

    #[test]
    fn redacts_access_token_argument_and_jwt() {
        let text = format!(
            "java_command: net.minecraft.client.main.Main --username Steve --accessToken {jwt} --version 1.21.1\nother {jwt}", jwt = fake_jwt()
        );
        let redacted = redact(&text, None, &[]);
        assert!(!redacted.contains(&fake_jwt()));
        assert!(redacted.contains("--accessToken <redacted> --version 1.21.1"));
        assert!(redacted.contains("other <redacted-token>"));
        assert!(redacted.contains("--username Steve"));
    }

    #[test]
    fn redacts_explicit_secrets() {
        let secret = "opaque-token-value-1234567890";
        let redacted = redact(&format!("token={secret}"), None, &[secret]);
        assert_eq!(redacted, "token=<redacted>");
    }

    #[test]
    fn redacts_home_directory_and_user_names() {
        let home = Path::new("C:\\Users\\Taro");
        let text = "path C:\\Users\\Taro\\AppData\\x and c:/users/taro/y and C:\\\\Users\\\\Taro\\\\z and D:\\Users\\Hanako\\w and C:\\Users\\Public\\v";
        let redacted = redact(text, Some(home), &[]);
        assert!(!redacted.to_lowercase().contains("taro"), "{redacted}");
        assert!(!redacted.contains("Hanako"), "{redacted}");
        assert!(redacted.contains("~\\AppData\\x"));
        assert!(redacted.contains("C:\\Users\\Public\\v"));
    }

    #[test]
    fn redacts_environment_variables() {
        let text = "Environment Variables:\nPATH=C:\\bin\nSECRET_TOKEN=abc123\nUSERNAME=Taro\r\nCOMPUTERNAME=TARO-PC\n\nOther section\nMODE=keep\nUSER=taro\n";
        let redacted = redact(text, None, &[]);
        assert!(redacted.contains("USERNAME=<redacted>\r\n"));
        assert!(redacted.contains("COMPUTERNAME=<redacted>\n"));
        assert!(redacted.contains("SECRET_TOKEN=<redacted>\n"));
        assert!(redacted.contains("PATH=<redacted>\n"));
        assert!(!redacted.contains("abc123"));
        // 欄の外では既知のユーザー名系だけを伏せる
        assert!(redacted.contains("MODE=keep\n"));
        assert!(redacted.contains("USER=<redacted>\n"));
    }

    #[test]
    fn connection_detection_prefers_crash_report_details() {
        assert!(looks_connected_to_server("Server type: Non-integrated multiplayer server", None));
        assert!(!looks_connected_to_server(
            "Server type: Integrated singleplayer server",
            Some("Connecting to play.example.com, 25565")
        ));
    }

    #[test]
    fn connection_detection_uses_last_log_event() {
        let log = "[INFO]: Connecting to play.example.com, 25565\n[INFO]: Starting integrated minecraft server version 1.21.1\n";
        assert!(!looks_connected_to_server("", Some(log)));
        let log = "[INFO]: Starting integrated minecraft server version 1.21.1\n[INFO]: Stopping singleplayer server as player logged out\n[INFO]: Connecting to play.example.com, 25565\n";
        assert!(looks_connected_to_server("", Some(log)));
        assert!(!looks_connected_to_server("", Some("[INFO]: Setting user: Steve\n")));
        assert!(!looks_connected_to_server("", None));
        let log = "[INFO]: Connecting to play.example.com, 25565\n[INFO]: Client disconnected with reason: Disconnected\n[ERROR]: boom\n";
        assert!(!looks_connected_to_server("", Some(log)));
        let log = "[INFO]: Connecting to play.example.com, 25565\n[ERROR]: Couldn't connect to server\n";
        assert!(!looks_connected_to_server("", Some(log)));
    }

    #[test]
    fn truncation_keeps_char_boundaries() {
        let text = "あ".repeat(1000);
        let head = truncate_bytes(&text, 500);
        assert!(head.len() <= 500);
        assert!(head.ends_with(TRUNCATED_MARKER));
        let tail = truncate_tail_bytes(&text, 500);
        assert!(tail.len() <= 500 + 3);
        assert!(tail.ends_with('あ'));
    }

    #[test]
    fn detects_newest_crash_since_launch_and_ignores_older_ones() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path();
        fs::create_dir_all(game_dir.join("crash-reports")).unwrap();
        fs::create_dir_all(game_dir.join("logs")).unwrap();

        // 起動前からあった古いレポートは対象外
        fs::write(game_dir.join("crash-reports").join("crash-old-client.txt"), "old").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let since = SystemTime::now();
        std::thread::sleep(Duration::from_millis(100));

        assert!(detect_crash(game_dir, since, &[]).unwrap().is_none());

        fs::write(
            game_dir.join("crash-reports").join("crash-2026-01-01_00.00.00-client.txt"),
            format!("---- Minecraft Crash Report ----\nDescription: Ticking entity\n--accessToken {}\n", fake_jwt()),
        )
        .unwrap();
        // 統合サーバー側のレポートは送信候補にしない
        fs::write(game_dir.join("crash-reports").join("crash-2026-01-01_00.00.00-server.txt"), "server").unwrap();
        fs::write(
            game_dir.join("logs").join("latest.log"),
            "[INFO]: Connecting to play.example.com, 25565\n[ERROR]: boom\n",
        )
        .unwrap();

        let crash = detect_crash(game_dir, since, &[]).unwrap().expect("crash should be found");
        assert_eq!(crash.kind, CrashKind::CrashReport);
        assert_eq!(crash.file_name, "crash-2026-01-01_00.00.00-client.txt");
        assert!(crash.connected_to_server);
        assert!(!crash.content.contains(&fake_jwt()));
        assert!(crash.log_excerpt.unwrap().contains("boom"));
    }

    #[test]
    fn detects_jvm_crash_log() {
        let dir = tempfile::tempdir().unwrap();
        let since = SystemTime::now() - Duration::from_secs(5);
        fs::write(dir.path().join("hs_err_pid1234.log"), "# A fatal error has been detected").unwrap();
        let crash = detect_crash(dir.path(), since, &[]).unwrap().expect("crash should be found");
        assert_eq!(crash.kind, CrashKind::JvmCrash);
        assert_eq!(crash.kind.as_str(), "jvm_crash");
        assert!(!crash.connected_to_server);
    }

    #[test]
    fn iso8601_uses_utc_millis() {
        let time = SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        assert_eq!(to_iso8601(time), "2023-11-14T22:13:20.123Z");
    }
}
