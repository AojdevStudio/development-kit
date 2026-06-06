//! Product-namespaced feature keys (issue #36 — product module seam).
//!
//! [`FeatureKey`](crate::FeatureKey) is a **closed** enum: it is the platform
//! spine's fixed vocabulary of gated capabilities, and the feature-key coverage
//! gate iterates it. A product module plugging into the kit (per
//! `docs/PRODUCT-MODULE-SEAM.md`) must be able to declare its OWN gated
//! capabilities **without editing that enum** — editing the foundation is exactly
//! what the seam forbids.
//!
//! [`ProductFeatureKey`] resolves that tension. It is a validated
//! `namespace.name` string key that lives in a key-space **provably disjoint**
//! from the baseline enum: every baseline `FeatureKey` wire string is dotless
//! `snake_case` (`export_pdf`), while every `ProductFeatureKey` carries exactly
//! one `.` separator (`vault.share_record`). The two can never collide on the
//! wire, so a product key can never silently inherit a baseline grant and a
//! baseline key can never be mistaken for a product key.
//!
//! Like [`FeatureKey`](crate::FeatureKey), this type carries ZERO new
//! dependencies (ADR-0002 keeps `shared` types-only) and its wire string is a
//! stable contract once a product ships it.

use serde::{Deserialize, Serialize};

/// The separator between a product feature key's namespace and name. A baseline
/// [`FeatureKey`](crate::FeatureKey) wire string never contains it, which is the
/// invariant that keeps the two key-spaces disjoint.
pub const NAMESPACE_SEPARATOR: char = '.';

/// Why a candidate string is not a valid [`ProductFeatureKey`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductFeatureKeyError {
    /// The namespace segment was empty.
    EmptyNamespace,
    /// The name segment was empty.
    EmptyName,
    /// A segment contained a character outside `[a-z0-9_]` (not `snake_case`),
    /// or the candidate did not have exactly one `namespace.name` split.
    NotSnakeCase(String),
}

impl std::fmt::Display for ProductFeatureKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProductFeatureKeyError::EmptyNamespace => f.write_str("namespace must not be empty"),
            ProductFeatureKeyError::EmptyName => f.write_str("name must not be empty"),
            ProductFeatureKeyError::NotSnakeCase(s) => {
                write!(f, "`{s}` is not a `namespace.name` snake_case product key")
            }
        }
    }
}

impl std::error::Error for ProductFeatureKeyError {}

/// A stable, explicit identifier for a **product-defined** gated capability.
///
/// Constructed only through validation, so an in-memory `ProductFeatureKey` is
/// always a well-formed `namespace.name` `snake_case` key. The product's
/// namespace groups all of its keys (`vault.*`) so two products never collide.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProductFeatureKey {
    /// The full `namespace.name` wire string. Private so the only way to hold a
    /// value is through [`ProductFeatureKey::new`] / the serde path, both of
    /// which validate.
    key: String,
    /// Byte index of the separator in `key`, so `namespace()`/`name()` are
    /// O(1) slices with no re-scan.
    sep: usize,
}

impl ProductFeatureKey {
    /// Build a key from a `namespace` and a `name`, validating both segments.
    ///
    /// Each segment must be non-empty `snake_case` (`[a-z][a-z0-9_]*`). Returns a
    /// [`ProductFeatureKeyError`] otherwise so a malformed key is rejected at the
    /// boundary, never silently shipped.
    pub fn new(
        namespace: impl AsRef<str>,
        name: impl AsRef<str>,
    ) -> Result<Self, ProductFeatureKeyError> {
        let namespace = namespace.as_ref();
        let name = name.as_ref();
        if namespace.is_empty() {
            return Err(ProductFeatureKeyError::EmptyNamespace);
        }
        if name.is_empty() {
            return Err(ProductFeatureKeyError::EmptyName);
        }
        if !is_snake_case_segment(namespace) {
            return Err(ProductFeatureKeyError::NotSnakeCase(namespace.to_string()));
        }
        if !is_snake_case_segment(name) {
            return Err(ProductFeatureKeyError::NotSnakeCase(name.to_string()));
        }
        Ok(ProductFeatureKey {
            sep: namespace.len(),
            key: format!("{namespace}{NAMESPACE_SEPARATOR}{name}"),
        })
    }

    /// Parse a full `namespace.name` wire string into a validated key.
    ///
    /// The candidate must split into exactly two non-empty `snake_case` segments
    /// on a single `.`. Anything else — no `.`, multiple `.`, a baseline dotless
    /// key — is rejected, which is what keeps the product key-space disjoint from
    /// the baseline [`FeatureKey`](crate::FeatureKey) space.
    pub fn parse(candidate: impl AsRef<str>) -> Result<Self, ProductFeatureKeyError> {
        let candidate = candidate.as_ref();
        let mut parts = candidate.split(NAMESPACE_SEPARATOR);
        match (parts.next(), parts.next(), parts.next()) {
            (Some(ns), Some(name), None) => Self::new(ns, name),
            _ => Err(ProductFeatureKeyError::NotSnakeCase(candidate.to_string())),
        }
    }

    /// The stable `namespace.name` wire string. This is the single source of
    /// truth the serde representation is built on, so the JSON the backend emits
    /// and the desktop reads agree byte-for-byte.
    pub fn as_str(&self) -> &str {
        &self.key
    }

    /// The namespace segment (everything before the separator).
    pub fn namespace(&self) -> &str {
        &self.key[..self.sep]
    }

    /// The name segment (everything after the separator).
    pub fn name(&self) -> &str {
        &self.key[self.sep + NAMESPACE_SEPARATOR.len_utf8()..]
    }
}

/// Whether `s` is a non-empty `snake_case` segment: starts with `[a-z]`, then
/// `[a-z0-9_]`. This is the same lowercase-wire-string discipline the baseline
/// keys follow, applied per segment.
fn is_snake_case_segment(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl std::fmt::Display for ProductFeatureKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<ProductFeatureKey> for String {
    fn from(key: ProductFeatureKey) -> String {
        key.key
    }
}

impl TryFrom<String> for ProductFeatureKey {
    type Error = ProductFeatureKeyError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        ProductFeatureKey::parse(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FeatureKey;

    // --- ISC-1: a namespaced key constructs ---
    #[test]
    fn new_builds_a_namespaced_key() {
        let key = ProductFeatureKey::new("vault", "share_record").expect("valid key");
        assert_eq!(key.namespace(), "vault");
        assert_eq!(key.name(), "share_record");
    }

    // --- ISC-2: empty segments are rejected ---
    #[test]
    fn empty_namespace_is_rejected() {
        assert_eq!(
            ProductFeatureKey::new("", "x"),
            Err(ProductFeatureKeyError::EmptyNamespace)
        );
    }

    #[test]
    fn empty_name_is_rejected() {
        assert_eq!(
            ProductFeatureKey::new("vault", ""),
            Err(ProductFeatureKeyError::EmptyName)
        );
    }

    // --- ISC-3: non-snake_case shape is rejected ---
    #[test]
    fn non_snake_case_segments_are_rejected() {
        assert!(matches!(
            ProductFeatureKey::new("Vault", "share_record"),
            Err(ProductFeatureKeyError::NotSnakeCase(_))
        ));
        assert!(matches!(
            ProductFeatureKey::new("vault", "shareRecord"),
            Err(ProductFeatureKeyError::NotSnakeCase(_))
        ));
        // a segment may not start with a digit or underscore
        assert!(matches!(
            ProductFeatureKey::new("1vault", "x"),
            Err(ProductFeatureKeyError::NotSnakeCase(_))
        ));
    }

    #[test]
    fn parse_rejects_a_candidate_without_exactly_one_dot() {
        // no dot at all (looks like a baseline key)
        assert!(ProductFeatureKey::parse("export_pdf").is_err());
        // two dots
        assert!(ProductFeatureKey::parse("a.b.c").is_err());
    }

    // --- ISC-3 (airtight): malformed wire strings the validator must reject ---
    #[test]
    fn parse_rejects_malformed_wire_strings() {
        // The wire string is forever once shipped, so the validator must be
        // airtight against every degenerate shape, not just "empty" and "no dot".
        for bad in [
            ".foo",     // empty namespace
            "foo.",     // empty name
            "foo..bar", // empty middle / double dot
            "foo.Bar",  // non-snake_case name
            "Foo.bar",  // non-snake_case namespace
            "_foo.bar", // leading underscore namespace
            "foo._bar", // leading underscore name
            "foo.1bar", // name starts with a digit
            "1foo.bar", // namespace starts with a digit
            "foo.bar.", // trailing dot
            "",         // empty
        ] {
            assert!(
                ProductFeatureKey::parse(bad).is_err(),
                "`{bad}` must be rejected"
            );
        }
    }

    // --- ISC-4: as_str is the namespace.name wire string ---
    #[test]
    fn as_str_is_the_namespace_dot_name_wire_string() {
        let key = ProductFeatureKey::new("vault", "share_record").unwrap();
        assert_eq!(key.as_str(), "vault.share_record");
    }

    // --- ISC-5: serde round-trips as the wire string ---
    #[test]
    fn serde_round_trips_as_the_wire_string() {
        let key = ProductFeatureKey::new("vault", "share_record").unwrap();
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"vault.share_record\"");
        let back: ProductFeatureKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    fn serde_rejects_a_malformed_wire_string() {
        // The serde path validates, so a forged dotless string never deserializes
        // into a product key — the disjointness invariant holds across the wire.
        let result: Result<ProductFeatureKey, _> = serde_json::from_str("\"export_pdf\"");
        assert!(result.is_err());
    }

    // --- ISC-6: product keys can never collide with baseline keys ---
    #[test]
    fn product_keys_are_disjoint_from_baseline_feature_keys() {
        // Every baseline key is dotless; every product key has exactly one dot.
        for baseline in FeatureKey::ALL {
            assert!(
                !baseline.as_str().contains(NAMESPACE_SEPARATOR),
                "baseline key `{}` must be dotless",
                baseline.as_str()
            );
            // A baseline wire string can never parse as a product key.
            assert!(
                ProductFeatureKey::parse(baseline.as_str()).is_err(),
                "baseline key `{}` must not parse as a product key",
                baseline.as_str()
            );
        }
        // And a product key always carries the separator.
        let product = ProductFeatureKey::new("vault", "share_record").unwrap();
        assert!(product.as_str().contains(NAMESPACE_SEPARATOR));
    }
}
