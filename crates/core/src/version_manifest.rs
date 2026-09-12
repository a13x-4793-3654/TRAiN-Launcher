//! Mojangが公開するバージョンマニフェスト (version_manifest_v2.json) と、
//! 個々のバージョンの詳細情報(ライブラリ・ダウンロードURL・起動引数等)の取得。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::paths::LauncherPaths;
use crate::rules::Rule;
use crate::CoreError;

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// バージョンマニフェスト内の1バージョンエントリ。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

/// 最新リリース/スナップショットのバージョンID。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

/// バージョンマニフェスト全体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionEntry>,
}

/// Mojangのバージョンマニフェストを取得する。
pub async fn fetch_version_manifest() -> Result<VersionManifest, CoreError> {
    let client = reqwest::Client::new();
    let manifest = client
        .get(VERSION_MANIFEST_URL)
        .send()
        .await?
        .error_for_status()?
        .json::<VersionManifest>()
        .await?;
    Ok(manifest)
}

/// マニフェストから指定バージョンIDのエントリを探す。
pub fn find_version<'a>(
    manifest: &'a VersionManifest,
    version_id: &str,
) -> Option<&'a VersionEntry> {
    manifest.versions.iter().find(|entry| entry.id == version_id)
}

/// バージョン固有の詳細情報 (`https://piston-meta.mojang.com/.../<id>.json` の内容)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDetails {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexRef,
    pub assets: String,
    pub downloads: VersionDownloads,
    #[serde(default)]
    pub libraries: Vec<Library>,
    /// 1.13以降: 構造化された条件付き引数。
    #[serde(default)]
    pub arguments: Option<Arguments>,
    /// 1.12以前: スペース区切りの単純な引数文字列(プレースホルダーのみ、条件分岐は無い)。
    #[serde(default, rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, rename = "javaVersion")]
    pub java_version: Option<JavaVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    #[serde(default, rename = "totalSize")]
    pub total_size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDownloads {
    pub client: Artifact,
    #[serde(default)]
    pub server: Option<Artifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaVersion {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

/// ダウンロード可能な1ファイル(URL・SHA1・サイズ)。ライブラリの場合は加えて
/// `libraries/` からの相対パス(`path`)を持つ。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(default)]
    pub path: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    /// レガシー(LWJGL2世代)ネイティブライブラリ: OS名 → `downloads.classifiers` のキー。
    /// `${arch}` は32/64ビット表記に置換される。
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub extract: Option<ExtractRules>,
    /// Fabric/Quilt/Forge等のローダーが生成するバージョンJSONで使われる「フラット形式」の
    /// ライブラリ定義。`downloads` が存在しない場合、ここに入るMavenリポジトリのベースURLと
    /// `name` (Maven座標)からダウンロードURL・保存パスを合成する
    /// ([`crate::maven::resolve_library_artifact`] 参照)。
    #[serde(default)]
    pub url: Option<String>,
    /// フラット形式での期待SHA1(省略されることがある。その場合はハッシュ検証を行わない)。
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractRules {
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// JVM/ゲーム起動引数(1.13以降の構造化形式)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgumentEntry>,
    #[serde(default)]
    pub jvm: Vec<ArgumentEntry>,
}

/// 引数1要素。単純な文字列、または `rules` による条件付き値のいずれか。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentEntry {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgumentValue,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    Single(String),
    Multiple(Vec<String>),
}

/// アセットインデックス (`assets/indexes/<id>.json`) の内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// バージョンマニフェストのエントリからバージョン詳細JSONを取得する。
pub async fn fetch_version_details(entry: &VersionEntry) -> Result<VersionDetails, CoreError> {
    let client = reqwest::Client::new();
    let details = client
        .get(&entry.url)
        .send()
        .await?
        .error_for_status()?
        .json::<VersionDetails>()
        .await?;
    Ok(details)
}

/// アセットインデックスJSONを取得する。
pub async fn fetch_asset_index(asset_index: &AssetIndexRef) -> Result<AssetIndex, CoreError> {
    let client = reqwest::Client::new();
    let index = client
        .get(&asset_index.url)
        .send()
        .await?
        .error_for_status()?
        .json::<AssetIndex>()
        .await?;
    Ok(index)
}

/// ローカルに保存されたバージョンJSON(`inheritsFrom` を持つ場合がある)。
///
/// Fabric/Forge等のModローダーが導入するバージョンJSONは、Mojangのバージョンマニフェストには
/// 掲載されておらず、`<destination>/versions/<id>/<id>.json` としてローカルにのみ存在する
/// (公式Launcherやローダー自身のインストーラが作成する)。またその多くのフィールドは
/// `inheritsFrom` で指定した親バージョン(通常はバニラのバージョンID)からの継承に委ねる形で
/// 省略されているため、[`VersionDetails`] とは異なりほぼ全フィールドを省略可能として扱う。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialVersionDetails {
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "type")]
    pub version_type: Option<String>,
    #[serde(default, rename = "inheritsFrom")]
    pub inherits_from: Option<String>,
    #[serde(default, rename = "mainClass")]
    pub main_class: Option<String>,
    #[serde(default, rename = "assetIndex")]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default)]
    pub downloads: Option<VersionDownloads>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default, rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, rename = "javaVersion")]
    pub java_version: Option<JavaVersion>,
}

impl PartialVersionDetails {
    /// `inheritsFrom` を持たない(=単独で完結しているはずの)ローカルバージョンJSONを
    /// [`VersionDetails`] に変換する。起動に必須のフィールドが欠けている場合はエラーを返す。
    fn into_complete(self, fallback_id: &str) -> Result<VersionDetails, CoreError> {
        let id = if self.id.is_empty() {
            fallback_id.to_string()
        } else {
            self.id
        };
        Ok(VersionDetails {
            main_class: self
                .main_class
                .ok_or_else(|| CoreError::IncompleteVersionJson(id.clone()))?,
            asset_index: self
                .asset_index
                .ok_or_else(|| CoreError::IncompleteVersionJson(id.clone()))?,
            assets: self
                .assets
                .ok_or_else(|| CoreError::IncompleteVersionJson(id.clone()))?,
            downloads: self
                .downloads
                .ok_or_else(|| CoreError::IncompleteVersionJson(id.clone()))?,
            version_type: self.version_type.unwrap_or_else(|| "release".to_string()),
            libraries: self.libraries,
            arguments: self.arguments,
            minecraft_arguments: self.minecraft_arguments,
            java_version: self.java_version,
            id,
        })
    }
}

/// 継承元バージョン(`parent`、解決済み完全版)と子バージョン(`child`、省略フィールドを
/// 含む可能性がある)をマージし、完全な [`VersionDetails`] を組み立てる。
///
/// マージ規則(公式Launcherの `inheritsFrom` 解決仕様に準拠):
/// - `mainClass`/`assetIndex`/`assets`/`downloads`/`javaVersion`/`type` は子側に値があれば
///   それを優先し、無ければ親から引き継ぐ(Fabric/Forge導入バージョンは通常 `mainClass` 以外を
///   省略し、親=バニラ側の値をそのまま使う)
/// - `libraries` は親→子の順に連結するが、子側に同じMaven座標(`group:artifact`、バージョン
///   部分を除く)のエントリがあれば親側のエントリを取り除く(Forgeが一部ライブラリの
///   バージョンを上書きするケースに対応)
/// - `arguments`(1.13以降の構造化引数)は親→子の順に連結する
fn merge_with_parent(child: PartialVersionDetails, parent: VersionDetails) -> VersionDetails {
    let id = if child.id.is_empty() {
        parent.id.clone()
    } else {
        child.id
    };
    let arguments = match (parent.arguments, child.arguments) {
        (Some(mut merged), Some(extra)) => {
            merged.game.extend(extra.game);
            merged.jvm.extend(extra.jvm);
            Some(merged)
        }
        (parent_args, child_args) => child_args.or(parent_args),
    };

    VersionDetails {
        id,
        version_type: child.version_type.unwrap_or(parent.version_type),
        main_class: child.main_class.unwrap_or(parent.main_class),
        asset_index: child.asset_index.unwrap_or(parent.asset_index),
        assets: child.assets.unwrap_or(parent.assets),
        downloads: child.downloads.unwrap_or(parent.downloads),
        libraries: merge_libraries(parent.libraries, child.libraries),
        arguments,
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
        java_version: child.java_version.or(parent.java_version),
    }
}

/// ライブラリ一覧を親→子の順にマージする。子側に同じMaven座標(`group:artifact`)の
/// エントリがある場合、親側の対応エントリは取り除く(バージョン差し替えとして扱う)。
fn merge_libraries(parent: Vec<Library>, child: Vec<Library>) -> Vec<Library> {
    use crate::maven::maven_group_artifact;

    let child_keys: std::collections::HashSet<String> = child
        .iter()
        .map(|library| maven_group_artifact(&library.name))
        .collect();
    let mut merged: Vec<Library> = parent
        .into_iter()
        .filter(|library| !child_keys.contains(&maven_group_artifact(&library.name)))
        .collect();
    merged.extend(child);
    merged
}

/// バージョン解決の出所。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSource {
    /// Mojangのバージョンマニフェストに掲載されている通常のバニラバージョン。
    Manifest,
    /// マニフェストには存在せず、ローカルの `versions/<id>/<id>.json` から解決した
    /// (Fabric/Forge等のModローダー導入済みバージョンなど)。呼び出し側はこの場合、
    /// 元ファイルを上書きしないこと(公式ランチャーや各ローダーのインストーラが
    /// 管理しているファイルのため)。
    Local,
}

/// バージョン解決結果。`id` はエイリアス解決後の実際のバージョンID
/// (`latest-release`/`latest-snapshot` はここで具体的なバージョンIDに変換される)。
#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub id: String,
    pub details: VersionDetails,
    pub source: VersionSource,
}

/// 継承関係の解決で無限ループ(不正な循環参照)を起こさないための最大深度。
const MAX_INHERITANCE_DEPTH: u8 = 8;

/// バージョンID(エイリアス・Modローダー導入済みバージョンを含む)を解決し、完全な
/// [`VersionDetails`] を返す。
///
/// 解決順序:
/// 1. `latest-release`/`latest-snapshot` を、マニフェストの `latest` が指す実際の
///    バージョンIDに変換する。
/// 2. Mojangのバージョンマニフェストに存在すれば、そこから直接取得する(通常のバニラ
///    バージョン)。
/// 3. マニフェストに存在しない場合(Fabric/Forge等のModローダー導入済みバージョンなど)、
///    `<destination>/versions/<id>/<id>.json` をローカルから読み込み、`inheritsFrom` で
///    指定された親バージョン(再帰的に本関数で解決)とマージする。
///
/// Modローダー自体の新規インストールは本関数の範囲外([`crate::mod_loader`] が担当する)。
/// 本関数は、`versions/<id>/<id>.json` が(公式Launcher・各ローダー公式インストーラ・
/// [`crate::mod_loader::ensure_mod_loader_installed`] のいずれかによって)既にローカルに
/// 存在していることを前提に解決するのみである。
pub async fn resolve_version(
    version_id: &str,
    paths: &LauncherPaths,
) -> Result<ResolvedVersion, CoreError> {
    let manifest = fetch_version_manifest().await?;
    resolve_version_with_manifest(version_id, paths, &manifest, 0).await
}

fn resolve_version_with_manifest<'a>(
    version_id: &'a str,
    paths: &'a LauncherPaths,
    manifest: &'a VersionManifest,
    depth: u8,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<ResolvedVersion, CoreError>> + Send + 'a>,
> {
    Box::pin(async move {
        if depth > MAX_INHERITANCE_DEPTH {
            return Err(CoreError::VersionNotFound(version_id.to_string()));
        }

        let resolved_id = match version_id {
            "latest-release" => manifest.latest.release.as_str(),
            "latest-snapshot" => manifest.latest.snapshot.as_str(),
            other => other,
        };

        if let Some(entry) = find_version(manifest, resolved_id) {
            let details = fetch_version_details(entry).await?;
            return Ok(ResolvedVersion {
                id: resolved_id.to_string(),
                details,
                source: VersionSource::Manifest,
            });
        }

        // マニフェストに存在しない: ローカルに保存済みのバージョンJSON
        // (Fabric/Forge等、外部ツールで導入済みのModローダー版バージョンなど)を探す。
        let local_path = paths.version_json_path(resolved_id);
        let bytes = tokio::fs::read(&local_path)
            .await
            .map_err(|_| CoreError::VersionNotFound(resolved_id.to_string()))?;
        let partial: PartialVersionDetails = serde_json::from_slice(&bytes)?;

        match partial.inherits_from.clone() {
            Some(parent_id) => {
                let parent =
                    resolve_version_with_manifest(&parent_id, paths, manifest, depth + 1).await?;
                let details = merge_with_parent(partial, parent.details);
                Ok(ResolvedVersion {
                    id: resolved_id.to_string(),
                    details,
                    source: VersionSource::Local,
                })
            }
            None => {
                let details = partial.into_complete(resolved_id)?;
                Ok(ResolvedVersion {
                    id: resolved_id.to_string(),
                    details,
                    source: VersionSource::Local,
                })
            }
        }
    })
}
