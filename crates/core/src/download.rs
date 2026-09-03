//! バージョンごとのライブラリ・アセット・クライアントjarのダウンロード。

use std::path::Path;

use crate::CoreError;

/// 指定バージョンに必要なライブラリ/アセット/クライアントjarをダウンロードする。
///
/// TODO: 並列ダウンロード・SHA1ハッシュ検証・既存ファイルの差分スキップを実装する。
pub async fn download_version_files(
    _version_id: &str,
    _destination: &Path,
) -> Result<(), CoreError> {
    Err(CoreError::NotImplemented(
        "download::download_version_files",
    ))
}
