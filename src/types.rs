//! `OpenAPI` types -- delegated to sekkei for canonical definitions,
//! with a compatibility `SchemaOrRef` adapter for openapi-forge consumers.

pub use sekkei::{Components, OpenApiSpec, Operation, PathItem};

// Re-export takumi's FieldType as TypeInfo for backward compatibility.
pub use takumi::FieldType as TypeInfo;

/// Extract the last segment of a `$ref` path (e.g. `Foo` from `#/components/schemas/Foo`).
#[must_use]
pub(crate) fn ref_name_from_path(ref_path: &str) -> Option<&str> {
    ref_path.rsplit('/').next()
}

/// Adapter: convert sekkei's flat `Schema` (with optional `ref_path`) to the
/// `SchemaOrRef` enum pattern used by openapi-forge consumers.
///
/// This preserves API compatibility for code that pattern-matches on
/// `SchemaOrRef::Ref` vs `SchemaOrRef::Schema`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SchemaOrRef {
    Ref { ref_path: String },
    Schema(Box<sekkei::Schema>),
}

impl SchemaOrRef {
    /// Extract the schema name from a `$ref` like `#/components/schemas/Foo`.
    #[must_use]
    pub fn ref_name(&self) -> Option<&str> {
        match self {
            Self::Ref { ref_path } => ref_name_from_path(ref_path),
            Self::Schema(_) => None,
        }
    }

    /// Convert from a sekkei `Schema`.
    ///
    /// If the schema has a `ref_path`, it becomes `SchemaOrRef::Ref`;
    /// otherwise it becomes `SchemaOrRef::Schema`.
    #[must_use]
    #[deprecated(since = "0.2.0", note = "use `SchemaOrRef::from(schema)` instead")]
    pub fn from_schema(schema: &sekkei::Schema) -> Self {
        Self::from(schema)
    }
}

impl From<&sekkei::Schema> for SchemaOrRef {
    fn from(schema: &sekkei::Schema) -> Self {
        if let Some(ref_path) = &schema.ref_path {
            Self::Ref {
                ref_path: ref_path.clone(),
            }
        } else {
            Self::Schema(Box::new(schema.clone()))
        }
    }
}

/// Alias: `SchemaObject` is now `sekkei::Schema`.
pub type SchemaObject = sekkei::Schema;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_name_extracts_last_segment() {
        let sor = SchemaOrRef::Ref {
            ref_path: "#/components/schemas/Foo".to_string(),
        };
        assert_eq!(sor.ref_name(), Some("Foo"));
    }

    #[test]
    fn ref_name_single_segment_ref() {
        let sor = SchemaOrRef::Ref {
            ref_path: "JustAName".to_string(),
        };
        assert_eq!(sor.ref_name(), Some("JustAName"));
    }

    #[test]
    fn ref_name_empty_trailing_slash() {
        let sor = SchemaOrRef::Ref {
            ref_path: "#/components/schemas/".to_string(),
        };
        assert_eq!(sor.ref_name(), Some(""));
    }

    #[test]
    fn ref_name_returns_none_for_schema_variant() {
        let schema = sekkei::Schema::default();
        let sor = SchemaOrRef::Schema(Box::new(schema));
        assert_eq!(sor.ref_name(), None);
    }

    #[test]
    #[allow(deprecated)]
    fn from_schema_with_ref_path_produces_ref_variant() {
        let schema = sekkei::Schema {
            ref_path: Some("#/components/schemas/Bar".to_string()),
            ..sekkei::Schema::default()
        };
        let sor = SchemaOrRef::from_schema(&schema);
        match &sor {
            SchemaOrRef::Ref { ref_path } => {
                assert_eq!(ref_path, "#/components/schemas/Bar");
            }
            SchemaOrRef::Schema(_) => panic!("expected Ref variant"),
        }
    }

    #[test]
    #[allow(deprecated)]
    fn from_schema_without_ref_path_produces_schema_variant() {
        let schema = sekkei::Schema::default();
        let sor = SchemaOrRef::from_schema(&schema);
        assert!(matches!(sor, SchemaOrRef::Schema(_)));
    }

    #[test]
    #[allow(deprecated)]
    fn from_schema_preserves_schema_type() {
        let schema = sekkei::Schema {
            schema_type: Some("string".to_string()),
            description: Some("a test field".to_string()),
            ..sekkei::Schema::default()
        };
        let sor = SchemaOrRef::from_schema(&schema);
        match sor {
            SchemaOrRef::Schema(s) => {
                assert_eq!(s.schema_type.as_deref(), Some("string"));
                assert_eq!(s.description.as_deref(), Some("a test field"));
            }
            SchemaOrRef::Ref { .. } => panic!("expected Schema variant"),
        }
    }

    #[test]
    fn schema_or_ref_debug_impl() {
        let sor = SchemaOrRef::Ref {
            ref_path: "#/test".to_string(),
        };
        let dbg = format!("{sor:?}");
        assert!(dbg.contains("Ref"));
        assert!(dbg.contains("#/test"));
    }

    #[test]
    fn schema_or_ref_clone() {
        let sor = SchemaOrRef::Ref {
            ref_path: "#/components/schemas/Cloned".to_string(),
        };
        let cloned = sor.clone();
        assert_eq!(cloned.ref_name(), Some("Cloned"));
    }

    #[test]
    fn ref_name_deeply_nested_path() {
        let sor = SchemaOrRef::Ref {
            ref_path: "a/b/c/d/e/DeepName".to_string(),
        };
        assert_eq!(sor.ref_name(), Some("DeepName"));
    }

    #[test]
    fn from_trait_ref_variant() {
        let schema = sekkei::Schema {
            ref_path: Some("#/components/schemas/Baz".to_string()),
            ..sekkei::Schema::default()
        };
        let sor = SchemaOrRef::from(&schema);
        assert_eq!(sor.ref_name(), Some("Baz"));
    }

    #[test]
    fn from_trait_schema_variant() {
        let schema = sekkei::Schema::default();
        let sor = SchemaOrRef::from(&schema);
        assert!(matches!(sor, SchemaOrRef::Schema(_)));
    }

    #[test]
    #[allow(deprecated)]
    fn schema_or_ref_from_schema_ref_name_round_trip() {
        let schema = sekkei::Schema {
            ref_path: Some("#/components/schemas/RoundTrip".to_string()),
            ..sekkei::Schema::default()
        };
        let sor = SchemaOrRef::from_schema(&schema);
        assert_eq!(sor.ref_name(), Some("RoundTrip"));
    }
}
