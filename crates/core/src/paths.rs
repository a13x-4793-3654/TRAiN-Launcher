//! ランチャーが使用するディレクトリ構造の解決。
//!
//! 全プロファイル共通のゲームデータ(バージョンjar・ライブラリ・アセット)を1つの
//! ルートディレクトリ配下にまとめて管理する(一般的なMinecraftランチャーの `.minecraft`
//! ディレクトリ相当)。バージョンファイル自体はどのプロファイルからも共有されるため、
//! プロファイルごとに複製はしない。

use std::path::{Path, PathBuf};

/// ランチャーのルートディレクトリを起点とした各種パスを解決するヘルパー。
#[derive(Debug, Clone)]
pub struct LauncherPaths {
    root: PathBuf,
}

impl LauncherPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn version_dir(&self, version_id: &str) -> PathBuf {
        self.versions_dir().join(version_id)
    }

    /// キャッシュ済みのバージョンJSON(取得したVersionDetailsの生データ)の保存先。
    /// 起動時に毎回Mojangへ問い合わせずに済むよう、ダウンロード時に保存しておく。
    pub fn version_json_path(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id).join(format!("{version_id}.json"))
    }

    pub fn version_jar_path(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id).join(format!("{version_id}.jar"))
    }

    /// レガシーバージョン(LWJGL2世代)向けに展開したネイティブライブラリの保存先。
    pub fn natives_dir(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id).join("natives")
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub fn library_path(&self, relative_path: &str) -> PathBuf {
        // Mojangのライブラリpathは常に `/` 区切りなので、OSのパス区切りに変換する。
        let mut path = self.libraries_dir();
        for segment in relative_path.split('/') {
            path.push(segment);
        }
        path
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn asset_index_path(&self, assets_id: &str) -> PathBuf {
        self.assets_dir().join("indexes").join(format!("{assets_id}.json"))
    }

    pub fn asset_object_path(&self, hash: &str) -> PathBuf {
        let prefix = &hash[..hash.len().min(2)];
        self.assets_dir().join("objects").join(prefix).join(hash)
    }
}

/// TRAiN Launcher自身の設定ディレクトリ (`%APPDATA%\train-launcher` 等)。
///
/// プロファイル設定 (`profile.rs`) はこの直下の `profiles.json` に保存する。
/// ゲーム本体のファイル(バージョンJar・ライブラリ・アセット)は
/// [`default_minecraft_root`] を参照すること。
pub fn default_launcher_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("train-launcher")
}

/// 公式Minecraft Launcherが使用するゲームデータディレクトリ(`.minecraft` 相当)。
///
/// TRAiN Launcherはバージョンjar・ライブラリ・アセットを公式Launcherと共有する設計とし、
/// 二重ダウンロードやディスク容量の浪費を避ける(ユーザーの明示的な選択による)。
/// このためTRAiN Launcher・公式Launcherのどちらでダウンロード/起動しても、
/// 互いに同じファイル群を再利用できる。
pub fn default_minecraft_root() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(".minecraft")
    }
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("minecraft")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        dirs::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(".minecraft")
    }
}

/// 実際に使用するゲームディレクトリを解決する。
///
/// `override_dir` (設定画面で指定された [`crate::settings::AppSettings::game_directory`])
/// が指定されていればそれを、未指定または空文字列の場合は [`default_minecraft_root`] を返す。
/// この上書き先を使う場合、公式Minecraft Launcherとのファイル共有・
/// `launcher_profiles.json` 連携は行われなくなる点に注意(上書き先は独自ディレクトリのため)。
pub fn effective_minecraft_root(override_dir: Option<&str>) -> PathBuf {
    match override_dir.map(str::trim) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => default_minecraft_root(),
    }
}
