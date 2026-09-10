//! バージョンごとのライブラリ・アセット・クライアントjarのダウンロード。
//!
//! Mojangのバージョン詳細JSONに従い、以下を並行ダウンロードする:
//! 1. クライアントjar本体
//! 2. 現在のOS/アーキテクチャに適用されるライブラリ(通常のクラスパス用jar)
//! 3. レガシー(LWJGL2世代)ネイティブライブラリ(classifiers形式のjarを展開)
//! 4. アセットインデックスとその配下の全アセットファイル
//!
//! 既にSHA1が一致するファイルが存在する場合はダウンロードをスキップする(再起動時の
//! 差分ダウンロード)。

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use sha1::{Digest, Sha1};
use tokio::sync::Semaphore;

use crate::paths::LauncherPaths;
use crate::rules::{rules_allow, CurrentPlatform};
use crate::version_manifest::{self, Artifact, VersionDetails, VersionSource};
use crate::CoreError;

/// 同時ダウンロード数の上限(アセットは数千個に及ぶため、際限なく並行実行しない)。
const MAX_CONCURRENT_DOWNLOADS: usize = 16;

/// ダウンロード処理の進行フェーズ。フロントエンドでの表示ラベルは呼び出し側(Tauri command層)
/// で決定するため、ここでは機械的に判定できる識別子のみを持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadPhase {
    /// バージョンマニフェスト・バージョン詳細JSONの取得。
    FetchingManifest,
    /// クライアント本体(jar)のダウンロード。
    ClientJar,
    /// ライブラリ・ネイティブライブラリのダウンロード。
    Libraries,
    /// アセットインデックス・アセット本体のダウンロード。
    Assets,
}

/// ダウンロード進捗の1イベント。呼び出し側は `completed`/`total` から進捗率を計算できる
/// (`total` が0の場合はまだ件数が確定していないことを示す)。
#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    pub phase: DownloadPhase,
    pub completed: usize,
    pub total: usize,
}

/// 進捗コールバックの型エイリアス。ダウンロードは並行実行されるため `Fn` + `Send + Sync`
/// を要求する(`Arc` で複数タスクに共有する)。
pub type ProgressCallback = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 既存ファイルが期待するSHA1と一致するかを確認する。ファイルが無い、または読み込めない
/// 場合は不一致(再ダウンロードが必要)として扱う。
async fn file_matches_sha1(path: &Path, expected_sha1: &str) -> bool {
    match tokio::fs::read(path).await {
        Ok(bytes) => sha1_hex(&bytes) == expected_sha1,
        Err(_) => false,
    }
}

/// 1ファイルをダウンロードし、SHA1を検証したうえで保存する。
/// 既に同じ内容のファイルが存在する場合はネットワークアクセスをスキップする。
async fn download_verified(
    client: &reqwest::Client,
    url: &str,
    expected_sha1: &str,
    dest: &Path,
) -> Result<(), CoreError> {
    if file_matches_sha1(dest, expected_sha1).await {
        return Ok(());
    }

    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let actual_sha1 = sha1_hex(&bytes);
    if actual_sha1 != expected_sha1 {
        return Err(CoreError::HashMismatch {
            url: url.to_string(),
            expected: expected_sha1.to_string(),
            actual: actual_sha1,
        });
    }

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(dest, &bytes).await?;
    Ok(())
}

/// 指定バージョンに必要なライブラリ/アセット/クライアントjarをダウンロードする。
///
/// `destination` はランチャーのルートディレクトリ(`.minecraft` 相当)であり、
/// `versions/` `libraries/` `assets/` の各サブディレクトリを内部で解決する。
/// `on_progress` にはフェーズごとの進捗が随時通知される(呼び出し側はこれをUIへ反映できる)。
///
/// `version_id` には次のいずれも指定できる(解決は [`version_manifest::resolve_version`] に
/// 委ねる):
/// - 通常のバニラバージョンID(例: `"1.21.1"`)
/// - `"latest-release"`/`"latest-snapshot"` エイリアス
/// - Fabric/Forge等、Modローダー導入済みのバージョンID(例:
///   `"fabric-loader-0.19.5-1.21.1"`)。ローダー自体が事前に導入済みで、
///   `<destination>/versions/<id>/<id>.json` がローカルに存在することが前提。
///
/// 戻り値の [`ResolvedVersion::id`] は解決後の実際のバージョンID(エイリアス解決済み)を返す。
/// 呼び出し側は以降の起動処理([`crate::launch::launch`] 等)に、この解決結果
/// (`ResolvedVersion`)をそのまま渡すこと。
pub async fn download_version_files(
    version_id: &str,
    destination: &Path,
    on_progress: ProgressCallback,
) -> Result<version_manifest::ResolvedVersion, CoreError> {
    let paths = LauncherPaths::new(destination);
    let platform = CurrentPlatform::detect();
    let client = reqwest::Client::new();

    on_progress(DownloadProgress {
        phase: DownloadPhase::FetchingManifest,
        completed: 0,
        total: 1,
    });
    let resolved = version_manifest::resolve_version(version_id, &paths).await?;
    on_progress(DownloadProgress {
        phase: DownloadPhase::FetchingManifest,
        completed: 1,
        total: 1,
    });

    // 起動時にMojangへ再問い合わせしなくて済むよう、バージョンJSONをキャッシュしておく。
    // Modローダー導入済みバージョン(`VersionSource::Local`)の場合、この元ファイルは
    // 公式ランチャーやローダーのインストーラが管理しているため上書きしない
    // (マージ結果は呼び出し側にメモリ上で返すのみとする)。
    if resolved.source == VersionSource::Manifest {
        let version_json_path = paths.version_json_path(&resolved.id);
        if let Some(parent) = version_json_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(
            &version_json_path,
            serde_json::to_vec_pretty(&resolved.details)?,
        )
        .await?;
    }

    on_progress(DownloadProgress {
        phase: DownloadPhase::ClientJar,
        completed: 0,
        total: 1,
    });
    download_client_jar(&client, &paths, &resolved.id, &resolved.details).await?;
    on_progress(DownloadProgress {
        phase: DownloadPhase::ClientJar,
        completed: 1,
        total: 1,
    });

    download_libraries(
        &client,
        &paths,
        &resolved.id,
        &resolved.details,
        platform,
        &on_progress,
    )
    .await?;
    download_assets(&client, &paths, &resolved.details, &on_progress).await?;

    Ok(resolved)
}

async fn download_client_jar(
    client: &reqwest::Client,
    paths: &LauncherPaths,
    version_id: &str,
    details: &VersionDetails,
) -> Result<(), CoreError> {
    let dest = paths.version_jar_path(version_id);
    download_verified(
        client,
        &details.downloads.client.url,
        &details.downloads.client.sha1,
        &dest,
    )
    .await
}

async fn download_libraries(
    client: &reqwest::Client,
    paths: &LauncherPaths,
    version_id: &str,
    details: &VersionDetails,
    platform: CurrentPlatform,
    on_progress: &ProgressCallback,
) -> Result<(), CoreError> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));
    let mut tasks = tokio::task::JoinSet::new();
    let mut total: usize = 0;

    for library in &details.libraries {
        if !rules_allow(&library.rules, platform) {
            continue;
        }
        let Some(downloads) = &library.downloads else {
            continue;
        };

        if let Some(artifact) = &downloads.artifact {
            let dest = artifact_dest(paths, artifact, &library.name)?;
            spawn_download(
                &mut tasks,
                semaphore.clone(),
                client.clone(),
                artifact.url.clone(),
                artifact.sha1.clone(),
                dest,
            );
            total += 1;
        }

        // レガシー(LWJGL2世代)ネイティブライブラリ: `natives` マップと `classifiers` の
        // 両方が揃っている場合のみ、対応する分類子のjarを展開対象とする。
        if let Some(natives_map) = &library.natives {
            if let Some(classifier_key) = natives_map.get(platform.os_name) {
                let classifier_key = classifier_key.replace("${arch}", platform.arch_bits());
                if let Some(classifiers) = &downloads.classifiers {
                    if let Some(artifact) = classifiers.get(&classifier_key) {
                        let exclude = library
                            .extract
                            .clone()
                            .unwrap_or_default()
                            .exclude;
                        let natives_dir = paths.natives_dir(version_id);
                        let (url, sha1) = (artifact.url.clone(), artifact.sha1.clone());
                        let client = client.clone();
                        let semaphore = semaphore.clone();
                        tasks.spawn(async move {
                            let _permit = semaphore.acquire_owned().await.expect("semaphore closed");
                            let bytes = client
                                .get(&url)
                                .send()
                                .await?
                                .error_for_status()?
                                .bytes()
                                .await?;
                            let actual_sha1 = sha1_hex(&bytes);
                            if actual_sha1 != sha1 {
                                return Err(CoreError::HashMismatch {
                                    url,
                                    expected: sha1,
                                    actual: actual_sha1,
                                });
                            }
                            extract_natives_jar(&bytes, &natives_dir, &exclude)
                        });
                        total += 1;
                    }
                }
            }
        }
    }

    let mut completed = 0usize;
    on_progress(DownloadProgress {
        phase: DownloadPhase::Libraries,
        completed,
        total,
    });
    while let Some(result) = tasks.join_next().await {
        result.expect("download task panicked")?;
        completed += 1;
        on_progress(DownloadProgress {
            phase: DownloadPhase::Libraries,
            completed,
            total,
        });
    }
    Ok(())
}

/// ライブラリの `artifact.path` から `libraries/` 配下の保存先を組み立てる。
/// `path` が省略されている場合(ごく古い形式)はMavenの `name` (`group:artifact:version`) から
/// 標準的なMavenレイアウトパスを合成する。
fn artifact_dest(
    paths: &LauncherPaths,
    artifact: &Artifact,
    library_name: &str,
) -> Result<std::path::PathBuf, CoreError> {
    if let Some(path) = &artifact.path {
        return Ok(paths.library_path(path));
    }
    maven_path(library_name).map(|relative| paths.library_path(&relative))
}

/// `group:artifact:version[:classifier]` 形式のMaven座標をパスに変換する。
fn maven_path(name: &str) -> Result<String, CoreError> {
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

fn spawn_download(
    tasks: &mut tokio::task::JoinSet<Result<(), CoreError>>,
    semaphore: Arc<Semaphore>,
    client: reqwest::Client,
    url: String,
    sha1: String,
    dest: std::path::PathBuf,
) {
    tasks.spawn(async move {
        let _permit = semaphore.acquire_owned().await.expect("semaphore closed");
        download_verified(&client, &url, &sha1, &dest).await
    });
}

/// ダウンロードしたネイティブライブラリjarを展開する。`exclude` に前方一致するエントリ
/// (通常は `META-INF/`)はスキップする。
fn extract_natives_jar(
    bytes: &[u8],
    natives_dir: &Path,
    exclude: &[String],
) -> Result<(), CoreError> {
    std::fs::create_dir_all(natives_dir)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(entry_name) = entry.enclosed_name() else {
            continue;
        };
        let entry_name_str = entry_name.to_string_lossy();
        if exclude.iter().any(|prefix| entry_name_str.starts_with(prefix.as_str())) {
            continue;
        }
        if entry.is_dir() {
            continue;
        }
        let dest_path = natives_dir.join(&entry_name);
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out_file = std::fs::File::create(&dest_path)?;
        std::io::copy(&mut entry, &mut out_file)?;
    }
    Ok(())
}

const ASSET_OBJECTS_BASE_URL: &str = "https://resources.download.minecraft.net";

async fn download_assets(
    client: &reqwest::Client,
    paths: &LauncherPaths,
    details: &VersionDetails,
    on_progress: &ProgressCallback,
) -> Result<(), CoreError> {
    let asset_index = version_manifest::fetch_asset_index(&details.asset_index).await?;

    let index_path = paths.asset_index_path(&details.asset_index.id);
    if let Some(parent) = index_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&index_path, serde_json::to_vec(&asset_index)?).await?;

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));
    let mut tasks = tokio::task::JoinSet::new();

    for object in asset_index.objects.values() {
        let dest = paths.asset_object_path(&object.hash);
        let url = format!(
            "{ASSET_OBJECTS_BASE_URL}/{}/{}",
            &object.hash[..2],
            object.hash
        );
        spawn_download(
            &mut tasks,
            semaphore.clone(),
            client.clone(),
            url,
            object.hash.clone(),
            dest,
        );
    }

    // アセットは数千件に及ぶことがあるため、全件通知するとIPCイベントが過剰になる。
    // 200回程度に間引いて通知する(最初・最後は必ず通知する)。
    let total = asset_index.objects.len();
    let report_step = (total / 200).max(1);
    let mut completed = 0usize;
    on_progress(DownloadProgress {
        phase: DownloadPhase::Assets,
        completed,
        total,
    });
    while let Some(result) = tasks.join_next().await {
        result.expect("download task panicked")?;
        completed += 1;
        if completed == total || completed % report_step == 0 {
            on_progress(DownloadProgress {
                phase: DownloadPhase::Assets,
                completed,
                total,
            });
        }
    }
    Ok(())
}

