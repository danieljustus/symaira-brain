//! The complete Brain-owned value, distinct from stored maps and Memory config.
use crate::GoText;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct BrainConfig {
    pub default_profile: GoText,
    pub audit: Audit,
    pub gateway: Gateway,
    pub updatecheck: UpdateCheck,
    pub servers: Servers,
    pub patterns: Patterns,
    pub modules: Modules,
}
#[derive(Debug, Clone)]
pub struct Audit {
    pub enabled: bool,
    pub verbose: bool,
}
#[derive(Debug, Clone)]
pub struct Gateway {
    pub identity_injection: bool,
}
#[derive(Debug, Clone)]
pub struct UpdateCheck {
    pub enabled: bool,
}
#[derive(Debug, Clone, Default)]
pub struct Servers {
    pub vault: GoText,
    pub operate: GoText,
    pub scope: GoText,
}
#[derive(Debug, Clone)]
pub struct Patterns {
    pub enabled: bool,
    pub promotion_threshold: i64,
}
#[derive(Debug, Clone, Default)]
pub struct Modules {
    pub browse: bool,
    pub operate: bool,
    pub scope: bool,
}
impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            default_profile: GoText::default(),
            audit: Audit {
                enabled: true,
                verbose: false,
            },
            gateway: Gateway {
                identity_injection: true,
            },
            updatecheck: UpdateCheck { enabled: true },
            servers: Servers::default(),
            patterns: Patterns {
                enabled: true,
                promotion_threshold: 3,
            },
            modules: Modules::default(),
        }
    }
}
impl Modules {
    /// Release manifests intentionally admit only Browse as an optional core.
    #[must_use]
    pub fn enabled_cores(&self) -> BTreeMap<String, bool> {
        if self.browse {
            BTreeMap::from([("symbrowse".into(), true)])
        } else {
            BTreeMap::new()
        }
    }
    /// Historical source workers retain all three module selections.
    #[must_use]
    pub fn enabled_modules(&self) -> BTreeMap<String, bool> {
        [
            ("symbrowse", self.browse),
            ("symoperate", self.operate),
            ("symscope", self.scope),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
    }
}
