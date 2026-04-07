//! `OpenAPI` 3.0 parser, CRUD grouper, and schema diffing library.
//!
//! `openapi-forge` provides a high-level [`Spec`] type that wraps a parsed
//! `OpenAPI` document and offers helpers for:
//!
//! - Endpoint enumeration and lookup
//! - Schema field resolution (including `allOf` composition)
//! - Schema diffing (added / removed / changed fields)
//! - Heuristic and configurable CRUD grouping of RPC-style endpoints

mod error;
mod spec;
mod types;

pub use error::ForgeError;
pub use spec::{
    CrudGroup, Endpoint, Field, FieldChange, RpcCrudGrouper, RpcCrudVerb, RpcPattern, SchemaDiff,
    Spec,
};
pub use types::{
    Components, OpenApiSpec, Operation, PathItem, SchemaObject, SchemaOrRef, TypeInfo,
};
