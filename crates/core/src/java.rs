//! Discover and validate installed Java runtimes without changing saved preferences.

use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::version_manifest::VersionDetails;
use crate::CoreError;

const JAVA_EXECUTABLE: &str = if cfg!(windows) { "java.exe" } else { "java" };
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct JavaRuntime {
    pub executable: PathBuf,
    pub major_version: u32,
    pub architecture: String,
}

impl JavaRuntime {
    pub fn supports(&self, required_major: u32) -> bool {
        self.major_version >= required_major && self.architecture == std::env::consts::ARCH
    }
}

pub fn required_major_version(details: &VersionDetails) -> Result<u32, CoreError> {
    if let Some(java) = &details.java_version {
        if java.major_version > 0 {
            return Ok(java.major_version);
        }
    } else if let Some(major) = legacy_java_requirement(&details.id) {
        return Ok(major);
    }
    Err(CoreError::JavaRuntime(format!(
        "Minecraft {} の必要なJavaバージョンを判定できません。バージョン情報のjavaVersionを確認してください。",
        details.id
    )))
}

// Older standalone version JSONs may predate javaVersion. Do not guess for
// unknown/custom IDs or snapshots; their resolved metadata must supply it.
fn legacy_java_requirement(id: &str) -> Option<u32> {
    let numbers: Vec<u32> = id
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    match numbers.as_slice() {
        [1, minor] | [1, minor, _] if *minor <= 16 => Some(8),
        [1, 17] | [1, 17, _] => Some(16),
        [1, 18..=19] | [1, 18..=19, _] | [1, 20] | [1, 20, 0..=4] => Some(17),
        [1, 20, patch] if *patch >= 5 => Some(21),
        [1, 21] | [1, 21, _] => Some(21),
        _ => None,
    }
}

pub(crate) fn managed_runtime_root() -> PathBuf {
    crate::paths::default_launcher_root().join("runtimes")
}

/// Download the required Java only when discovery finds no suitable runtime.
pub async fn ensure_runtime(
    required_major: u32,
    preferred_paths: &[String],
    minecraft_root: &Path,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<JavaRuntime, CoreError> {
    on_progress(&format!("Java {required_major} の実行環境を検索中..."));
    match select_runtime(required_major, preferred_paths, minecraft_root).await {
        Ok(runtime) => Ok(runtime),
        Err(CoreError::JavaNotFound(message)) => {
            eprintln!("{message}");
            crate::java_download::install_runtime(required_major, on_progress).await
        }
        Err(err) => Err(err),
    }
}

/// Valid configured paths win in order. Automatic selection uses the required
/// major exactly: newer Java can break older Minecraft versions and their mods.
pub async fn select_runtime(
    required_major: u32,
    preferred_paths: &[String],
    minecraft_root: &Path,
) -> Result<JavaRuntime, CoreError> {
    let configured: Vec<_> = preferred_paths
        .iter()
        .map(|path| path.trim())
        .filter(|path| !path.is_empty())
        .map(|path| (PathBuf::from(path), true))
        .collect();
    let configuration_error = if configured.is_empty() {
        None
    } else {
        match select_from_candidates(required_major, configured, probe_runtime).await {
            Ok(runtime) => return Ok(runtime),
            Err(CoreError::JavaNotFound(message)) => Some(message),
            Err(err) => return Err(err),
        }
    };
    let root = minecraft_root.to_path_buf();
    let installed = tokio::task::spawn_blocking(move || discover_candidates(&root))
        .await
        .map_err(|err| CoreError::JavaRuntime(format!("Javaの検索に失敗しました: {err}")))?;
    let candidates = installed.into_iter().map(|path| (path, false));
    match select_from_candidates(required_major, candidates, probe_runtime).await {
        Err(CoreError::JavaNotFound(message)) => {
            Err(CoreError::JavaNotFound(match configuration_error {
                Some(configured) => format!("{message}\n設定済みJavaの確認結果:\n{configured}"),
                None => message,
            }))
        }
        result => result,
    }
}

async fn select_from_candidates<F, Fut>(
    required_major: u32,
    candidates: impl IntoIterator<Item = (PathBuf, bool)>,
    mut probe: F,
) -> Result<JavaRuntime, CoreError>
where
    F: FnMut(PathBuf) -> Fut,
    Fut: Future<Output = Result<JavaRuntime, String>>,
{
    let mut seen = HashSet::new();
    let mut diagnostics = Vec::new();
    for (path, configured) in candidates {
        if !seen.insert(path.clone()) {
            continue;
        }
        match probe(path.clone()).await {
            Ok(runtime)
                if runtime.supports(required_major)
                    && (configured || runtime.major_version == required_major) =>
            {
                return Ok(runtime);
            }
            Ok(runtime) => diagnostics.push(format!(
                "{}: Java {} ({})",
                path.display(),
                runtime.major_version,
                runtime.architecture
            )),
            Err(err) => {
                eprintln!("failed to probe Java {}: {err}", path.display());
                diagnostics.push(format!("{}: {err}", path.display()));
            }
        }
        if configured {
            if let Some(message) = diagnostics.last() {
                eprintln!("configured Java is not usable: {message}");
            }
        }
    }
    let found = if diagnostics.is_empty() {
        "Java実行ファイルが見つかりませんでした。".to_string()
    } else {
        diagnostics.join("\n")
    };
    Err(CoreError::JavaNotFound(format!(
        "Java {required_major} ({}) の自動選択候補、または必要条件を満たす指定済みJavaが見つかりませんでした。\n検索結果:\n{found}",
        std::env::consts::ARCH
    )))
}

fn resolve_executable(path: &Path) -> Result<PathBuf, String> {
    let mut path = path.to_path_buf();
    // javaw does not reliably produce version output on Windows.
    if path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        name.eq_ignore_ascii_case("javaw.exe")
            || (cfg!(windows) && name.eq_ignore_ascii_case("javaw"))
    }) {
        path.set_file_name("java.exe");
    }
    if path.components().count() == 1 && !path.is_absolute() {
        let search_path = std::env::var_os("PATH").unwrap_or_default();
        let mut names = vec![path.clone()];
        if cfg!(windows) && path.extension().is_none() {
            names.insert(0, path.with_extension("exe"));
        }
        path = std::env::split_paths(&search_path)
            .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| "PATH上に実行ファイルがありません".to_string())?;
    }
    std::fs::canonicalize(&path).map_err(|err| err.to_string())
}

pub(crate) async fn probe_runtime(path: PathBuf) -> Result<JavaRuntime, String> {
    let executable = resolve_executable(&path)?;
    let mut command = Command::new(&executable);
    command
        .args(["-XshowSettings:properties", "-version"])
        .stdin(Stdio::null())
        .kill_on_drop(true);
    crate::process_ext::suppress_console_window(&mut command);
    let output = tokio::time::timeout(PROBE_TIMEOUT, command.output())
        .await
        .map_err(|_| "Javaバージョンの確認がタイムアウトしました".to_string())?
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Javaバージョンの確認に失敗しました ({})",
            output.status
        ));
    }
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    parse_runtime(executable, &text)
}

fn parse_runtime(executable: PathBuf, output: &str) -> Result<JavaRuntime, String> {
    let property = |key: &str| {
        output.lines().find_map(|line| {
            let (name, value) = line.trim().split_once('=')?;
            (name.trim() == key).then(|| value.trim())
        })
    };
    let major_version = property("java.specification.version")
        .and_then(parse_major_version)
        .ok_or_else(|| "Javaのバージョン情報を読み取れません".to_string())?;
    let architecture = match property("os.arch") {
        Some("amd64" | "x86_64") => "x86_64",
        Some("x86" | "i386" | "i486" | "i586" | "i686") => "x86",
        Some("aarch64" | "arm64") => "aarch64",
        Some(other) => other,
        None => return Err("Javaのアーキテクチャを読み取れません".to_string()),
    };
    Ok(JavaRuntime {
        executable,
        major_version,
        architecture: architecture.to_string(),
    })
}

fn parse_major_version(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '-', '_', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    let major = if first == 1 {
        parts.next()?.parse().ok()?
    } else {
        first
    };
    (major > 0).then_some(major)
}

fn discover_candidates(minecraft_root: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        candidates.push(PathBuf::from(home).join("bin").join(JAVA_EXECUTABLE));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let executable = dir.join(JAVA_EXECUTABLE);
            if executable.is_file() {
                candidates.push(executable);
            }
        }
    }
    let mut roots = vec![
        managed_runtime_root().join("installed"),
        minecraft_root.join("runtime"),
        crate::paths::default_minecraft_root().join("runtime"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".jdks"));
        roots.push(home.join(".sdkman").join("candidates").join("java"));
        roots.push(home.join(".jabba").join("jdk"));
        #[cfg(target_os = "macos")]
        roots.push(
            home.join("Library")
                .join("Java")
                .join("JavaVirtualMachines"),
        );
    }
    #[cfg(windows)]
    {
        for variable in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(dir) = std::env::var_os(variable) {
                let dir = PathBuf::from(dir);
                add_windows_java_roots(&dir, &mut roots);
                roots.push(dir.join("Minecraft Launcher").join("runtime"));
            }
        }
        if let Some(local) = dirs::data_local_dir() {
            add_windows_java_roots(&local.join("Programs"), &mut roots);
            roots.push(
                local
                    .join("Packages")
                    .join("Microsoft.4297127D64EC6_8wekyb3d8bbwe")
                    .join("LocalCache")
                    .join("Local")
                    .join("runtime"),
            );
        }
    }
    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
        for prefix in ["/opt/homebrew/opt", "/usr/local/opt"] {
            for dir in child_directories(Path::new(prefix)) {
                if dir
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("openjdk"))
                {
                    roots.push(dir);
                }
            }
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    for root in ["/usr/lib/jvm", "/usr/java", "/opt/java", "/opt/jdk"] {
        roots.push(PathBuf::from(root));
    }
    let mut visited = HashSet::new();
    for root in roots {
        discover_in_root(&root, 4, &mut visited, &mut candidates);
    }
    candidates
}

#[cfg(windows)]
fn add_windows_java_roots(base: &Path, roots: &mut Vec<PathBuf>) {
    for vendor in [
        "Java",
        "Eclipse Adoptium",
        "Eclipse Foundation",
        "Microsoft",
        "Amazon Corretto",
        "BellSoft",
        "Zulu",
        "Semeru",
        "SapMachine",
    ] {
        roots.push(base.join(vendor));
    }
}

fn child_directories(root: &Path) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            eprintln!("failed to search Java directory {}: {err}", root.display());
            return Vec::new();
        }
    };
    let mut directories = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) if entry.path().is_dir() => directories.push(entry.path()),
            Ok(_) => {}
            Err(err) => eprintln!("failed to read Java directory {}: {err}", root.display()),
        }
    }
    directories.sort();
    directories
}

fn discover_in_root(
    root: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    candidates: &mut Vec<PathBuf>,
) {
    // Canonical paths avoid cycles and duplicate vendor/runtime directories.
    let canonical = match std::fs::canonicalize(root) {
        Ok(path) => path,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            eprintln!("failed to resolve Java directory {}: {err}", root.display());
            return;
        }
    };
    if !visited.insert(canonical) {
        return;
    }
    let executable = root.join("bin").join(JAVA_EXECUTABLE);
    if executable.is_file() {
        candidates.push(executable);
        return;
    }
    if depth > 0 {
        for child in child_directories(root) {
            discover_in_root(&child, depth - 1, visited, candidates);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(path: &str, major_version: u32) -> JavaRuntime {
        JavaRuntime {
            executable: PathBuf::from(path),
            major_version,
            architecture: std::env::consts::ARCH.to_string(),
        }
    }

    async fn choose(
        required: u32,
        entries: Vec<(JavaRuntime, bool)>,
    ) -> Result<JavaRuntime, CoreError> {
        let candidates: Vec<_> = entries
            .iter()
            .map(|(runtime, preferred)| (runtime.executable.clone(), *preferred))
            .collect();
        select_from_candidates(required, candidates, |path| {
            let result = entries
                .iter()
                .find(|(runtime, _)| runtime.executable == path)
                .map(|(runtime, _)| runtime.clone())
                .ok_or_else(|| "missing".to_string());
            std::future::ready(result)
        })
        .await
    }

    #[test]
    fn parses_legacy_modern_and_early_access_versions() {
        for (text, expected) in [
            ("1.8.0_451", Some(8)),
            ("8", Some(8)),
            ("17.0.12", Some(17)),
            ("21", Some(21)),
            ("25-ea", Some(25)),
            ("21.0.8+9-LTS", Some(21)),
            ("", None),
            ("unknown", None),
            ("0", None),
            ("1.", None),
        ] {
            assert_eq!(parse_major_version(text), expected, "{text}");
        }
    }

    #[test]
    fn reads_java_properties_and_normalizes_architectures() {
        for (version, arch, expected_major, expected_arch) in [
            ("1.8", "x86", 8, "x86"),
            ("21", "amd64", 21, "x86_64"),
            ("17", "aarch64", 17, "aarch64"),
        ] {
            let text = format!(
                "Property settings:\n    java.specification.version = {version}\n    os.arch = {arch}\n"
            );
            let result = parse_runtime(PathBuf::from("java"), &text).unwrap();
            assert_eq!(result.major_version, expected_major);
            assert_eq!(result.architecture, expected_arch);
        }
        assert!(parse_runtime(PathBuf::from("java"), "java.specification.version = 21").is_err());
        assert!(parse_runtime(PathBuf::from("java"), "os.arch = amd64").is_err());
    }

    #[test]
    fn supports_legacy_metadata_without_guessing_unknown_ids() {
        for (id, expected) in [
            ("1.12.2", Some(8)),
            ("1.16.5", Some(8)),
            ("1.17.1", Some(16)),
            ("1.18", Some(17)),
            ("1.20.4", Some(17)),
            ("1.20.5", Some(21)),
            ("1.21.1", Some(21)),
            ("26.1", None),
            ("24w14a", None),
            ("fabric-loader-1.21.1", None),
        ] {
            assert_eq!(legacy_java_requirement(id), expected, "{id}");
        }
    }

    #[test]
    fn declared_requirement_takes_precedence() {
        let mut details: VersionDetails = serde_json::from_value(serde_json::json!({
            "id": "1.21.1", "type": "release", "mainClass": "Main",
            "assetIndex": {"id": "test", "sha1": "", "size": 0, "url": ""},
            "assets": "test", "downloads": {"client": {"sha1": "", "size": 0, "url": ""}},
            "javaVersion": {"component": "future-runtime", "majorVersion": 25}
        }))
        .unwrap();
        assert_eq!(required_major_version(&details).unwrap(), 25);
        details.java_version.as_mut().unwrap().major_version = 0;
        assert!(required_major_version(&details).is_err());
        details.java_version = None;
        assert_eq!(required_major_version(&details).unwrap(), 21);
        details.id = "unknown".to_string();
        assert!(required_major_version(&details).is_err());
    }

    #[tokio::test]
    async fn replaces_java_8_and_prefers_21_over_newer_installs() {
        let result = choose(
            21,
            vec![
                (runtime("configured-java8", 8), true),
                (runtime("path-java8", 8), false),
                (runtime("jdk25", 25), false),
                (runtime("jdk21", 21), false),
            ],
        )
        .await
        .unwrap();
        assert_eq!(result.executable, PathBuf::from("jdk21"));
    }

    #[tokio::test]
    async fn preserves_compatible_profile_then_global_preference() {
        let result = choose(
            21,
            vec![
                (runtime("profile25", 25), true),
                (runtime("global21", 21), true),
            ],
        )
        .await
        .unwrap();
        assert_eq!(result.executable, PathBuf::from("profile25"));
        let result = choose(
            21,
            vec![
                (runtime("profile8", 8), true),
                (runtime("global21", 21), true),
                (runtime("auto21", 21), false),
            ],
        )
        .await
        .unwrap();
        assert_eq!(result.executable, PathBuf::from("global21"));
    }

    #[tokio::test]
    async fn newer_automatic_candidates_do_not_replace_the_required_major() {
        let error = choose(
            21,
            vec![(runtime("jdk25", 25), false), (runtime("jdk22", 22), false)],
        )
        .await
        .unwrap_err();
        assert!(matches!(error, CoreError::JavaNotFound(_)));
    }

    #[tokio::test]
    async fn rejects_old_or_wrong_architecture_and_reports_requirement() {
        let mut wrong_arch = runtime("wrong-architecture", 21);
        wrong_arch.architecture = "incompatible-architecture".to_string();
        let error = choose(21, vec![(runtime("jdk8", 8), false), (wrong_arch, false)])
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Java 21"));
        assert!(error.contains("jdk8: Java 8"));
        assert!(error.contains("incompatible-architecture"));
        assert!(choose(21, vec![])
            .await
            .unwrap_err()
            .to_string()
            .contains("見つかりません"));
    }

    #[tokio::test]
    async fn continues_after_broken_candidates_and_deduplicates_paths() {
        let mut calls = Vec::new();
        let selected = select_from_candidates(
            21,
            [
                (PathBuf::from("broken"), true),
                (PathBuf::from("broken"), false),
                (PathBuf::from("working"), false),
            ],
            |path| {
                calls.push(path.clone());
                std::future::ready(if path == Path::new("broken") {
                    Err("cannot execute".to_string())
                } else {
                    Ok(runtime("working", 21))
                })
            },
        )
        .await
        .unwrap();
        assert_eq!(selected.major_version, 21);
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn discovers_vendor_mojang_and_macos_layouts_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let homes = [
            dir.path().join("Eclipse Adoptium").join("jdk-21"),
            dir.path()
                .join("runtime")
                .join("java-runtime-delta")
                .join("windows-x64")
                .join("java-runtime-delta"),
            dir.path()
                .join("JavaVirtualMachines")
                .join("temurin-21.jdk")
                .join("Contents")
                .join("Home"),
        ];
        for home in &homes {
            std::fs::create_dir_all(home.join("bin")).unwrap();
            std::fs::write(home.join("bin").join(JAVA_EXECUTABLE), b"fixture").unwrap();
        }
        let mut found = Vec::new();
        let mut visited = HashSet::new();
        discover_in_root(dir.path(), 4, &mut visited, &mut found);
        discover_in_root(dir.path(), 4, &mut visited, &mut found);
        assert_eq!(found.len(), 3);
        for home in homes {
            assert!(found.contains(&home.join("bin").join(JAVA_EXECUTABLE)));
        }
    }

    #[test]
    fn resolves_javaw_to_console_executable_and_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let java = dir.path().join("java.exe");
        std::fs::write(&java, b"fixture").unwrap();
        assert_eq!(
            resolve_executable(&dir.path().join("javaw.exe")).unwrap(),
            java.canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn missing_executable_is_a_probe_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(probe_runtime(dir.path().join("missing-java"))
            .await
            .is_err());
    }
}
