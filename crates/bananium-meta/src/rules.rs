use std::collections::HashMap;

use serde::Deserialize;

use crate::platform::Platform;

/// Which "quick play"/demo/etc. flags are active for this launch. All off
/// by default; only offline vanilla launches happen in M1, so none of these
/// are ever true yet, but the rule evaluator needs the type to exist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeatureFlags {
    pub is_demo_user: bool,
    pub has_custom_resolution: bool,
    pub has_quick_plays_support: bool,
    pub is_quick_play_singleplayer: bool,
    pub is_quick_play_multiplayer: bool,
    pub is_quick_play_realms: bool,
}

impl FeatureFlags {
    /// Look up a flag by the string key Mojang profiles use in
    /// `rules[].features`. Unknown keys are treated as `false` rather than
    /// erroring, so a future flag Bananium doesn't know about yet.
    fn get(&self, key: &str) -> bool {
        match key {
            "is_demo_user" => self.is_demo_user,
            "has_custom_resolution" => self.has_custom_resolution,
            "has_quick_plays_support" => self.has_quick_plays_support,
            "is_quick_play_singleplayer" => self.is_quick_play_singleplayer,
            "is_quick_play_multiplayer" => self.is_quick_play_multiplayer,
            "is_quick_play_realms" => self.is_quick_play_realms,
            _ => false,
        }
    }
}

/// Whether a matching [`Rule`] permits or forbids whatever it's attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

/// The OS conditions a [`Rule`] can match against. Every field is optional
/// and, when present, must match for the rule to apply.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
    /// A regex matched against [`Platform::os_version`].
    pub version: Option<String>,
}

/// One entry in a Mojang `rules` array. See [`evaluate_rules`] for how a
/// whole array combines into a single allow/disallow verdict.
#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default)]
    pub os: Option<OsRule>,
    /// Required feature-flag values, matched by name against [`FeatureFlags`].
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

/// Evaluate a Mojang `rules` array against the given platform/features.
///
/// Semantics (matching the official launcher): an empty rules array always
/// applies. Otherwise the running verdict starts at `false`, and each rule
/// whose conditions match overwrites it with that rule's action — so the
/// common `[{"action":"allow"}, {"action":"disallow","os":{...}}]` pattern
/// reads as "allow, except on this OS", while a lone
/// `[{"action":"allow","os":{"name":"windows"}}]` reads as "only on
/// Windows".
pub fn evaluate_rules(rules: &[Rule], platform: &Platform, features: &FeatureFlags) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, platform, features) {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

/// Whether a single rule's conditions (not its action) match the current
/// context: every present `os.*` field and every entry in `features` must
/// match, AND-ed together. A rule with no conditions at all (no `os`, no
/// `features`) always matches.
fn rule_matches(rule: &Rule, platform: &Platform, features: &FeatureFlags) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != &platform.os_name {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            if arch != &platform.arch {
                return false;
            }
        }
        if let Some(version) = &os.version {
            let Ok(re) = regex_lite::Regex::new(version) else {
                return false;
            };
            if !re.is_match(&platform.os_version) {
                return false;
            }
        }
    }
    if let Some(feature_reqs) = &rule.features {
        for (key, expected) in feature_reqs {
            if features.get(key) != *expected {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux() -> Platform {
        Platform {
            os_name: "linux".into(),
            arch: "x86_64".into(),
            os_version: "6.10.0".into(),
        }
    }
    fn windows() -> Platform {
        Platform {
            os_name: "windows".into(),
            arch: "x86_64".into(),
            os_version: "10.0.19045".into(),
        }
    }
    fn osx() -> Platform {
        Platform {
            os_name: "osx".into(),
            arch: "x86_64".into(),
            os_version: "23.0".into(),
        }
    }

    #[test]
    fn empty_rules_always_apply() {
        assert!(evaluate_rules(&[], &linux(), &FeatureFlags::default()));
    }

    #[test]
    fn allow_then_disallow_os_excludes_that_os() {
        let rules: Vec<Rule> = serde_json::from_str(
            r#"[{"action":"allow"},{"action":"disallow","os":{"name":"osx"}}]"#,
        )
        .unwrap();
        assert!(evaluate_rules(&rules, &linux(), &FeatureFlags::default()));
        assert!(evaluate_rules(&rules, &windows(), &FeatureFlags::default()));
        assert!(!evaluate_rules(&rules, &osx(), &FeatureFlags::default()));
    }

    #[test]
    fn lone_allow_rule_is_an_inclusion_filter() {
        let rules: Vec<Rule> =
            serde_json::from_str(r#"[{"action":"allow","os":{"name":"windows"}}]"#).unwrap();
        assert!(evaluate_rules(&rules, &windows(), &FeatureFlags::default()));
        assert!(!evaluate_rules(&rules, &linux(), &FeatureFlags::default()));
    }

    #[test]
    fn feature_flag_gates_demo_argument() {
        let rules: Vec<Rule> =
            serde_json::from_str(r#"[{"action":"allow","features":{"is_demo_user":true}}]"#)
                .unwrap();
        assert!(!evaluate_rules(&rules, &linux(), &FeatureFlags::default()));
        let demo = FeatureFlags {
            is_demo_user: true,
            ..Default::default()
        };
        assert!(evaluate_rules(&rules, &linux(), &demo));
    }

    #[test]
    fn os_version_regex_matches_windows_10() {
        let rules: Vec<Rule> = serde_json::from_str(
            r#"[{"action":"allow","os":{"name":"windows","version":"^10\\."}}]"#,
        )
        .unwrap();
        assert!(evaluate_rules(&rules, &windows(), &FeatureFlags::default()));
        let win7 = Platform {
            os_version: "6.1.7601".into(),
            ..windows()
        };
        assert!(!evaluate_rules(&rules, &win7, &FeatureFlags::default()));
    }
}
