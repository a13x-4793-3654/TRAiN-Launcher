//! 配布されたリソースパックを`options.txt`の`resourcePacks:`で有効化する。
//!
//! `resourcepacks/`フォルダへ置くだけでは「候補に並ぶ」だけで、実際の見た目には反映されない。
//! 配布側が意図した見た目にするには、`options.txt`の`resourcePacks:`(有効なリソースパックの
//! ファイル名をJSON配列で並べたもの、`file/<ファイル名>`形式)まで書き換える必要がある。
//!
//! ただし、利用者が自分の判断で無効化したリソースパックを毎回勝手に戻すのは押し付けになる。
//! そのため有効化するのは「まだ一度も有効化していないもの」だけに限り、一度触れたものは
//! `enabled_before`(呼び出し側で永続化する、[`crate::profile::Profile::enabled_resource_packs`]参照)
//! に記録し、以後は利用者の判断へ委ねる。配布から外れたパックについては、こちらが有効化した
//! ものに限り片付ける(利用者が自分で入れたものには触れない)。
//!
//! [TRAiN-Setup](https://github.com/a13x-4793-3654/TRAiN-Setup)の
//! `TrainClientSetup/ResourcePackOptions.cs`と同じ設計を踏襲している。

use std::path::Path;

use crate::CoreError;

const FILE_NAME: &str = "options.txt";
const KEY: &str = "resourcePacks:";

/// [`apply`]の結果。呼び出し側の進捗表示・警告表示に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourcePackActivation {
    pub enabled: usize,
    /// 利用者が自分で無効化したため、有効化しなかった件数。
    pub skipped: usize,
    /// 配布から外れたため、片付けた件数。
    pub cleaned: usize,
}

/// `wanted_filenames`(現在配布中のリソースパックのファイル名一覧)を`options.txt`で有効化する。
///
/// `enabled_before`は「これまでにこの仕組みで有効化したことがあるファイル名」の集合を
/// 呼び出し側から渡し、この関数が最新の状態に書き換える(呼び出し側で永続化すること)。
pub fn apply(
    game_dir: &Path,
    wanted_filenames: &[String],
    enabled_before: &mut Vec<String>,
) -> Result<ResourcePackActivation, CoreError> {
    let path = game_dir.join(FILE_NAME);

    // 配布側の指定。options.txtでは "file/<ファイル名>" で表す。
    let wanted_entries: Vec<String> = wanted_filenames
        .iter()
        .map(|name| format!("file/{name}"))
        .collect();
    let keep: std::collections::HashSet<&str> = wanted_entries.iter().map(String::as_str).collect();

    let mut current = read_current(&path)?;
    let mut already_touched: std::collections::HashSet<String> = enabled_before
        .iter()
        .map(|entry| entry.to_ascii_lowercase())
        .collect();

    // 配布から外れたものは、こちらが有効にした分だけ片付ける。残しておくとMinecraftが
    // 「存在しないパック」を抱えたままになる。
    let before_len = current.len();
    current.retain(|entry| {
        let lower = entry.to_ascii_lowercase();
        !(already_touched.contains(&lower) && !keep.contains(entry.as_str()))
    });
    let cleaned = before_len - current.len();

    // 片付けた分は「触ったことがある」記録からも落とす。残すと、同じパックを配り直したときに
    // 「利用者が自分で外した」と誤判定して有効化できなくなる。
    already_touched.retain(|entry| keep.contains(entry.as_str()));

    let mut enabled = 0usize;
    let mut skipped = 0usize;
    for entry in &wanted_entries {
        let lower = entry.to_ascii_lowercase();
        if current
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(entry))
        {
            // 既に有効。こちらで入れたものとして覚えておく。
            already_touched.insert(lower);
            continue;
        }

        if already_touched.contains(&lower) {
            // 一度こちらで入れたのに今は無い = 利用者が外した。戻さない。
            skipped += 1;
            continue;
        }

        // 後ろほど優先されるため、配布物を既定の見た目にしたいので末尾へ足す。
        current.push(entry.clone());
        already_touched.insert(lower);
        enabled += 1;
    }

    // 書き換えが不要な場合でも、記録は必ず最新にする。ここを飛ばすと、配布から外れた
    // パックが記録に残り続け、配り直したときに有効化できなくなる。
    *enabled_before = already_touched.into_iter().collect();
    enabled_before.sort();

    if enabled > 0 || cleaned > 0 {
        write(&path, &current)?;
    }

    Ok(ResourcePackActivation {
        enabled,
        skipped,
        cleaned,
    })
}

/// `options.txt`の`resourcePacks:`行を読む。中身は書き換えず、そのまま返す
/// (既定の見た目は常に土台として効くため、"vanilla"を補ったりはしない)。
fn read_current(path: &Path) -> Result<Vec<String>, CoreError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };

    for line in text.lines() {
        let Some(rest) = line.strip_prefix(KEY) else {
            continue;
        };
        // 壊れている行は読み捨てる。書き直せばMinecraft側が整える。
        return Ok(serde_json::from_str::<Vec<String>>(rest.trim()).unwrap_or_default());
    }
    Ok(Vec::new())
}

fn write(path: &Path, entries: &[String]) -> Result<(), CoreError> {
    let rendered = format!("{KEY}{}", serde_json::to_string(entries)?);

    let lines: Vec<String> = match std::fs::read_to_string(path) {
        Ok(text) => {
            let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
            let mut replaced = false;
            for line in lines.iter_mut() {
                if line.starts_with(KEY) {
                    *line = rendered.clone();
                    replaced = true;
                    break;
                }
            }
            if !replaced {
                lines.push(rendered);
            }
            lines
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // 初回起動前はoptions.txtが無い。この1行だけ書けば、残りの項目は
            // Minecraftが既定値で埋める。
            vec![rendered]
        }
        Err(err) => return Err(err.into()),
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = lines.join("\n");
    content.push('\n');
    let tmp_path = path.with_extension("txt.tmp");
    std::fs::write(&tmp_path, &content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn enables_new_packs_when_options_file_is_absent() {
        let dir = tempdir().unwrap();
        let mut enabled_before = Vec::new();
        let result = apply(dir.path(), &["pack-a.zip".to_string()], &mut enabled_before).unwrap();

        assert_eq!(
            result,
            ResourcePackActivation {
                enabled: 1,
                skipped: 0,
                cleaned: 0
            }
        );
        let content = std::fs::read_to_string(dir.path().join("options.txt")).unwrap();
        assert!(content.contains(r#"resourcePacks:["file/pack-a.zip"]"#));
        assert_eq!(enabled_before, vec!["file/pack-a.zip".to_string()]);
    }

    #[test]
    fn does_not_reenable_pack_the_user_disabled() {
        let dir = tempdir().unwrap();
        let mut enabled_before = Vec::new();
        apply(dir.path(), &["pack-a.zip".to_string()], &mut enabled_before).unwrap();

        // 利用者がoptions.txtからpack-aを手動で外した状態を模す。
        std::fs::write(dir.path().join("options.txt"), "resourcePacks:[]\n").unwrap();

        let result = apply(dir.path(), &["pack-a.zip".to_string()], &mut enabled_before).unwrap();
        assert_eq!(
            result,
            ResourcePackActivation {
                enabled: 0,
                skipped: 1,
                cleaned: 0
            }
        );

        let content = std::fs::read_to_string(dir.path().join("options.txt")).unwrap();
        assert!(content.contains(r#"resourcePacks:[]"#));
    }

    #[test]
    fn cleans_up_packs_removed_from_distribution() {
        let dir = tempdir().unwrap();
        let mut enabled_before = Vec::new();
        apply(
            dir.path(),
            &["pack-a.zip".to_string(), "pack-b.zip".to_string()],
            &mut enabled_before,
        )
        .unwrap();

        // pack-bが配布から外れた。
        let result = apply(dir.path(), &["pack-a.zip".to_string()], &mut enabled_before).unwrap();
        assert_eq!(
            result,
            ResourcePackActivation {
                enabled: 0,
                skipped: 0,
                cleaned: 1
            }
        );

        let content = std::fs::read_to_string(dir.path().join("options.txt")).unwrap();
        assert!(content.contains("file/pack-a.zip"));
        assert!(!content.contains("file/pack-b.zip"));
    }

    #[test]
    fn preserves_unrelated_options_lines() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("options.txt"),
            "version:3465\nfov:1.0\nsoundCategory_master:0.5\n",
        )
        .unwrap();

        let mut enabled_before = Vec::new();
        apply(dir.path(), &["pack-a.zip".to_string()], &mut enabled_before).unwrap();

        let content = std::fs::read_to_string(dir.path().join("options.txt")).unwrap();
        assert!(content.contains("version:3465"));
        assert!(content.contains("fov:1.0"));
        assert!(content.contains("soundCategory_master:0.5"));
        assert!(content.contains("resourcePacks:"));
    }

    #[test]
    fn does_not_touch_packs_the_user_added_manually() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("options.txt"),
            r#"resourcePacks:["file/my-own-pack.zip"]"#.to_string() + "\n",
        )
        .unwrap();

        let mut enabled_before = Vec::new();
        let result = apply(dir.path(), &["pack-a.zip".to_string()], &mut enabled_before).unwrap();
        assert_eq!(result.enabled, 1);
        assert_eq!(result.cleaned, 0);

        let content = std::fs::read_to_string(dir.path().join("options.txt")).unwrap();
        assert!(content.contains("file/my-own-pack.zip"));
        assert!(content.contains("file/pack-a.zip"));
    }
}
