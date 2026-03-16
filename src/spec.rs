use std::path::Path;

use indexmap::IndexMap;

use crate::error::ForgeError;
use crate::types::{OpenApiSpec, Operation, SchemaObject, SchemaOrRef};

/// High-level representation of a parsed and queryable OpenAPI spec.
#[derive(Debug, Clone)]
pub struct Spec {
    raw: OpenApiSpec,
}

/// A resolved endpoint with its request/response schema names.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub path: String,
    pub method: String,
    pub operation_id: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub request_schema_ref: Option<String>,
    pub response_schema_ref: Option<String>,
}

/// A resolved field from a schema.
#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub type_info: TypeInfo,
    pub required: bool,
    pub description: Option<String>,
    pub default: Option<serde_json::Value>,
    pub format: Option<String>,
}

/// Type information for a schema field.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeInfo {
    String,
    Integer,
    Number,
    Boolean,
    Array(Box<TypeInfo>),
    Object(String),
    Map(Box<TypeInfo>),
    Any,
}

impl std::fmt::Display for TypeInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String => write!(f, "string"),
            Self::Integer => write!(f, "integer"),
            Self::Number => write!(f, "number"),
            Self::Boolean => write!(f, "boolean"),
            Self::Array(inner) => write!(f, "array<{inner}>"),
            Self::Object(name) => write!(f, "object<{name}>"),
            Self::Map(inner) => write!(f, "map<{inner}>"),
            Self::Any => write!(f, "any"),
        }
    }
}

/// Result of diffing two schemas.
#[derive(Debug, Clone)]
pub struct SchemaDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<FieldChange>,
}

/// A field that changed between two schema versions.
#[derive(Debug, Clone)]
pub struct FieldChange {
    pub name: String,
    pub old_type: TypeInfo,
    pub new_type: TypeInfo,
    pub required_changed: bool,
}

/// A group of related CRUD endpoints.
#[derive(Debug, Clone)]
pub struct CrudGroup {
    pub base_name: String,
    pub create: Option<Endpoint>,
    pub read: Option<Endpoint>,
    pub update: Option<Endpoint>,
    pub delete: Option<Endpoint>,
    pub list: Option<Endpoint>,
}

impl Spec {
    /// Load an OpenAPI spec from a YAML or JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, parsed, or is not OpenAPI 3.0.x.
    pub fn load(path: &Path) -> Result<Self, ForgeError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    /// Parse an OpenAPI spec from a string (YAML or JSON).
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed or is not OpenAPI 3.0.x.
    pub fn from_str(content: &str) -> Result<Self, ForgeError> {
        let raw: OpenApiSpec = if content.trim_start().starts_with('{') {
            serde_json::from_str(content)?
        } else {
            serde_yaml::from_str(content)?
        };

        if !raw.openapi.starts_with("3.0") {
            return Err(ForgeError::UnsupportedVersion(raw.openapi.clone()));
        }

        Ok(Self { raw })
    }

    /// Get all endpoints in the spec.
    #[must_use]
    pub fn endpoints(&self) -> Vec<Endpoint> {
        let mut result = Vec::new();

        for (path, item) in &self.raw.paths {
            let methods: &[(&str, &Option<Operation>)] = &[
                ("get", &item.get),
                ("post", &item.post),
                ("put", &item.put),
                ("delete", &item.delete),
                ("patch", &item.patch),
            ];

            for &(method, op) in methods {
                if let Some(operation) = op {
                    result.push(Endpoint {
                        path: path.clone(),
                        method: method.to_string(),
                        operation_id: operation.operation_id.clone(),
                        summary: operation.summary.clone(),
                        tags: operation.tags.clone(),
                        request_schema_ref: Self::extract_request_ref(operation),
                        response_schema_ref: Self::extract_response_ref(operation),
                    });
                }
            }
        }

        result
    }

    /// Look up a component schema by name.
    ///
    /// # Errors
    ///
    /// Returns `SchemaNotFound` if the schema name does not exist.
    pub fn schema(&self, name: &str) -> Result<&SchemaObject, ForgeError> {
        self.raw
            .components
            .schemas
            .get(name)
            .ok_or_else(|| ForgeError::SchemaNotFound(name.to_string()))
    }

    /// Get all component schema names.
    #[must_use]
    pub fn schema_names(&self) -> Vec<&str> {
        self.raw
            .components
            .schemas
            .keys()
            .map(String::as_str)
            .collect()
    }

    /// Resolve fields from a named schema, including type info and required status.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema is not found.
    pub fn fields(&self, schema_name: &str) -> Result<Vec<Field>, ForgeError> {
        let schema = self.schema(schema_name)?;
        Ok(self.resolve_fields(schema))
    }

    /// Resolve the type info for a `SchemaOrRef`.
    #[must_use]
    pub fn resolve_type(&self, schema_or_ref: &SchemaOrRef) -> TypeInfo {
        match schema_or_ref {
            SchemaOrRef::Ref { ref_path } => {
                let name = ref_path.rsplit('/').next().unwrap_or("Unknown");
                TypeInfo::Object(name.to_string())
            }
            SchemaOrRef::Schema(schema) => self.type_from_schema(schema),
        }
    }

    /// Diff two schemas by name, showing added/removed/changed fields.
    ///
    /// # Errors
    ///
    /// Returns an error if either schema is not found.
    pub fn diff_schemas(&self, name_a: &str, name_b: &str) -> Result<SchemaDiff, ForgeError> {
        let fields_a = self.fields(name_a)?;
        let fields_b = self.fields(name_b)?;

        let map_a: IndexMap<&str, &Field> = fields_a.iter().map(|f| (f.name.as_str(), f)).collect();
        let map_b: IndexMap<&str, &Field> = fields_b.iter().map(|f| (f.name.as_str(), f)).collect();

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();

        for (name, field_b) in &map_b {
            if let Some(field_a) = map_a.get(name) {
                if field_a.type_info != field_b.type_info || field_a.required != field_b.required {
                    changed.push(FieldChange {
                        name: (*name).to_string(),
                        old_type: field_a.type_info.clone(),
                        new_type: field_b.type_info.clone(),
                        required_changed: field_a.required != field_b.required,
                    });
                }
            } else {
                added.push((*name).to_string());
            }
        }

        for name in map_a.keys() {
            if !map_b.contains_key(name) {
                removed.push((*name).to_string());
            }
        }

        Ok(SchemaDiff {
            added,
            removed,
            changed,
        })
    }

    /// Heuristic CRUD grouping of endpoints by operation pattern.
    ///
    /// Groups endpoints like `/create-secret`, `/get-secret-value`,
    /// `/update-secret-val`, `/delete-item` into clusters by matching
    /// create/get/update/delete prefixes.
    #[must_use]
    pub fn group_by_crud_pattern(&self) -> Vec<CrudGroup> {
        let endpoints = self.endpoints();
        let mut groups: IndexMap<String, CrudGroup> = IndexMap::new();

        for ep in &endpoints {
            let path = ep.path.trim_start_matches('/');
            let op_id = ep.operation_id.as_deref().unwrap_or(path);

            // Detect CRUD verb prefix
            let (verb, base) = detect_crud_verb(op_id);
            if base.is_empty() {
                continue;
            }

            let group = groups.entry(base.clone()).or_insert_with(|| CrudGroup {
                base_name: base,
                create: None,
                read: None,
                update: None,
                delete: None,
                list: None,
            });

            match verb {
                CrudVerb::Create => group.create = Some(ep.clone()),
                CrudVerb::Read => group.read = Some(ep.clone()),
                CrudVerb::Update => group.update = Some(ep.clone()),
                CrudVerb::Delete => group.delete = Some(ep.clone()),
                CrudVerb::List => group.list = Some(ep.clone()),
                CrudVerb::None => {}
            }
        }

        groups.into_values().collect()
    }

    /// Find an endpoint by its path.
    #[must_use]
    pub fn endpoint_by_path(&self, path: &str) -> Option<Endpoint> {
        self.endpoints().into_iter().find(|e| e.path == path)
    }

    // --- private helpers ---

    fn resolve_fields(&self, schema: &SchemaObject) -> Vec<Field> {
        let mut fields = Vec::new();

        // Handle allOf by merging properties
        if let Some(all_of) = &schema.all_of {
            for item in all_of {
                match item {
                    SchemaOrRef::Ref { ref_path } => {
                        if let Some(name) = ref_path.rsplit('/').next() {
                            if let Ok(referenced) = self.schema(name) {
                                fields.extend(self.resolve_fields(referenced));
                            }
                        }
                    }
                    SchemaOrRef::Schema(s) => {
                        fields.extend(self.resolve_fields(s));
                    }
                }
            }
        }

        for (name, prop) in &schema.properties {
            let required = schema.required.contains(name);
            let type_info = self.resolve_type(prop);
            let (description, default, format) = match prop {
                SchemaOrRef::Schema(s) => {
                    (s.description.clone(), s.default.clone(), s.format.clone())
                }
                SchemaOrRef::Ref { .. } => (None, None, None),
            };

            fields.push(Field {
                name: name.clone(),
                type_info,
                required,
                description,
                default,
                format,
            });
        }

        fields
    }

    fn type_from_schema(&self, schema: &SchemaObject) -> TypeInfo {
        match schema.schema_type.as_deref() {
            Some("string") => TypeInfo::String,
            Some("integer") => TypeInfo::Integer,
            Some("number") => TypeInfo::Number,
            Some("boolean") => TypeInfo::Boolean,
            Some("array") => {
                let inner = schema
                    .items
                    .as_ref()
                    .map_or(TypeInfo::Any, |items| self.resolve_type(items));
                TypeInfo::Array(Box::new(inner))
            }
            Some("object") => {
                if let Some(additional) = &schema.additional_properties {
                    let inner = self.resolve_type(additional);
                    TypeInfo::Map(Box::new(inner))
                } else {
                    TypeInfo::Object("inline".to_string())
                }
            }
            _ => TypeInfo::Any,
        }
    }

    fn extract_request_ref(op: &Operation) -> Option<String> {
        let body = op.request_body.as_ref()?;
        let media = body.content.get("application/json")?;
        let schema = media.schema.as_ref()?;
        schema.ref_name().map(String::from)
    }

    fn extract_response_ref(op: &Operation) -> Option<String> {
        // Try 200, 201, then default
        let response = op
            .responses
            .get("200")
            .or_else(|| op.responses.get("201"))
            .or_else(|| op.responses.get("default"))?;
        let media = response.content.get("application/json")?;
        let schema = media.schema.as_ref()?;
        schema.ref_name().map(String::from)
    }
}

enum CrudVerb {
    Create,
    Read,
    Update,
    Delete,
    List,
    None,
}

fn detect_crud_verb(operation_id: &str) -> (CrudVerb, String) {
    // Normalize: remove hyphens, lowercase
    let normalized = operation_id.replace('-', "").to_lowercase();

    let prefixes: &[(&str, CrudVerb)] = &[
        ("create", CrudVerb::Create),
        ("add", CrudVerb::Create),
        ("get", CrudVerb::Read),
        ("describe", CrudVerb::Read),
        ("update", CrudVerb::Update),
        ("delete", CrudVerb::Delete),
        ("remove", CrudVerb::Delete),
        ("list", CrudVerb::List),
    ];

    for (prefix, verb) in prefixes {
        if normalized.starts_with(prefix) {
            let base = &normalized[prefix.len()..];
            // Also try with the original hyphenated form for base name
            let original_base = strip_verb_prefix(operation_id);
            let name = if original_base.is_empty() {
                base.to_string()
            } else {
                original_base
            };
            return (
                match verb {
                    CrudVerb::Create => CrudVerb::Create,
                    CrudVerb::Read => CrudVerb::Read,
                    CrudVerb::Update => CrudVerb::Update,
                    CrudVerb::Delete => CrudVerb::Delete,
                    CrudVerb::List => CrudVerb::List,
                    CrudVerb::None => CrudVerb::None,
                },
                name,
            );
        }
    }

    (CrudVerb::None, String::new())
}

fn strip_verb_prefix(operation_id: &str) -> String {
    let verbs = [
        "create-",
        "add-",
        "get-",
        "describe-",
        "update-",
        "delete-",
        "remove-",
        "list-",
    ];
    for verb in &verbs {
        if let Some(rest) = operation_id.to_lowercase().strip_prefix(verb) {
            return rest.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_SPEC: &str = r#"
openapi: "3.0.0"
info:
  title: Test API
  version: "1.0"
paths:
  /create-secret:
    post:
      operationId: createSecret
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CreateSecret'
      responses:
        "200":
          description: success
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/CreateSecretOutput'
  /get-secret-value:
    post:
      operationId: getSecretValue
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/GetSecretValue'
      responses:
        "200":
          description: success
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/GetSecretValueOutput'
  /update-secret-val:
    post:
      operationId: updateSecretVal
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/UpdateSecretVal'
      responses:
        "200":
          description: success
  /delete-item:
    post:
      operationId: deleteItem
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/DeleteItem'
      responses:
        "200":
          description: success
components:
  schemas:
    CreateSecret:
      type: object
      required:
        - name
        - value
      properties:
        name:
          type: string
          description: Secret name
        value:
          type: string
          description: Secret value
        tags:
          type: array
          items:
            type: string
        metadata:
          type: string
          description: Deprecated
        token:
          type: string
        delete_protection:
          type: string
          description: "true/false"
    CreateSecretOutput:
      type: object
      properties:
        name:
          type: string
    GetSecretValue:
      type: object
      required:
        - names
      properties:
        names:
          type: array
          items:
            type: string
        token:
          type: string
    GetSecretValueOutput:
      type: object
      properties:
        name:
          type: string
        value:
          type: string
        type:
          type: string
    UpdateSecretVal:
      type: object
      required:
        - name
        - value
      properties:
        name:
          type: string
        value:
          type: string
        tags:
          type: array
          items:
            type: string
        token:
          type: string
    DeleteItem:
      type: object
      required:
        - name
      properties:
        name:
          type: string
        token:
          type: string
"#;

    #[test]
    fn parse_minimal_spec() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        assert_eq!(spec.endpoints().len(), 4);
    }

    #[test]
    fn resolve_schema_fields() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let fields = spec.fields("CreateSecret").expect("fields");
        assert!(fields.len() >= 4);

        let name_field = fields
            .iter()
            .find(|f| f.name == "name")
            .expect("name field");
        assert!(name_field.required);
        assert_eq!(name_field.type_info, TypeInfo::String);

        let tags_field = fields
            .iter()
            .find(|f| f.name == "tags")
            .expect("tags field");
        assert!(!tags_field.required);
        assert_eq!(
            tags_field.type_info,
            TypeInfo::Array(Box::new(TypeInfo::String))
        );
    }

    #[test]
    fn diff_schemas() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let diff = spec
            .diff_schemas("CreateSecret", "UpdateSecretVal")
            .expect("diff");
        // CreateSecret has metadata, delete_protection that UpdateSecretVal doesn't
        assert!(!diff.removed.is_empty() || !diff.added.is_empty() || !diff.changed.is_empty());
    }

    #[test]
    fn crud_grouping() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let groups = spec.group_by_crud_pattern();
        assert!(!groups.is_empty());
    }

    #[test]
    fn endpoint_by_path() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let ep = spec.endpoint_by_path("/create-secret").expect("found");
        assert_eq!(ep.operation_id.as_deref(), Some("createSecret"));
        assert_eq!(ep.request_schema_ref.as_deref(), Some("CreateSecret"));
    }

    #[test]
    fn reject_openapi_2() {
        let spec_str =
            r#"{"swagger": "2.0", "info": {"title": "test", "version": "1"}, "paths": {}}"#;
        // This will either fail parsing (no "openapi" field) or version check
        let result = Spec::from_str(spec_str);
        assert!(result.is_err());
    }
}
