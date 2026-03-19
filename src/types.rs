//! OpenAPI types -- delegated to sekkei for canonical definitions,
//! with a compatibility `SchemaOrRef` adapter for openapi-forge consumers.

pub use sekkei::{Components, OpenApiSpec, Operation, PathItem};

// Re-export takumi's FieldType as TypeInfo for backward compatibility.
pub use takumi::FieldType as TypeInfo;

/// Adapter: convert sekkei's flat `Schema` (with optional `ref_path`) to the
/// `SchemaOrRef` enum pattern used by openapi-forge consumers.
///
/// This preserves API compatibility for code that pattern-matches on
/// `SchemaOrRef::Ref` vs `SchemaOrRef::Schema`.
#[derive(Debug, Clone)]
pub enum SchemaOrRef {
    Ref { ref_path: String },
    Schema(Box<sekkei::Schema>),
}

impl SchemaOrRef {
    /// Extract the schema name from a `$ref` like `#/components/schemas/Foo`.
    #[must_use]
    pub fn ref_name(&self) -> Option<&str> {
        match self {
            Self::Ref { ref_path } => ref_path.rsplit('/').next(),
            Self::Schema(_) => None,
        }
    }

    /// Convert from a sekkei `Schema`.
    ///
    /// If the schema has a `ref_path`, it becomes `SchemaOrRef::Ref`;
    /// otherwise it becomes `SchemaOrRef::Schema`.
    #[must_use]
    pub fn from_schema(schema: &sekkei::Schema) -> Self {
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
