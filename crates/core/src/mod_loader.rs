//! Fabric/Quilt/Forge/NeoForge Modローダーの自動導入。
//!
//! [`crate::profile::Profile::mod_loader`] にローダー名が指定されている場合、起動前に
//! (未導入であれば)対応するローダーを自動導入し、导入後のバージョンID
//! (`crate::download::download_version_files` へそのまま渡せるID)を返す。
//!
//! - **Fabric/Quilt**: 公式のMeta API(`meta.fabricmc.net`/`meta.quiltmc.org`)から
//!   ローダー一覧の取得・完全なバージョンJSON(`profile/json`)の取得までHTTPのみで完結する。
//!   取得したJSONをそのまま `<destination>/versions/<id>/<id>.json` に保存すればよく、
//!   外部プロセスの起動は不要。
//! - **Forge/NeoForge**: 公開されたシンプルなメタデータAPIが無いため、公式インストーラjarを
//!   ダウンロードし、`java -jar <installer> --installClient=<destination>`
//!   をヘッドレスモードで実行して導入する(GUIは表示されない)。バージョンJSONの命名規則は
//!   将来変更される可能性があるため、規則から予測したIDが存在すればそれを使い、
//!   存在しない場合は `versions/` ディレクトリの実行前後の差分から実際に生成されたIDを
//!   検出する(フォールバック)。
//!
//! いずれの方式で導入したバージョンJSONも、通常のMojang形式のバージョンJSON
//! (`inheritsFrom` で親バージョンを参照する形式)であるため、以降のダウンロード・起動処理
//! ([`crate::download::download_version_files`]・[`crate::launch::launch`])は本モジュールを
//! 意識する必要はない。

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::paths::LauncherPaths;
use crate::CoreError;

/// 対応するModローダーの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModLoaderKind {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl ModLoaderKind {
    /// [`crate::profile::Profile::mod_loader`] に保存される文字列表現から解析する
    /// (大文字小文字を区別しない)。未知の文字列の場合は `None`。
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "fabric" => Some(Self::Fabric),
            "quilt" => Some(Self::Quilt),
            "forge" => Some(Self::Forge),
            "neoforge" => Some(Self::NeoForge),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
        }
    }
}

/// 選択可能なローダーバージョン1件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersionInfo {
    pub version: String,
    /// 安定版かどうか(Fabric/Quilt: メタデータの `stable` フラグ、
    /// Forge: `promotions_slim.json` の `recommended` に該当、
    /// NeoForge: バージョン文字列に `-beta`/`-alpha` を含まない)。
    pub stable: bool,
}

const FABRIC_META_BASE: &str = "https://meta.fabricmc.net/v2/versions/loader";
const QUILT_META_BASE: &str = "https://meta.quiltmc.org/v3/versions/loader";
const FORGE_PROMOTIONS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const NEOFORGE_VERSIONS_URL: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

/// 指定Minecraftバージョンに対して選択可能なローダーバージョン一覧を、新しい順で返す。
///
/// - Fabric/Quilt: 全ビルドを返す(Meta APIがそのまま提供)。
/// - Forge: `promotions_slim.json` に掲載されている「推奨版」「最新版」のみを返す
///   (全ビルド一覧を提供する軽量な公式APIが無いため)。他のビルドを使いたい場合は
///   フロントエンドの自由入力で直接バージョン文字列を指定できる。
/// - NeoForge: 命名規則(`{MC上2桁}.{ビルド番号}`)から該当バージョンを絞り込んで返す。
pub async fn list_loader_versions(
    kind: ModLoaderKind,
    game_version: &str,
) -> Result<Vec<LoaderVersionInfo>, CoreError> {
    match kind {
        ModLoaderKind::Fabric => list_fabric_like_versions(FABRIC_META_BASE, game_version).await,
        ModLoaderKind::Quilt => list_fabric_like_versions(QUILT_META_BASE, game_version).await,
        ModLoaderKind::Forge => list_forge_versions(game_version).await,
        ModLoaderKind::NeoForge => list_neoforge_versions(game_version).await,
    }
}

#[derive(Debug, Deserialize)]
struct FabricLikeLoaderMeta {
    version: String,
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct FabricLikeEntry {
    loader: FabricLikeLoaderMeta,
}

async fn list_fabric_like_versions(
    base_url: &str,
    game_version: &str,
) -> Result<Vec<LoaderVersionInfo>, CoreError> {
    let url = format!("{base_url}/{game_version}");
    let entries: Vec<FabricLikeEntry> = reqwest::get(&url)
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(entries
        .into_iter()
        .map(|entry| LoaderVersionInfo {
            version: entry.loader.version,
            stable: entry.loader.stable,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct ForgePromotions {
    promos: std::collections::HashMap<String, String>,
}

async fn list_forge_versions(game_version: &str) -> Result<Vec<LoaderVersionInfo>, CoreError> {
    let promotions: ForgePromotions = reqwest::get(FORGE_PROMOTIONS_URL)
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut versions = Vec::new();
    if let Some(recommended) = promotions.promos.get(&format!("{game_version}-recommended")) {
        versions.push(LoaderVersionInfo {
            version: recommended.clone(),
            stable: true,
        });
    }
    if let Some(latest) = promotions.promos.get(&format!("{game_version}-latest")) {
        // 推奨版と同一の場合は重複させない。
        if !versions.iter().any(|entry| &entry.version == latest) {
            versions.push(LoaderVersionInfo {
                version: latest.clone(),
                stable: false,
            });
        }
    }
    Ok(versions)
}

/// NeoForgeのバージョン番号は先頭の `"1."` を省いた `{マイナー}.{パッチ}` から始まる
/// (例: Minecraft `"1.20.4"` → `"20.4.xxx"`、`"1.21"` → `"21.0.xxx"`)。
fn neoforge_prefix(game_version: &str) -> Option<String> {
    let rest = game_version.strip_prefix("1.")?;
    let mut parts = rest.splitn(2, '.');
    let minor = parts.next()?;
    let patch = parts.next().unwrap_or("0");
    Some(format!("{minor}.{patch}"))
}

#[derive(Debug, Deserialize)]
struct NeoForgeVersionsResponse {
    versions: Vec<String>,
}

async fn list_neoforge_versions(game_version: &str) -> Result<Vec<LoaderVersionInfo>, CoreError> {
    let prefix = neoforge_prefix(game_version).ok_or_else(|| CoreError::ModLoaderVersionNotFound {
        loader: ModLoaderKind::NeoForge.as_str().to_string(),
        game_version: game_version.to_string(),
    })?;
    let prefix_with_dot = format!("{prefix}.");

    let response: NeoForgeVersionsResponse = reqwest::get(NEOFORGE_VERSIONS_URL)
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut matches: Vec<LoaderVersionInfo> = response
        .versions
        .into_iter()
        .filter(|version| version.starts_with(&prefix_with_dot))
        .map(|version| {
            let stable = !version.contains("-beta") && !version.contains("-alpha");
            LoaderVersionInfo { version, stable }
        })
        .collect();
    matches.sort_by(|a, b| version_sort_key(&a.version).cmp(&version_sort_key(&b.version)));
    matches.reverse();
    Ok(matches)
}

/// バージョン文字列をドット/ハイフン区切りで数値化し、数値比較できるようにする
/// (文字列としての辞書順ソートでは `"20.4.100"` が `"20.4.9"` より前に来てしまうため)。
/// 数値でない部分(`-beta` 等)は `0` として扱う。
fn version_sort_key(version: &str) -> Vec<u64> {
    version
        .split(|c: char| c == '.' || c == '-')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

/// 指定Minecraftバージョン + ローダーの組み合わせに対する既定のローダーバージョンを選ぶ
/// (安定版があれば最初の安定版、無ければ一覧の先頭)。
fn pick_default_version(versions: &[LoaderVersionInfo]) -> Option<String> {
    versions
        .iter()
        .find(|entry| entry.stable)
        .or_else(|| versions.first())
        .map(|entry| entry.version.clone())
}

/// 指定Modローダーが対象のMinecraftバージョン向けに導入済みであることを保証する
/// (未導入であれば自動導入する)。戻り値は導入後のバージョンID
/// (そのまま [`crate::download::download_version_files`] に渡せる)。
///
/// `loader_version` を省略した場合は既定バージョン(安定版優先)を自動選択する。
/// `java_path` はForge/NeoForgeのインストーラjarを実行するために使う
/// (Fabric/Quiltの場合は未使用)。
pub async fn ensure_mod_loader_installed(
    kind: ModLoaderKind,
    game_version: &str,
    loader_version: Option<&str>,
    destination: &Path,
    java_path: &str,
) -> Result<String, CoreError> {
    let paths = LauncherPaths::new(destination);

    match kind {
        ModLoaderKind::Fabric | ModLoaderKind::Quilt => {
            let base_url = match kind {
                ModLoaderKind::Fabric => FABRIC_META_BASE,
                _ => QUILT_META_BASE,
            };
            let loader_version = resolve_loader_version(kind, game_version, loader_version).await?;

            // Fabric/Quilt自身のバージョンJSON命名規則は固定(`<loader>-loader-<loader_version>-<game_version>`)
            // だが、実際に使うIDは取得したJSON自身の `id` フィールドを信頼する
            // (将来的な規則変更にも耐えられるようにするため)。
            let profile_url = format!("{base_url}/{game_version}/{loader_version}/profile/json");
            let bytes = reqwest::get(&profile_url)
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            let id = value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    CoreError::ModLoaderInstallFailed(
                        "loader profile json is missing an 'id' field".to_string(),
                    )
                })?
                .to_string();

            let json_path = paths.version_json_path(&id);
            if let Some(parent) = json_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&json_path, &bytes).await?;
            Ok(id)
        }
        ModLoaderKind::Forge | ModLoaderKind::NeoForge => {
            let loader_version = resolve_loader_version(kind, game_version, loader_version).await?;
            ensure_installer_based_loader_installed(kind, game_version, &loader_version, &paths, java_path)
                .await
        }
    }
}

async fn resolve_loader_version(
    kind: ModLoaderKind,
    game_version: &str,
    requested: Option<&str>,
) -> Result<String, CoreError> {
    if let Some(requested) = requested {
        return Ok(requested.to_string());
    }
    let versions = list_loader_versions(kind, game_version).await?;
    pick_default_version(&versions).ok_or_else(|| CoreError::ModLoaderVersionNotFound {
        loader: kind.as_str().to_string(),
        game_version: game_version.to_string(),
    })
}

/// Forge/NeoForgeが生成するバージョンID(命名規則から予測したもの)。
/// 実際のインストーラの挙動が規則から外れていた場合は、[`ensure_installer_based_loader_installed`]
/// が `versions/` ディレクトリの差分から実際のIDへフォールバックする。
fn predicted_installer_version_id(kind: ModLoaderKind, game_version: &str, loader_version: &str) -> String {
    match kind {
        ModLoaderKind::Forge => format!("{game_version}-forge-{loader_version}"),
        ModLoaderKind::NeoForge => format!("neoforge-{loader_version}"),
        _ => unreachable!("only Forge/NeoForge use installer-based install"),
    }
}

fn installer_jar_url(kind: ModLoaderKind, game_version: &str, loader_version: &str) -> String {
    match kind {
        ModLoaderKind::Forge => format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{game_version}-{loader_version}/forge-{game_version}-{loader_version}-installer.jar"
        ),
        ModLoaderKind::NeoForge => format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{loader_version}/neoforge-{loader_version}-installer.jar"
        ),
        _ => unreachable!("only Forge/NeoForge use installer-based install"),
    }
}

/// `versions/` 直下のバージョンID(ディレクトリ名)一覧を取得する(存在しなければ空集合)。
async fn list_installed_version_ids(paths: &LauncherPaths) -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(mut entries) = tokio::fs::read_dir(paths.versions_dir()).await else {
        return ids;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(file_type) = entry.file_type().await {
            if file_type.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    ids.insert(name.to_string());
                }
            }
        }
    }
    ids
}

/// Forge/NeoForge共通: 公式インストーラjarをダウンロードし、
/// `--installClient=<destination>` でヘッドレス導入する。
async fn ensure_installer_based_loader_installed(
    kind: ModLoaderKind,
    game_version: &str,
    loader_version: &str,
    paths: &LauncherPaths,
    java_path: &str,
) -> Result<String, CoreError> {
    let predicted_id = predicted_installer_version_id(kind, game_version, loader_version);
    if paths.version_json_path(&predicted_id).exists() {
        return Ok(predicted_id);
    }

    let installer_url = installer_jar_url(kind, game_version, loader_version);
    let installer_bytes = reqwest::get(&installer_url)
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let installer_dir = std::env::temp_dir().join("train-launcher-installers");
    tokio::fs::create_dir_all(&installer_dir).await?;
    let installer_path = installer_dir.join(format!(
        "{}-{}-{}-installer.jar",
        kind.as_str(),
        game_version,
        loader_version
    ));
    tokio::fs::write(&installer_path, &installer_bytes).await?;

    tokio::fs::create_dir_all(paths.root()).await?;
    let versions_before = list_installed_version_ids(paths).await;

    let install_arg = format!("--installClient={}", paths.root().display());
    let status = Command::new(java_path)
        .arg("-jar")
        .arg(&installer_path)
        .arg(&install_arg)
        .status()
        .await
        .map_err(|err| {
            CoreError::ModLoaderInstallFailed(format!(
                "failed to launch loader installer ({java_path}): {err}"
            ))
        })?;

    // インストーラ本体は導入後は不要なため、ベストエフォートで削除する。
    let _ = tokio::fs::remove_file(&installer_path).await;

    if !status.success() {
        return Err(CoreError::ModLoaderInstallFailed(format!(
            "loader installer exited with status {status}"
        )));
    }

    if paths.version_json_path(&predicted_id).exists() {
        return Ok(predicted_id);
    }

    // 命名規則から予測したIDが見つからない場合、導入前後の `versions/` ディレクトリの
    // 差分から実際に生成されたバージョンIDを検出する。
    let versions_after = list_installed_version_ids(paths).await;
    versions_after
        .difference(&versions_before)
        .next()
        .cloned()
        .ok_or_else(|| {
            CoreError::ModLoaderInstallFailed(
                "installer completed but no new version was found under versions/".to_string(),
            )
        })
}
