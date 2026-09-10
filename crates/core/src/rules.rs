//! Mojangバージョンjsonの `rules` 配列の評価。
//!
//! ライブラリ・JVM引数・ゲーム引数のいずれも同じ形式の `rules` (action + 条件) で
//! 現在のOS/機能フラグに応じて適用有無を判定する。判定アルゴリズムはMojang公式ランチャーと
//! 同一: ルールを先頭から順に評価し、条件が一致したルールのうち最後に一致したものの
//! action (allow/disallow) が最終結果になる。`rules` が存在しない場合は常に適用する。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 現在の実行環境(OS種別・アーキテクチャ)。ルール評価に使う。
#[derive(Debug, Clone, Copy)]
pub struct CurrentPlatform {
    pub os_name: &'static str,
    pub os_arch: &'static str,
}

impl CurrentPlatform {
    /// 実行中のOS/アーキテクチャから判定する(コンパイル時のターゲットに基づく)。
    pub fn detect() -> Self {
        let os_name = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "osx"
        } else {
            "linux"
        };
        let os_arch = if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "x86") {
            "x86"
        } else if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "unknown"
        };
        Self { os_name, os_arch }
    }

    /// レガシーライブラリの `natives` マップの値に含まれることがある `${arch}` プレースホルダー
    /// (32/64ビット表記)の置換に使う。
    pub fn arch_bits(&self) -> &'static str {
        if cfg!(target_pointer_width = "64") {
            "64"
        } else {
            "32"
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default)]
    pub os: Option<OsRule>,
    /// `is_demo_user`/`has_custom_resolution`等。現状これらの機能は未対応のため、
    /// 「trueが要求されている機能」を含むルールは常に不一致として扱う。
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    // `version` (正規表現でのOSバージョン一致)は現状使用例が乏しいため未対応。
}

impl Rule {
    fn matches(&self, platform: CurrentPlatform) -> bool {
        if let Some(os) = &self.os {
            if let Some(name) = &os.name {
                if name != platform.os_name {
                    return false;
                }
            }
            if let Some(arch) = &os.arch {
                if arch != platform.os_arch {
                    return false;
                }
            }
        }
        if let Some(features) = &self.features {
            // 現状サポートしている機能フラグは無い(常にfalse扱い)ため、
            // trueが要求されている項目が1つでもあれば不一致とする。
            if features.values().any(|required| *required) {
                return false;
            }
        }
        true
    }
}

/// `rules` 配列全体を評価し、現在の環境に適用すべきかどうかを判定する。
/// `rules` が `None` の場合は常に `true` (常に適用)。
pub fn rules_allow(rules: &Option<Vec<Rule>>, platform: CurrentPlatform) -> bool {
    match rules {
        None => true,
        Some(rules) => {
            let mut allowed = false;
            for rule in rules {
                if rule.matches(platform) {
                    allowed = rule.action == RuleAction::Allow;
                }
            }
            allowed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows() -> CurrentPlatform {
        CurrentPlatform {
            os_name: "windows",
            os_arch: "x86_64",
        }
    }

    fn osx() -> CurrentPlatform {
        CurrentPlatform {
            os_name: "osx",
            os_arch: "x86_64",
        }
    }

    #[test]
    fn no_rules_always_allowed() {
        assert!(rules_allow(&None, windows()));
    }

    #[test]
    fn os_specific_allow_rule_matches_only_that_os() {
        let rules = Some(vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("osx".to_string()),
                arch: None,
            }),
            features: None,
        }]);
        assert!(!rules_allow(&rules, windows()));
        assert!(rules_allow(&rules, osx()));
    }

    #[test]
    fn allow_all_then_disallow_specific_os() {
        // 典型的な「基本は許可、特定OSのみ除外」パターン。
        let rules = Some(vec![
            Rule {
                action: RuleAction::Allow,
                os: None,
                features: None,
            },
            Rule {
                action: RuleAction::Disallow,
                os: Some(OsRule {
                    name: Some("osx".to_string()),
                    arch: None,
                }),
                features: None,
            },
        ]);
        assert!(rules_allow(&rules, windows()));
        assert!(!rules_allow(&rules, osx()));
    }

    #[test]
    fn feature_rule_requiring_true_is_never_matched() {
        let mut features = HashMap::new();
        features.insert("is_demo_user".to_string(), true);
        let rules = Some(vec![Rule {
            action: RuleAction::Allow,
            os: None,
            features: Some(features),
        }]);
        assert!(!rules_allow(&rules, windows()));
    }
}
