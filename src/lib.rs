mod error;
mod spec;
mod types;

pub use error::ForgeError;
pub use spec::{
    CrudGroup, Endpoint, Field, FieldChange, RpcCrudGrouper, RpcCrudVerb, RpcPattern, SchemaDiff,
    Spec,
};
// TypeInfo is now takumi::FieldType, re-exported via types.
pub use types::{
    Components, OpenApiSpec, Operation, PathItem, SchemaObject, SchemaOrRef, TypeInfo,
};
