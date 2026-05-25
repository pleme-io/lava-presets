//! lava-presets — typed Profile overlays.
//!
//! Pangea's `Pangea::Presets.apply(config, PROFILES)` analog. A
//! [`Preset`] is a named bag of bindings; [`PresetRegistry::apply`]
//! resolves the precedence chain `defaults < preset < operator`.
//!
//! ## Form
//!
//! ```lisp
//! (deflava-preset public-dns/production
//!   :bindings (:dnssec-enabled "true"
//!              :query-logging-enabled "true"
//!              :retention-days "365"))
//!
//! (deflava-preset public-dns/dev
//!   :bindings (:dnssec-enabled "false"
//!              :query-logging-enabled "false"
//!              :retention-days "7"))
//! ```
//!
//! ## Usage
//!
//! ```rust
//! use lava_presets::{Preset, PresetRegistry};
//! use indexmap::IndexMap;
//!
//! let mut reg = PresetRegistry::new();
//! reg.register(Preset::new("prod").with("retention-days", "365"));
//!
//! let mut operator_bindings = IndexMap::new();
//! // operator-supplied bindings take precedence over the preset:
//! operator_bindings.insert("retention-days".into(), "90".into());
//!
//! let merged = reg.apply("prod", &operator_bindings).unwrap();
//! assert_eq!(merged["retention-days"], "90");
//! ```

#![allow(clippy::module_name_repetitions)]

use indexmap::IndexMap;
use lava_eval::{parse_all, Sx};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One named preset = a typed bindings overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    pub bindings: IndexMap<String, String>,
}

impl Preset {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            doc: None,
            bindings: IndexMap::new(),
        }
    }

    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.bindings.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }
}

/// Registry of typed presets the operator + CLI lookup by name.
#[derive(Debug, Default)]
pub struct PresetRegistry {
    by_name: IndexMap<String, Preset>,
}

impl PresetRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, p: Preset) {
        self.by_name.insert(p.name.clone(), p);
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Preset> {
        self.by_name.get(name)
    }

    #[must_use]
    pub fn names(&self) -> Vec<&String> {
        self.by_name.keys().collect()
    }

    /// Apply the named preset on top of operator-supplied bindings.
    /// Precedence: operator > preset (so operators can override any
    /// preset value at call time, matching pangea's
    /// `config.merge(profile)` shape where the caller wins).
    ///
    /// # Errors
    /// Returns [`PresetError::Unknown`] if `name` isn't registered.
    pub fn apply(
        &self,
        name: &str,
        operator_bindings: &IndexMap<String, String>,
    ) -> Result<IndexMap<String, String>, PresetError> {
        let preset = self
            .get(name)
            .ok_or_else(|| PresetError::Unknown(name.to_string()))?;
        let mut merged: IndexMap<String, String> = IndexMap::new();
        for (k, v) in &preset.bindings {
            merged.insert(k.clone(), v.clone());
        }
        for (k, v) in operator_bindings {
            merged.insert(k.clone(), v.clone());
        }
        Ok(merged)
    }

    /// Layer a preset over an arbitrary base (e.g. architecture defaults
    /// already resolved). Returns a new bag; the registry stays
    /// immutable.
    #[must_use]
    pub fn layer(
        &self,
        name: &str,
        base: &IndexMap<String, String>,
    ) -> Option<IndexMap<String, String>> {
        let preset = self.get(name)?;
        let mut merged: IndexMap<String, String> = base.clone();
        for (k, v) in &preset.bindings {
            merged.insert(k.clone(), v.clone());
        }
        Some(merged)
    }
}

#[derive(Debug, Error)]
pub enum PresetError {
    #[error("unknown preset `{0}`")]
    Unknown(String),
    #[error("parse: {0}")]
    Parse(#[from] lava_eval::ParseError),
    #[error("malformed deflava-preset form: {0}")]
    Malformed(String),
}

/// Scan a source string for every `(deflava-preset …)` form and
/// return one [`Preset`] per declaration.
///
/// # Errors
/// Parse errors and per-preset shape errors surface as typed
/// [`PresetError`] variants.
pub fn presets_in_source(src: &str) -> Result<Vec<Preset>, PresetError> {
    let forms = parse_all(src)?;
    let mut out = Vec::new();
    for form in forms {
        let Some(xs) = form.as_list() else { continue };
        if xs.first().and_then(Sx::as_sym) == Some("deflava-preset") {
            out.push(preset_from_form(xs)?);
        }
    }
    Ok(out)
}

fn preset_from_form(xs: &[Sx]) -> Result<Preset, PresetError> {
    let name = xs
        .get(1)
        .and_then(Sx::as_sym)
        .or_else(|| xs.get(1).and_then(Sx::as_str))
        .ok_or_else(|| PresetError::Malformed("missing preset name".into()))?
        .to_string();
    let mut preset = Preset::new(name);
    let mut i = 2;
    while i + 1 < xs.len() {
        match xs[i].as_kw() {
            Some("doc") => {
                preset.doc = xs[i + 1].as_str().map(std::string::ToString::to_string);
            }
            Some("bindings") => {
                if let Some(pairs) = xs[i + 1].as_list() {
                    let mut j = 0;
                    while j + 1 < pairs.len() {
                        if let (Some(k), Some(v)) =
                            (pairs[j].as_kw(), pairs[j + 1].as_str())
                        {
                            preset.bindings.insert(k.to_string(), v.to_string());
                        }
                        j += 2;
                    }
                }
            }
            _ => {}
        }
        i += 2;
    }
    Ok(preset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_merges_preset_into_operator_bag_with_operator_winning() {
        let mut reg = PresetRegistry::new();
        reg.register(
            Preset::new("prod")
                .with("retention-days", "365")
                .with("dnssec-enabled", "true"),
        );
        let mut op = IndexMap::new();
        op.insert("retention-days".into(), "90".into()); // operator override
        let merged = reg.apply("prod", &op).unwrap();
        assert_eq!(merged["retention-days"], "90");
        assert_eq!(merged["dnssec-enabled"], "true");
    }

    #[test]
    fn apply_unknown_preset_surfaces_typed_error() {
        let reg = PresetRegistry::new();
        let err = reg.apply("nope", &IndexMap::new()).unwrap_err();
        matches!(err, PresetError::Unknown(_));
    }

    #[test]
    fn layer_returns_none_for_unknown_preset() {
        let reg = PresetRegistry::new();
        assert!(reg.layer("nope", &IndexMap::new()).is_none());
    }

    #[test]
    fn layer_keeps_base_values_not_overridden_by_preset() {
        let mut reg = PresetRegistry::new();
        reg.register(Preset::new("prod").with("retention-days", "365"));
        let mut base = IndexMap::new();
        base.insert("region".into(), "us-east-2".into());
        let merged = reg.layer("prod", &base).unwrap();
        assert_eq!(merged["region"], "us-east-2");
        assert_eq!(merged["retention-days"], "365");
    }

    #[test]
    fn presets_in_source_extracts_typed_presets() {
        let src = r#"
            (deflava-preset prod
              :doc "Production-grade defaults"
              :bindings (:dnssec-enabled "true"
                         :retention-days "365"))

            (deflava-preset dev
              :bindings (:dnssec-enabled "false"
                         :retention-days "7"))
        "#;
        let presets = presets_in_source(src).unwrap();
        assert_eq!(presets.len(), 2);
        assert_eq!(presets[0].name, "prod");
        assert_eq!(presets[0].doc.as_deref(), Some("Production-grade defaults"));
        assert_eq!(presets[0].bindings["dnssec-enabled"], "true");
        assert_eq!(presets[1].bindings["retention-days"], "7");
    }

    #[test]
    fn missing_preset_name_surfaces_typed_error() {
        let src = "(deflava-preset)";
        let err = presets_in_source(src).unwrap_err();
        matches!(err, PresetError::Malformed(_));
    }

    #[test]
    fn preset_round_trips_through_serde() {
        let p = Preset::new("x").with("a", "1").with_doc("doc");
        let json = serde_json::to_string(&p).unwrap();
        let parsed: Preset = serde_json::from_str(&json).unwrap();
        assert_eq!(p, parsed);
    }
}
