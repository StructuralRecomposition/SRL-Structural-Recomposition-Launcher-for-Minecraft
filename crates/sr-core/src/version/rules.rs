use super::model::{Argument, Rule, StringOrVec};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsName {
    Windows,
    Osx,
    Linux,
    Other(String),
}

impl OsName {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "windows" => Self::Windows,
            "macos" => Self::Osx,
            "linux" => Self::Linux,
            other => Self::Other(other.to_string()),
        }
    }
    pub fn as_minecraft(&self) -> &str {
        match self {
            Self::Windows => "windows",
            Self::Osx => "osx",
            Self::Linux => "linux",
            Self::Other(s) => s,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuleContext {
    pub os: OsName,
    pub arch: String,
    pub os_version: String,
    pub features: BTreeMap<String, bool>,
}

impl Default for RuleContext {
    fn default() -> Self {
        Self {
            os: OsName::current(),
            arch: std::env::consts::ARCH.to_string(),
            os_version: String::new(),
            features: BTreeMap::new(),
        }
    }
}

impl RuleContext {
    pub fn with_features(features: impl IntoIterator<Item = (&'static str, bool)>) -> Self {
        let mut ctx = Self::default();
        for (k, v) in features {
            ctx.features.insert(k.to_string(), v);
        }
        ctx
    }

    /// Minecraft's arch vocabulary.
    pub fn arch_name(&self) -> &'static str {
        match self.arch.as_str() {
            "x86_64" | "amd64" | "x86_64h" => "x86_64",
            "x86" | "i686" | "i386" => "x86",
            "aarch64" | "arm64" => "arm64",
            _ => "x86_64",
        }
    }

    fn matches_os(&self, os: &super::model::OsRule) -> bool {
        if let Some(name) = &os.name {
            if name != self.os.as_minecraft() {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            // Some legacy manifests use "x86" to mean "not 64-bit".
            if arch == "x86" {
                if self.arch_name() != "x86" {
                    return false;
                }
            } else if arch != self.arch_name() && arch != &self.arch {
                return false;
            }
        }
        if let Some(version_pattern) = &os.version {
            if !version_matches(version_pattern, &self.os_version) {
                return false;
            }
        }
        true
    }
}

/// Minimal matcher for the version patterns used by Mojang manifests.
/// Supports `^prefix`, `prefix$` and plain substring; treats other regex
/// metacharacters as literal text. Documented limitation: full regex is not
/// implemented because the official manifests only use prefix patterns.
fn version_matches(pattern: &str, version: &str) -> bool {
    let p = pattern.trim();
    if let Some(rest) = p.strip_prefix('^') {
        let rest = rest.trim_end_matches('$').replace("\\\\", "\\");
        return version.starts_with(&rest.replace("\\.", "."));
    }
    if let Some(rest) = p.strip_suffix('$') {
        return version.ends_with(&rest.replace("\\.", "."));
    }
    version.contains(&p.replace("\\.", "."))
}

/// Evaluate a rule list with last-match-wins semantics, matching the vanilla
/// behaviour: no rules means allowed; no matching rule means disallowed.
pub fn rules_allow(rules: &[Rule], ctx: &RuleContext) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        let os_ok = rule.os.as_ref().map(|o| ctx.matches_os(o)).unwrap_or(true);
        let features_ok = rule
            .features
            .as_ref()
            .map(|f| {
                f.iter()
                    .all(|(k, v)| ctx.features.get(k).copied().unwrap_or(false) == *v)
            })
            .unwrap_or(true);
        if os_ok && features_ok {
            allowed = rule.action.eq_ignore_ascii_case("allow");
        }
    }
    allowed
}

/// Does this library apply on the current platform?
pub fn library_allowed(rules: &Option<Vec<Rule>>, ctx: &RuleContext) -> bool {
    match rules {
        None => true,
        Some(r) => rules_allow(r, ctx),
    }
}

/// Expand a possibly-conditional argument entry.
pub fn argument_values<'a>(arg: &'a Argument, ctx: &RuleContext) -> Vec<&'a String> {
    match arg {
        Argument::Simple(s) => vec![s],
        Argument::Conditional { rules, value } => {
            if rules_allow(rules, ctx) {
                value.iter().collect()
            } else {
                Vec::new()
            }
        }
    }
}

pub fn string_or_vec_values(value: &StringOrVec) -> Vec<&String> {
    value.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx(os: &str, arch: &str) -> RuleContext {
        RuleContext {
            os: match os {
                "windows" => OsName::Windows,
                "osx" => OsName::Osx,
                _ => OsName::Linux,
            },
            arch: arch.into(),
            os_version: "10.0.19045".into(),
            features: BTreeMap::new(),
        }
    }

    fn parse_rules(v: serde_json::Value) -> Vec<Rule> {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn empty_rules_allow() {
        assert!(rules_allow(&[], &ctx("linux", "x86_64")));
    }

    #[test]
    fn os_specific_rules() {
        let rules = parse_rules(json!([{ "action": "allow", "os": { "name": "osx" } }]));
        assert!(rules_allow(&rules, &ctx("osx", "arm64")));
        assert!(!rules_allow(&rules, &ctx("linux", "x86_64")));
    }

    #[test]
    fn disallow_overrides_allow() {
        let rules = parse_rules(json!([
            { "action": "allow" },
            { "action": "disallow", "os": { "name": "windows" } }
        ]));
        assert!(!rules_allow(&rules, &ctx("windows", "x86_64")));
        assert!(rules_allow(&rules, &ctx("linux", "x86_64")));
    }

    #[test]
    fn arch_matching() {
        let rules = parse_rules(json!([{ "action": "allow", "os": { "arch": "x86" } }]));
        assert!(!rules_allow(&rules, &ctx("windows", "x86_64")));
        assert!(rules_allow(&rules, &ctx("windows", "x86")));
        let rules = parse_rules(json!([{ "action": "allow", "os": { "arch": "arm64" } }]));
        assert!(rules_allow(&rules, &ctx("osx", "aarch64")));
    }

    #[test]
    fn feature_rules() {
        let rules = parse_rules(
            json!([{ "action": "allow", "features": { "has_custom_resolution": true } }]),
        );
        let mut c = ctx("linux", "x86_64");
        assert!(!rules_allow(&rules, &c));
        c.features.insert("has_custom_resolution".into(), true);
        assert!(rules_allow(&rules, &c));
    }

    #[test]
    fn windows_version_prefix() {
        let rules = parse_rules(
            json!([{ "action": "allow", "os": { "name": "windows", "version": "^10\\." } }]),
        );
        assert!(rules_allow(&rules, &ctx("windows", "x86_64")));
        let rules = parse_rules(
            json!([{ "action": "allow", "os": { "name": "windows", "version": "^6\\." } }]),
        );
        assert!(!rules_allow(&rules, &ctx("windows", "x86_64")));
    }

    #[test]
    fn conditional_arguments_filtered() {
        let arg: Argument = serde_json::from_value(json!({
            "rules": [{ "action": "allow", "os": { "name": "osx" } }],
            "value": ["-XstartOnFirstThread"]
        }))
        .unwrap();
        assert_eq!(argument_values(&arg, &ctx("osx", "arm64")).len(), 1);
        assert_eq!(argument_values(&arg, &ctx("linux", "x86_64")).len(), 0);
    }
}
