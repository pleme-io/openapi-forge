use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Represents an OpenAPI 3.0 specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiSpec {
    pub openapi: String,
    #[serde(default)]
    pub info: Info,
    #[serde(default)]
    pub servers: Vec<Server>,
    #[serde(default)]
    pub paths: IndexMap<String, PathItem>,
    #[serde(default)]
    pub components: Components,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Info {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub contact: Option<Contact>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Contact {
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Server {
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathItem {
    #[serde(default)]
    pub get: Option<Operation>,
    #[serde(default)]
    pub post: Option<Operation>,
    #[serde(default)]
    pub put: Option<Operation>,
    #[serde(default)]
    pub delete: Option<Operation>,
    #[serde(default)]
    pub patch: Option<Operation>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub request_body: Option<RequestBody>,
    #[serde(default)]
    pub responses: IndexMap<String, Response>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestBody {
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub content: IndexMap<String, MediaType>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaType {
    #[serde(default)]
    pub schema: Option<SchemaOrRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub content: IndexMap<String, MediaType>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Parameter {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "in")]
    pub location: String,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub schema: Option<SchemaOrRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Components {
    #[serde(default)]
    pub schemas: IndexMap<String, SchemaObject>,
}

/// A schema that can be either an inline object or a `$ref`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SchemaOrRef {
    Ref {
        #[serde(rename = "$ref")]
        ref_path: String,
    },
    Schema(Box<SchemaObject>),
}

impl Default for SchemaOrRef {
    fn default() -> Self {
        Self::Schema(Box::default())
    }
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
}

/// An OpenAPI schema object.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaObject {
    #[serde(default, rename = "type")]
    pub schema_type: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub properties: IndexMap<String, SchemaOrRef>,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub items: Option<Box<SchemaOrRef>>,
    #[serde(default, rename = "enum")]
    pub enum_values: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default, rename = "allOf")]
    pub all_of: Option<Vec<SchemaOrRef>>,
    #[serde(default, rename = "oneOf")]
    pub one_of: Option<Vec<SchemaOrRef>>,
    #[serde(default, rename = "anyOf")]
    pub any_of: Option<Vec<SchemaOrRef>>,
    #[serde(default, rename = "additionalProperties")]
    pub additional_properties: Option<Box<SchemaOrRef>>,
    #[serde(default, rename = "x-go-name")]
    pub x_go_name: Option<String>,
}
