//! Typed facet filters for [`crate::client::ModrinthClient::search`].
//!
//! Modrinth's facet syntax is a JSON array of arrays of `"field:value"`
//! strings — arrays are OR'd together, the outer list is AND'd. Getting
//! that nesting (and the field names — mod loaders are filtered through
//! the *same* `categories:` field as ordinary categories, which is easy to
//! miss) subtly wrong by hand-formatting strings is exactly what this
//! builder exists to prevent.

use serde::Serialize;

/// One facet condition. Each variant maps to Modrinth's `field:value`
/// syntax on the wire; [`Facet::Loader`] is a separate constructor from
/// [`Facet::Category`] purely for caller ergonomics — both serialize
/// through the same `categories:` field, since Modrinth models mod loaders
/// as a kind of category rather than giving them their own facet field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Facet {
    /// A project category, e.g. `"adventure"`.
    Category(String),
    /// A mod loader, e.g. `"fabric"`. Wire-identical to [`Facet::Category`]
    /// but kept as its own variant so callers don't have to know that.
    Loader(String),
    /// A supported Minecraft version, e.g. `"1.21.1"`.
    Version(String),
    /// A project type: `"mod"`, `"modpack"`, `"resourcepack"`, or
    /// `"shader"`.
    ProjectType(String),
    /// An SPDX license id, e.g. `"MIT"`.
    License(String),
}

impl Facet {
    /// A `categories:<value>` facet.
    pub fn category(value: impl Into<String>) -> Self {
        Facet::Category(value.into())
    }

    /// A `categories:<value>` facet for a mod loader specifically — see
    /// [`Facet::Loader`]'s doc comment for why this is distinct from
    /// [`Facet::category`] despite serializing identically.
    pub fn loader(value: impl Into<String>) -> Self {
        Facet::Loader(value.into())
    }

    /// A `versions:<value>` facet for a Minecraft version.
    pub fn version(value: impl Into<String>) -> Self {
        Facet::Version(value.into())
    }

    /// A `project_type:<value>` facet.
    pub fn project_type(value: impl Into<String>) -> Self {
        Facet::ProjectType(value.into())
    }

    /// A `license:<value>` facet.
    pub fn license(value: impl Into<String>) -> Self {
        Facet::License(value.into())
    }

    /// This facet's `"field:value"` wire form.
    fn to_wire(&self) -> String {
        match self {
            Facet::Category(v) | Facet::Loader(v) => format!("categories:{v}"),
            Facet::Version(v) => format!("versions:{v}"),
            Facet::ProjectType(v) => format!("project_type:{v}"),
            Facet::License(v) => format!("license:{v}"),
        }
    }
}

/// A built, ready-to-send facet filter — an AND of OR groups, matching
/// Modrinth's `[["a"],["b","c"]]` shape exactly. Build one via
/// [`FacetsBuilder`] rather than constructing this directly.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Facets(Vec<Vec<String>>);

impl Facets {
    /// The exact JSON array-of-arrays string Modrinth's `facets` query
    /// parameter expects, e.g. `[["categories:forge"],["versions:1.17.1"]]`
    /// (`reqwest` handles URL-encoding it when it's set as a query value).
    pub fn to_query_value(&self) -> String {
        serde_json::to_string(&self.0).expect("Vec<Vec<String>> always serializes")
    }

    /// True if no groups were added — callers use this to skip sending the
    /// `facets` param at all rather than sending `[]`, which Modrinth would
    /// read as "no results match no constraints."
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Builds a [`Facets`] value one AND'd group at a time.
#[derive(Debug, Clone, Default)]
pub struct FacetsBuilder {
    groups: Vec<Vec<String>>,
}

impl FacetsBuilder {
    /// An empty builder — equivalent to no filtering at all once built.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one AND'd group, its members OR'd together (e.g. "fabric OR
    /// quilt, AND 1.21.1"). An empty group is silently dropped rather than
    /// emitting `[]` into the array, which Modrinth treats as an
    /// unsatisfiable constraint rather than a no-op.
    pub fn group(mut self, facets: impl IntoIterator<Item = Facet>) -> Self {
        let group: Vec<String> = facets.into_iter().map(|f| f.to_wire()).collect();
        if !group.is_empty() {
            self.groups.push(group);
        }
        self
    }

    /// Add a single facet as its own AND'd group — the common case where
    /// there's nothing to OR it with.
    pub fn and(self, facet: Facet) -> Self {
        self.group([facet])
    }

    /// Finish building.
    pub fn build(self) -> Facets {
        Facets(self.groups)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The single easiest thing to get wrong in this crate: Modrinth's
    /// facets param is a JSON array of arrays, and a hand-formatted string
    /// can silently produce the wrong nesting (all AND'd, or all OR'd)
    /// without any error surfacing until search results are subtly wrong.
    #[test]
    fn builds_exact_array_of_arrays_shape() {
        let facets = FacetsBuilder::new()
            .and(Facet::category("adventure"))
            .and(Facet::version("1.21.1"))
            .build();
        assert_eq!(
            facets.to_query_value(),
            r#"[["categories:adventure"],["versions:1.21.1"]]"#
        );
    }

    #[test]
    fn ors_within_a_group() {
        let facets = FacetsBuilder::new()
            .group([Facet::loader("fabric"), Facet::loader("quilt")])
            .build();
        assert_eq!(
            facets.to_query_value(),
            r#"[["categories:fabric","categories:quilt"]]"#
        );
    }

    #[test]
    fn loader_and_category_share_the_categories_field() {
        assert_eq!(
            Facet::loader("forge").to_wire(),
            Facet::category("forge").to_wire()
        );
    }

    #[test]
    fn project_type_and_license_facets() {
        let facets = FacetsBuilder::new()
            .and(Facet::project_type("mod"))
            .and(Facet::license("MIT"))
            .build();
        assert_eq!(
            facets.to_query_value(),
            r#"[["project_type:mod"],["license:MIT"]]"#
        );
    }

    #[test]
    fn empty_group_is_dropped_not_emitted_as_empty_array() {
        let facets = FacetsBuilder::new()
            .group(Vec::<Facet>::new())
            .and(Facet::version("1.20.1"))
            .build();
        assert_eq!(facets.to_query_value(), r#"[["versions:1.20.1"]]"#);
    }

    #[test]
    fn empty_builder_is_empty() {
        assert!(FacetsBuilder::new().build().is_empty());
    }
}
