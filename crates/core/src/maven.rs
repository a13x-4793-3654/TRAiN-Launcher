//! Maven座標(`group:artifact:version[:classifier]`)に関する共通ヘルパー。
//!
//! [`version_manifest`](crate::version_manifest)・[`download`](crate::download)・
//! [`launch`](crate::launch) の各モジュールから参照される。
//!
//! Mojang公式のバージョンJSONは各ライブラリの実体を `downloads.artifact`(相対パス・SHA1・
//! ダウンロードURLを明示)で表現するが、Fabric/Quilt/Forge等のローダーが生成する
//! バージョンJSONは、より単純な「フラット形式」(`name` + Mavenリポジトリのベース `url`
//! のみ、`downloads` フィールド自体が存在しない)を使うことが多い。この違いを吸収し、
//! どちらの形式でも同じようにダウンロードURL・保存先パスを解決できるようにする。

use crate::version_manifest::Library;
use crate::CoreError;

/// `group:artifact:version[:classifier]` 形式のMaven座標を、標準的なMavenリポジトリの
/// 相対パス(`group/artifact/version/artifact-version[-classifier].jar`)に変換する。
pub fn maven_path(name: &str) -> Result<String, CoreError> {
    let parts: Vec<&str> = name.split(':').collect();
    let [group, artifact, version] = parts[..3.min(parts.len())] else {
        return Err(CoreError::InvalidLibraryName(name.to_string()));
    };
    let classifier = parts.get(3);
    let group_path = group.replace('.', "/");
    let file_name = match classifier {
        Some(classifier) => format!("{artifact}-{version}-{classifier}.jar"),
        None => format!("{artifact}-{version}.jar"),
    };
    Ok(format!("{group_path}/{artifact}/{version}/{file_name}"))
}

/// Maven座標から `group:artifact` 部分のみを取り出す(バージョン差し替え検出用の比較キー)。
pub fn maven_group_artifact(name: &str) -> String {
    name.splitn(3, ':').take(2).collect::<Vec<_>>().join(":")
}

/// ライブラリのダウンロード情報(URL・SHA1・`libraries/` 配下の相対保存パス)。
/// SHA1が不明な場合(フラット形式で省略されている場合)は検証をスキップする。
#[derive(Debug, Clone)]
pub struct ResolvedLibraryArtifact {
    pub url: String,
    pub sha1: Option<String>,
    /// `libraries/` ディレクトリからの相対パス(`/` 区切り)。クラスパス組み立て・
    /// ダウンロード先の両方に使う。
    pub relative_path: String,
}

/// ライブラリのダウンロード対象(メインjar)を解決する。ネイティブ(classifiers)専用の
/// エントリや、rulesにより対象外となるものは呼び出し側で別途判定すること
/// (本関数は `library.rules` を見ない)。
///
/// 対応する形式:
/// - 現行のMojang形式: `downloads.artifact`(`path` 省略時はMaven座標から合成)
/// - Fabric/Quilt/Forge等が生成するフラット形式: トップレベルの `url`
///   (Mavenリポジトリのベースurl)+ `name`(Maven座標)。`sha1` は省略されることがある。
pub fn resolve_library_artifact(
    library: &Library,
) -> Result<Option<ResolvedLibraryArtifact>, CoreError> {
    if let Some(downloads) = &library.downloads {
        return Ok(match &downloads.artifact {
            Some(artifact) => {
                let relative_path = match &artifact.path {
                    Some(path) => path.clone(),
                    None => maven_path(&library.name)?,
                };
                Some(ResolvedLibraryArtifact {
                    url: artifact.url.clone(),
                    sha1: Some(artifact.sha1.clone()),
                    relative_path,
                })
            }
            // natives専用(classifiersのみ)等、通常のartifactを持たないライブラリ。
            None => None,
        });
    }

    // フラット形式: `downloads` が存在せず、トップレベルの `url` がMavenリポジトリの
    // ベースURLを表す。
    match &library.url {
        Some(base_url) => {
            let relative_path = maven_path(&library.name)?;
            let base = base_url.trim_end_matches('/');
            Ok(Some(ResolvedLibraryArtifact {
                url: format!("{base}/{relative_path}"),
                sha1: library.sha1.clone(),
                relative_path,
            }))
        }
        None => Ok(None),
    }
}
