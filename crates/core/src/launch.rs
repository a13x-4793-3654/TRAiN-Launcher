//! 起動コマンド(JVM引数・クラスパス・ゲーム引数)の構築とプロセス起動。
//!
//! `download::download_version_files` で対象バージョンをダウンロード済みであることが前提。
//! その戻り値([`crate::version_manifest::ResolvedVersion`]、エイリアス解決・Mod
//! ローダー継承マージ済みの完全なバージョン情報)をそのまま本モジュールの関数に渡すこと
//! (ディスク上のキャッシュJSONは再読み込みしない)。
//!
//! Minecraft自体はMicrosoftアカウントでのサインインが必須のため、`LaunchAuth` は常にMSA経由
//! (Xbox Live/XSTSを経てMinecraftトークンへ変換済み)のユーザー名・UUID・アクセストークンを
//! 期待する(Discordサインインはゲーム起動には使用できない)。

use std::collections::HashMap;
use std::path::Path;

use tokio::process::{Child, Command};

use crate::paths::LauncherPaths;
use crate::profile::Profile;
use crate::rules::{rules_allow, CurrentPlatform};
use crate::version_manifest::{ArgumentEntry, ArgumentValue, ResolvedVersion, VersionDetails};
use crate::CoreError;

/// 起動に必要な認証情報(Minecraftトークンへ変換済みのもの)。
#[derive(Debug, Clone)]
pub struct LaunchAuth {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
}

/// プロファイルからMinecraft起動コマンド(実行ファイル + 引数)を構築する。
///
/// `resolved_version` は [`crate::download::download_version_files`] の戻り値をそのまま渡す
/// (エイリアス解決・Fabric/Forge等のModローダー継承マージが完了した完全な情報)。
/// `game_dir` はMod・リソースパック・セーブデータ等の実際の保存先
/// ([`Profile::effective_game_dir`] で解決したもの)。バージョンjar・ライブラリ・アセットは
/// 引き続き `launcher_root` 側([`LauncherPaths`])を参照するため、`game_dir` の値に
/// 関わらず共通ディレクトリが再利用される。
pub fn build_launch_command(
    profile: &Profile,
    resolved_version: &ResolvedVersion,
    launcher_root: &Path,
    game_dir: &Path,
    auth: &LaunchAuth,
) -> Result<Vec<String>, CoreError> {
    let paths = LauncherPaths::new(launcher_root);
    let version_id = &resolved_version.id;
    let details = &resolved_version.details;

    let platform = CurrentPlatform::detect();
    let classpath = build_classpath(&paths, version_id, details, platform)?;
    let natives_dir = paths.natives_dir(version_id).display().to_string();

    let mut placeholders: HashMap<&str, String> = HashMap::new();
    placeholders.insert("auth_player_name", auth.username.clone());
    placeholders.insert("version_name", details.id.clone());
    placeholders.insert("game_directory", game_dir.display().to_string());
    placeholders.insert("assets_root", paths.assets_dir().display().to_string());
    placeholders.insert("assets_index_name", details.assets.clone());
    placeholders.insert("auth_uuid", auth.uuid.clone());
    placeholders.insert("auth_access_token", auth.access_token.clone());
    // テレメトリ関連のプレースホルダー。この機能は未対応のため空文字列で埋める。
    placeholders.insert("clientid", String::new());
    placeholders.insert("auth_xuid", String::new());
    placeholders.insert("user_type", "msa".to_string());
    placeholders.insert("version_type", details.version_type.clone());
    placeholders.insert("natives_directory", natives_dir);
    placeholders.insert("launcher_name", "TRAiN-Launcher".to_string());
    placeholders.insert(
        "launcher_version",
        env!("CARGO_PKG_VERSION").to_string(),
    );
    placeholders.insert("classpath", classpath);

    let mut command = Vec::new();
    let java_path = profile
        .java_path
        .clone()
        .unwrap_or_else(|| "java".to_string());
    command.push(java_path);

    if let Some(max_memory) = profile.max_memory_mb {
        command.push(format!("-Xmx{max_memory}M"));
    }

    match &details.arguments {
        Some(arguments) => append_arguments(&mut command, &arguments.jvm, platform, &placeholders),
        None => {
            // 1.12以前: 構造化されたarguments.jvmが無いため、標準的な相当分を合成する。
            command.push(format!(
                "-Djava.library.path={}",
                placeholders["natives_directory"]
            ));
            command.push(format!(
                "-Dminecraft.launcher.brand={}",
                placeholders["launcher_name"]
            ));
            command.push(format!(
                "-Dminecraft.launcher.version={}",
                placeholders["launcher_version"]
            ));
            command.push("-cp".to_string());
            command.push(placeholders["classpath"].clone());
        }
    }

    command.push(details.main_class.clone());

    match &details.arguments {
        Some(arguments) => {
            append_arguments(&mut command, &arguments.game, platform, &placeholders);
        }
        None => {
            let template = details.minecraft_arguments.as_deref().unwrap_or_default();
            for token in template.split_whitespace() {
                command.push(substitute(token, &placeholders));
            }
        }
    }

    Ok(command)
}

/// 現在のOSに適用されるライブラリ + クライアントjar からクラスパス文字列を組み立てる。
fn build_classpath(
    paths: &LauncherPaths,
    version_id: &str,
    details: &VersionDetails,
    platform: CurrentPlatform,
) -> Result<String, CoreError> {
    let separator = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let mut entries = Vec::new();

    for library in &details.libraries {
        if !rules_allow(&library.rules, platform) {
            continue;
        }
        // Mojang形式(`downloads.artifact`)・Fabric/Quilt/Forge等が生成するフラット形式
        // (トップレベルの `url`)の両方に対応する。展開して java.library.path 経由で
        // 読み込むレガシーネイティブ(classifiers専用)は対象外([`maven::resolve_library_artifact`]
        // がこれらは `None` を返す)。
        if let Some(resolved) = crate::maven::resolve_library_artifact(library)? {
            entries.push(paths.library_path(&resolved.relative_path).display().to_string());
        }
    }

    entries.push(paths.version_jar_path(version_id).display().to_string());
    Ok(entries.join(separator))
}

fn append_arguments(
    command: &mut Vec<String>,
    entries: &[ArgumentEntry],
    platform: CurrentPlatform,
    placeholders: &HashMap<&str, String>,
) {
    for entry in entries {
        match entry {
            ArgumentEntry::Plain(value) => command.push(substitute(value, placeholders)),
            ArgumentEntry::Conditional { rules, value } => {
                if !rules_allow(&Some(rules.clone()), platform) {
                    continue;
                }
                match value {
                    ArgumentValue::Single(value) => command.push(substitute(value, placeholders)),
                    ArgumentValue::Multiple(values) => {
                        for value in values {
                            command.push(substitute(value, placeholders));
                        }
                    }
                }
            }
        }
    }
}

fn substitute(template: &str, placeholders: &HashMap<&str, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in placeholders {
        result = result.replace(&format!("${{{key}}}"), value);
    }
    result
}

/// 構築したコマンドでMinecraftプロセスを起動する。
///
/// `game_dir` はプロセスの作業ディレクトリとして使う(存在しない場合は事前に作成する)。
/// 呼び出し元が終了を待つか(あるいはログを継続的に読み取るか)判断できるよう、
/// 起動済みの `tokio::process::Child` をそのまま返す。
pub async fn launch(
    profile: &Profile,
    resolved_version: &ResolvedVersion,
    launcher_root: &Path,
    game_dir: &Path,
    auth: &LaunchAuth,
) -> Result<Child, CoreError> {
    let command = build_launch_command(profile, resolved_version, launcher_root, game_dir, auth)?;
    let (program, args) = command
        .split_first()
        .ok_or_else(|| CoreError::InvalidLaunchCommand("launch command is empty".to_string()))?;

    tokio::fs::create_dir_all(game_dir).await?;

    let child = Command::new(program)
        .args(args)
        .current_dir(game_dir)
        .spawn()?;
    Ok(child)
}

