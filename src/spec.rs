use std::path::Path;
use std::str::FromStr;

use indexmap::IndexMap;

use crate::error::ForgeError;
use crate::types::{ref_name_from_path, OpenApiSpec, Operation, SchemaObject, SchemaOrRef, TypeInfo};

/// The CRUD verb detected from an RPC-style operation path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RpcCrudVerb {
    /// Resource creation (e.g. `create-*`, `add-*`).
    Create,
    /// Resource retrieval (e.g. `get-*`, `describe-*`).
    Read,
    /// Resource mutation (e.g. `update-*`).
    Update,
    /// Resource removal (e.g. `delete-*`, `remove-*`).
    Delete,
    /// Collection listing (e.g. `list-*`).
    List,
}

impl std::fmt::Display for RpcCrudVerb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Create => f.write_str("create"),
            Self::Read => f.write_str("read"),
            Self::Update => f.write_str("update"),
            Self::Delete => f.write_str("delete"),
            Self::List => f.write_str("list"),
        }
    }
}

impl FromStr for RpcCrudVerb {
    type Err = ForgeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "create" => Ok(Self::Create),
            "read" | "get" | "describe" => Ok(Self::Read),
            "update" => Ok(Self::Update),
            "delete" | "remove" => Ok(Self::Delete),
            "list" => Ok(Self::List),
            _ => Err(ForgeError::InvalidInput(format!(
                "unknown CRUD verb: {s}"
            ))),
        }
    }
}

/// A pattern that matches RPC-style operation paths to CRUD verbs and resource names.
///
/// Each pattern has a verb, a regex-like prefix, and an optional suffix that together
/// extract the resource base name from a path like `/auth-method-create-aws-iam`.
#[derive(Debug, Clone)]
pub struct RpcPattern {
    pub verb: RpcCrudVerb,
    /// The path prefix to match (e.g., `/auth-method-create-`). Use `{resource}` as
    /// placeholder for the variable part.
    pub template: String,
    /// The resulting group base name template. Use `{0}` for the captured variable part.
    pub group_name: String,
}

impl RpcPattern {
    /// Create a new RPC pattern.
    #[must_use]
    pub fn new(verb: RpcCrudVerb, template: &str, group_name: &str) -> Self {
        Self {
            verb,
            template: template.to_string(),
            group_name: group_name.to_string(),
        }
    }

    /// Try to match a path against this pattern. Returns the group base name if matched.
    fn try_match(&self, path: &str) -> Option<String> {
        let path_lower = path.to_lowercase();
        let template_lower = self.template.to_lowercase();

        if let Some(idx) = template_lower.find("{resource}") {
            let prefix = &template_lower[..idx];
            let suffix = &template_lower[idx + "{resource}".len()..];

            if !path_lower.starts_with(prefix) {
                return None;
            }

            let rest = &path_lower[prefix.len()..];

            let captured = if suffix.is_empty() {
                rest.to_string()
            } else if let Some(end_idx) = rest.find(suffix) {
                rest[..end_idx].to_string()
            } else {
                return None;
            };

            if captured.is_empty() {
                return None;
            }

            let group = self.group_name.replace("{0}", &captured);
            Some(group.replace('-', "_"))
        } else {
            // Exact match (no resource variable)
            if path_lower == template_lower {
                Some(self.group_name.replace('-', "_"))
            } else {
                None
            }
        }
    }
}

/// Configurable grouper for RPC-style CRUD APIs.
///
/// Unlike REST APIs where the HTTP method determines the CRUD verb,
/// RPC APIs encode the verb in the path or operation ID. This grouper
/// uses configurable patterns to detect verbs and group endpoints
/// by resource name.
///
/// # Example
///
/// ```
/// use openapi_forge::{RpcCrudGrouper, RpcCrudVerb, RpcPattern};
///
/// let grouper = RpcCrudGrouper::new()
///     .pattern(RpcPattern::new(
///         RpcCrudVerb::Create,
///         "/create-{resource}",
///         "{0}",
///     ))
///     .pattern(RpcPattern::new(
///         RpcCrudVerb::Delete,
///         "/delete-{resource}",
///         "{0}",
///     ));
/// ```
#[derive(Debug, Clone)]
pub struct RpcCrudGrouper {
    patterns: Vec<RpcPattern>,
}

impl Default for RpcCrudGrouper {
    fn default() -> Self {
        Self::new()
    }
}

impl RpcCrudGrouper {
    /// Create a new empty grouper with no patterns.
    #[must_use]
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
        }
    }

    /// Add a pattern to this grouper. Patterns are tried in order; first match wins.
    #[must_use]
    pub fn pattern(mut self, pat: RpcPattern) -> Self {
        self.patterns.push(pat);
        self
    }

    /// Add multiple patterns at once.
    #[must_use]
    pub fn patterns(mut self, pats: Vec<RpcPattern>) -> Self {
        self.patterns.extend(pats);
        self
    }

    /// Create a grouper with default patterns for common RPC APIs.
    ///
    /// Handles `create-X`, `get-X`, `update-X`, `delete-X`, `list-X`,
    /// `describe-X`, `add-X`, `remove-X` patterns.
    #[must_use]
    pub fn default_patterns() -> Self {
        Self::new().patterns(vec![
            RpcPattern::new(RpcCrudVerb::Create, "/create-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Create, "/add-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Read, "/get-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Read, "/describe-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Update, "/update-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Delete, "/delete-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Delete, "/remove-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::List, "/list-{resource}", "{0}"),
        ])
    }

    /// Create a grouper with Akeyless-specific patterns.
    ///
    /// Handles all the Akeyless API naming conventions:
    /// - Auth methods: `auth-method-create-X`, `create-auth-method-X`
    /// - Targets: `target-create-X`, `create-X-target`
    /// - Dynamic secrets: `dynamic-secret-create-X`, `create-dynamic-secret`
    /// - Rotated secrets: `rotated-secret-create-X`, `create-rotated-secret`
    /// - Gateway producers: `gateway-create-producer-X`, `gateway-update-producer-X`
    /// - Static secrets: `create-secret`, `get-secret-value`, etc.
    /// - Items: `describe-item`, `delete-item`, `list-items`
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn akeyless_patterns() -> Self {
        Self::new().patterns(vec![
            // --- Auth methods (Pattern A: auth-method-{verb}-{variant}) ---
            RpcPattern::new(
                RpcCrudVerb::Create,
                "/auth-method-create-{resource}",
                "auth_method_{0}",
            ),
            RpcPattern::new(
                RpcCrudVerb::Update,
                "/auth-method-update-{resource}",
                "auth_method_{0}",
            ),
            // --- Auth methods (Pattern B: {verb}-auth-method-{variant}) ---
            RpcPattern::new(
                RpcCrudVerb::Create,
                "/create-auth-method-{resource}",
                "auth_method_{0}",
            ),
            RpcPattern::new(
                RpcCrudVerb::Update,
                "/update-auth-method-{resource}",
                "auth_method_{0}",
            ),
            // Auth method shared read/delete (no variant)
            RpcPattern::new(RpcCrudVerb::Read, "/get-auth-method", "auth_method"),
            RpcPattern::new(RpcCrudVerb::Delete, "/delete-auth-method", "auth_method"),
            RpcPattern::new(RpcCrudVerb::List, "/list-auth-methods", "auth_method"),
            // --- Targets (Pattern A: target-{verb}-{variant}) ---
            RpcPattern::new(
                RpcCrudVerb::Create,
                "/target-create-{resource}",
                "target_{0}",
            ),
            RpcPattern::new(
                RpcCrudVerb::Update,
                "/target-update-{resource}",
                "target_{0}",
            ),
            // --- Targets (Pattern B: {verb}-{variant}-target) ---
            RpcPattern::new(
                RpcCrudVerb::Create,
                "/create-{resource}-target",
                "target_{0}",
            ),
            RpcPattern::new(
                RpcCrudVerb::Update,
                "/update-{resource}-target",
                "target_{0}",
            ),
            // Target shared read/delete
            RpcPattern::new(RpcCrudVerb::Read, "/target-get", "target"),
            RpcPattern::new(RpcCrudVerb::Delete, "/target-delete", "target"),
            RpcPattern::new(RpcCrudVerb::List, "/list-targets", "target"),
            // --- Dynamic secrets (Pattern A: dynamic-secret-{verb}-{variant}) ---
            RpcPattern::new(
                RpcCrudVerb::Create,
                "/dynamic-secret-create-{resource}",
                "dynamic_secret_{0}",
            ),
            RpcPattern::new(
                RpcCrudVerb::Update,
                "/dynamic-secret-update-{resource}",
                "dynamic_secret_{0}",
            ),
            // --- Dynamic secrets (Pattern B: {verb}-dynamic-secret-{variant}) ---
            RpcPattern::new(
                RpcCrudVerb::Create,
                "/create-dynamic-secret-{resource}",
                "dynamic_secret_{0}",
            ),
            RpcPattern::new(
                RpcCrudVerb::Update,
                "/update-dynamic-secret-{resource}",
                "dynamic_secret_{0}",
            ),
            // Dynamic secret shared read/delete
            RpcPattern::new(RpcCrudVerb::Read, "/dynamic-secret-get", "dynamic_secret"),
            RpcPattern::new(
                RpcCrudVerb::Delete,
                "/dynamic-secret-delete",
                "dynamic_secret",
            ),
            RpcPattern::new(RpcCrudVerb::List, "/list-dynamic-secrets", "dynamic_secret"),
            // --- Rotated secrets (Pattern A: rotated-secret-{verb}-{variant}) ---
            RpcPattern::new(
                RpcCrudVerb::Create,
                "/rotated-secret-create-{resource}",
                "rotated_secret_{0}",
            ),
            RpcPattern::new(
                RpcCrudVerb::Update,
                "/rotated-secret-update-{resource}",
                "rotated_secret_{0}",
            ),
            // --- Rotated secrets (Pattern B: {verb}-rotated-secret-{variant}) ---
            RpcPattern::new(
                RpcCrudVerb::Create,
                "/create-rotated-secret-{resource}",
                "rotated_secret_{0}",
            ),
            RpcPattern::new(
                RpcCrudVerb::Update,
                "/update-rotated-secret-{resource}",
                "rotated_secret_{0}",
            ),
            // Rotated secret shared read/delete
            RpcPattern::new(RpcCrudVerb::Read, "/rotated-secret-get", "rotated_secret"),
            RpcPattern::new(
                RpcCrudVerb::Delete,
                "/rotated-secret-delete",
                "rotated_secret",
            ),
            RpcPattern::new(RpcCrudVerb::List, "/list-rotated-secrets", "rotated_secret"),
            // --- Gateway producers: gateway-{verb}-producer-{variant} ---
            RpcPattern::new(
                RpcCrudVerb::Create,
                "/gateway-create-producer-{resource}",
                "gateway_producer_{0}",
            ),
            RpcPattern::new(
                RpcCrudVerb::Update,
                "/gateway-update-producer-{resource}",
                "gateway_producer_{0}",
            ),
            RpcPattern::new(
                RpcCrudVerb::Delete,
                "/gateway-delete-producer-{resource}",
                "gateway_producer_{0}",
            ),
            // --- Static secrets ---
            RpcPattern::new(RpcCrudVerb::Create, "/create-secret", "static_secret"),
            RpcPattern::new(RpcCrudVerb::Update, "/update-secret-val", "static_secret"),
            RpcPattern::new(RpcCrudVerb::Read, "/get-secret-value", "static_secret"),
            // --- Items (generic) ---
            RpcPattern::new(RpcCrudVerb::Read, "/describe-item", "item"),
            RpcPattern::new(RpcCrudVerb::Delete, "/delete-item", "item"),
            RpcPattern::new(RpcCrudVerb::List, "/list-items", "item"),
            // --- Fallback generic patterns (must come last) ---
            RpcPattern::new(RpcCrudVerb::Create, "/create-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Create, "/add-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Read, "/get-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Read, "/describe-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Update, "/update-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Delete, "/delete-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Delete, "/remove-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::List, "/list-{resource}", "{0}"),
        ])
    }

    /// Group the given endpoints into CRUD groups using the configured patterns.
    ///
    /// Each endpoint's path is tested against the patterns in order. The first
    /// matching pattern determines the verb and group name.
    #[must_use]
    pub fn group(&self, endpoints: &[Endpoint]) -> Vec<CrudGroup> {
        let mut groups: IndexMap<String, CrudGroup> = IndexMap::new();

        for ep in endpoints {
            if let Some((verb, group_name)) = self.match_endpoint(ep) {
                let group = groups
                    .entry(group_name.clone())
                    .or_insert_with(|| CrudGroup {
                        base_name: group_name,
                        ..CrudGroup::default()
                    });

                match verb {
                    RpcCrudVerb::Create => group.create = Some(ep.clone()),
                    RpcCrudVerb::Read => group.read = Some(ep.clone()),
                    RpcCrudVerb::Update => group.update = Some(ep.clone()),
                    RpcCrudVerb::Delete => group.delete = Some(ep.clone()),
                    RpcCrudVerb::List => group.list = Some(ep.clone()),
                }
            }
        }

        groups.into_values().collect()
    }

    /// Group endpoints directly from a Spec.
    #[must_use]
    pub fn group_spec(&self, spec: &Spec) -> Vec<CrudGroup> {
        self.group(&spec.endpoints())
    }

    /// Try to match an endpoint against the configured patterns.
    /// Returns the verb and group name if a pattern matches.
    fn match_endpoint(&self, ep: &Endpoint) -> Option<(RpcCrudVerb, String)> {
        for pat in &self.patterns {
            if let Some(group_name) = pat.try_match(&ep.path) {
                return Some((pat.verb, group_name));
            }
        }
        None
    }
}

/// High-level representation of a parsed and queryable `OpenAPI` spec.
#[derive(Debug, Clone)]
pub struct Spec {
    raw: OpenApiSpec,
}

/// A resolved endpoint with its request/response schema names.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// The URL path (e.g. `/create-secret`).
    pub path: String,
    /// The HTTP method in lowercase (e.g. `post`, `get`).
    pub method: String,
    /// The `operationId` declared in the spec, if any.
    pub operation_id: Option<String>,
    /// A human-readable summary from the spec.
    pub summary: Option<String>,
    /// Tags associated with this operation.
    pub tags: Vec<String>,
    /// The schema name extracted from the request body `$ref`, if present.
    pub request_schema_ref: Option<String>,
    /// The schema name extracted from the response `$ref`, if present.
    pub response_schema_ref: Option<String>,
}

/// A resolved field from a component schema.
#[derive(Debug, Clone)]
pub struct Field {
    /// Property name as declared in the schema.
    pub name: String,
    /// Resolved type (delegated to `takumi`).
    pub type_info: TypeInfo,
    /// Whether the field is listed in the schema's `required` array.
    pub required: bool,
    /// The `description` annotation, if any.
    pub description: Option<String>,
    /// The `default` value, if any.
    pub default: Option<serde_json::Value>,
    /// The `format` annotation (e.g. `date-time`, `int32`).
    pub format: Option<String>,
    /// Allowed values when the field declares an `enum` constraint.
    pub enum_values: Option<Vec<String>>,
}

/// Result of diffing two schemas by field name.
#[derive(Debug, Clone)]
pub struct SchemaDiff {
    /// Field names present in schema B but absent from schema A.
    pub added: Vec<String>,
    /// Field names present in schema A but absent from schema B.
    pub removed: Vec<String>,
    /// Fields present in both but with different type or required status.
    pub changed: Vec<FieldChange>,
}

/// A field whose type or required status changed between two schema versions.
#[derive(Debug, Clone)]
pub struct FieldChange {
    /// The field name.
    pub name: String,
    /// The type in the first (old) schema.
    pub old_type: TypeInfo,
    /// The type in the second (new) schema.
    pub new_type: TypeInfo,
    /// Whether the `required` status differs between the two schemas.
    pub required_changed: bool,
}

/// A group of related CRUD endpoints sharing a common resource name.
#[derive(Debug, Clone, Default)]
pub struct CrudGroup {
    /// The normalised resource name that ties these endpoints together.
    pub base_name: String,
    /// The endpoint that creates this resource, if detected.
    pub create: Option<Endpoint>,
    /// The endpoint that reads / retrieves this resource, if detected.
    pub read: Option<Endpoint>,
    /// The endpoint that updates this resource, if detected.
    pub update: Option<Endpoint>,
    /// The endpoint that deletes this resource, if detected.
    pub delete: Option<Endpoint>,
    /// The endpoint that lists instances of this resource, if detected.
    pub list: Option<Endpoint>,
}

impl Spec {
    /// Load an `OpenAPI` spec from a YAML or JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, parsed, or is not `OpenAPI` 3.0.x.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ForgeError> {
        let content = std::fs::read_to_string(path.as_ref())?;
        Self::parse(&content)
    }

    /// Parse an `OpenAPI` spec from a string (YAML or JSON).
    ///
    /// Convenience wrapper around the [`FromStr`] implementation.
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed.
    pub fn parse(content: &str) -> Result<Self, ForgeError> {
        let raw: OpenApiSpec = if content.trim_start().starts_with('{') {
            serde_json::from_str(content)?
        } else {
            serde_yaml_ng::from_str(content)?
        };

        Ok(Self { raw })
    }

    /// Get all endpoints in the spec.
    #[must_use]
    pub fn endpoints(&self) -> Vec<Endpoint> {
        self.raw
            .paths
            .iter()
            .flat_map(|(path, item)| {
                let methods: [(&str, &Option<Operation>); 5] = [
                    ("get", &item.get),
                    ("post", &item.post),
                    ("put", &item.put),
                    ("delete", &item.delete),
                    ("patch", &item.patch),
                ];
                methods.into_iter().filter_map(move |(method, op)| {
                    op.as_ref().map(|operation| Endpoint {
                        path: path.clone(),
                        method: method.to_string(),
                        operation_id: operation.operation_id.clone(),
                        summary: operation.summary.clone(),
                        tags: operation.tags.clone(),
                        request_schema_ref: Self::extract_request_ref(operation),
                        response_schema_ref: Self::extract_response_ref(operation),
                    })
                })
            })
            .collect()
    }

    /// Look up a component schema by name.
    ///
    /// # Errors
    ///
    /// Returns `SchemaNotFound` if the schema name does not exist.
    pub fn schema(&self, name: &str) -> Result<&SchemaObject, ForgeError> {
        self.raw
            .components
            .as_ref()
            .and_then(|c| c.schemas.get(name))
            .ok_or_else(|| ForgeError::SchemaNotFound(name.to_string()))
    }

    /// Get all component schema names.
    #[must_use]
    pub fn schema_names(&self) -> Vec<&str> {
        self.raw
            .components
            .as_ref()
            .map(|c| c.schemas.keys().map(String::as_str).collect())
            .unwrap_or_default()
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

    /// Resolve the type info for a sekkei `Schema`.
    ///
    /// Delegates to `takumi::schema_to_field_type` for consistent type resolution.
    #[must_use]
    pub fn resolve_type(&self, schema: &SchemaObject) -> TypeInfo {
        takumi::schema_to_field_type(schema)
    }

    /// Resolve the type info for a `SchemaOrRef` adapter.
    #[must_use]
    pub fn resolve_schema_or_ref_type(&self, schema_or_ref: &SchemaOrRef) -> TypeInfo {
        match schema_or_ref {
            SchemaOrRef::Ref { ref_path } => {
                let name = ref_name_from_path(ref_path).unwrap_or("Unknown");
                TypeInfo::Object(name.to_string())
            }
            SchemaOrRef::Schema(schema) => takumi::schema_to_field_type(schema),
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

        let added: Vec<String> = map_b
            .keys()
            .filter(|name| !map_a.contains_key(*name))
            .map(|name| (*name).to_string())
            .collect();

        let removed: Vec<String> = map_a
            .keys()
            .filter(|name| !map_b.contains_key(*name))
            .map(|name| (*name).to_string())
            .collect();

        let changed: Vec<FieldChange> = map_b
            .iter()
            .filter_map(|(name, field_b)| {
                let field_a = map_a.get(name)?;
                if field_a.type_info != field_b.type_info || field_a.required != field_b.required {
                    Some(FieldChange {
                        name: (*name).to_string(),
                        old_type: field_a.type_info.clone(),
                        new_type: field_b.type_info.clone(),
                        required_changed: field_a.required != field_b.required,
                    })
                } else {
                    None
                }
            })
            .collect();

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
            let (verb, base) = Self::detect_crud_verb(op_id);
            if base.is_empty() {
                continue;
            }

            let group = groups.entry(base.clone()).or_insert_with(|| CrudGroup {
                base_name: base,
                ..CrudGroup::default()
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
        self.resolve_fields_recursive(schema, &mut Vec::new())
    }

    fn resolve_fields_recursive(
        &self,
        schema: &SchemaObject,
        visited: &mut Vec<String>,
    ) -> Vec<Field> {
        let mut fields = Vec::new();

        // Handle allOf by merging properties from all referenced schemas.
        // sekkei represents allOf as Vec<Schema> where each item may have a ref_path.
        for item in &schema.all_of {
            if let Some(ref_path) = &item.ref_path {
                // This is a $ref inside allOf
                if let Some(name) = ref_name_from_path(ref_path) {
                    // Prevent infinite recursion on circular refs
                    if !visited.contains(&name.to_string()) {
                        visited.push(name.to_string());
                        if let Ok(referenced) = self.schema(name) {
                            fields.extend(self.resolve_fields_recursive(referenced, visited));
                        }
                    }
                }
            } else {
                // Inline schema inside allOf
                fields.extend(self.resolve_fields_recursive(item, visited));
            }
        }

        // sekkei properties are BTreeMap<String, Schema> (flat, not SchemaOrRef enum)
        for (name, prop) in &schema.properties {
            let required = schema.required.contains(name);
            let type_info = takumi::schema_to_field_type(prop);

            let description = prop.description.clone();
            let default = prop.default.clone();
            let format = prop.format.clone();
            let enum_values = prop.enum_values.as_ref().map(|vals| {
                vals.iter()
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect()
            });

            // Avoid duplicates from allOf merging -- last definition wins
            fields.retain(|f: &Field| f.name != *name);

            fields.push(Field {
                name: name.clone(),
                type_info,
                required,
                description,
                default,
                format,
                enum_values,
            });
        }

        fields
    }

    fn extract_request_ref(op: &Operation) -> Option<String> {
        let body = op.request_body.as_ref()?;
        let media = body.content.get("application/json")?;
        let schema = media.schema.as_ref()?;
        schema
            .ref_path
            .as_ref()
            .and_then(|r| ref_name_from_path(r))
            .map(String::from)
    }

    fn extract_response_ref(op: &Operation) -> Option<String> {
        let response = op
            .responses
            .get("200")
            .or_else(|| op.responses.get("201"))
            .or_else(|| op.responses.get("default"))?;
        let content = response.content.as_ref()?;
        let media = content.get("application/json")?;
        let schema = media.schema.as_ref()?;
        schema
            .ref_path
            .as_ref()
            .and_then(|r| ref_name_from_path(r))
            .map(String::from)
    }
}

impl FromStr for Spec {
    type Err = ForgeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[derive(Clone, Copy)]
enum CrudVerb {
    Create,
    Read,
    Update,
    Delete,
    List,
    None,
}

impl Spec {
    fn detect_crud_verb(operation_id: &str) -> (CrudVerb, String) {
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

        for &(prefix, verb) in prefixes {
            if let Some(base) = normalized.strip_prefix(prefix) {
                let original_base = Self::strip_verb_prefix(operation_id);
                let name = if original_base.is_empty() {
                    base.to_string()
                } else {
                    original_base
                };
                return (verb, name);
            }
        }

        (CrudVerb::None, String::new())
    }

    fn strip_verb_prefix(operation_id: &str) -> String {
        const VERB_PREFIXES: &[&str] = &[
            "create-",
            "add-",
            "get-",
            "describe-",
            "update-",
            "delete-",
            "remove-",
            "list-",
        ];
        for verb in VERB_PREFIXES {
            if let Some(rest) = operation_id.to_lowercase().strip_prefix(verb) {
                return rest.to_string();
            }
        }
        String::new()
    }
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
    fn reject_invalid_content() {
        let spec_str = "this is definitely not valid yaml or json {{{{";
        let result = Spec::from_str(spec_str);
        assert!(result.is_err());
    }

    // --- Enum support tests ---

    const ENUM_SPEC: &str = r#"
openapi: "3.0.0"
info:
  title: Enum Test API
  version: "1.0"
paths: {}
components:
  schemas:
    AccessPermission:
      type: object
      required:
        - permission
      properties:
        permission:
          type: string
          enum:
            - read
            - write
            - admin
          description: The permission level
        status:
          type: string
          enum:
            - active
            - inactive
            - pending
"#;

    #[test]
    fn enum_values_populated() {
        let spec = Spec::from_str(ENUM_SPEC).expect("parse");
        let fields = spec.fields("AccessPermission").expect("fields");

        let perm = fields
            .iter()
            .find(|f| f.name == "permission")
            .expect("permission field");
        assert_eq!(
            perm.enum_values,
            Some(vec![
                "read".to_string(),
                "write".to_string(),
                "admin".to_string()
            ])
        );

        let status = fields
            .iter()
            .find(|f| f.name == "status")
            .expect("status field");
        assert_eq!(
            status.enum_values,
            Some(vec![
                "active".to_string(),
                "inactive".to_string(),
                "pending".to_string()
            ])
        );
    }

    #[test]
    fn enum_values_none_when_absent() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let fields = spec.fields("CreateSecret").expect("fields");
        let name_field = fields
            .iter()
            .find(|f| f.name == "name")
            .expect("name field");
        assert!(name_field.enum_values.is_none());
    }

    // --- allOf composition tests ---

    const ALLOF_SPEC: &str = r#"
openapi: "3.0.0"
info:
  title: AllOf Test API
  version: "1.0"
paths: {}
components:
  schemas:
    BaseResource:
      type: object
      required:
        - id
      properties:
        id:
          type: string
          description: Resource ID
        created_at:
          type: string
          format: date-time
    AuditFields:
      type: object
      properties:
        updated_by:
          type: string
        audit_trail:
          type: array
          items:
            type: string
    NamedResource:
      type: object
      allOf:
        - $ref: '#/components/schemas/BaseResource'
      required:
        - name
      properties:
        name:
          type: string
          description: Resource name
    FullResource:
      type: object
      allOf:
        - $ref: '#/components/schemas/NamedResource'
        - $ref: '#/components/schemas/AuditFields'
      required:
        - status
      properties:
        status:
          type: string
          enum:
            - active
            - archived
"#;

    #[test]
    fn allof_single_ref_merges_fields() {
        let spec = Spec::from_str(ALLOF_SPEC).expect("parse");
        let fields = spec.fields("NamedResource").expect("fields");
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"id"), "should inherit id from BaseResource");
        assert!(
            names.contains(&"created_at"),
            "should inherit created_at from BaseResource"
        );
        assert!(names.contains(&"name"), "should have own property name");
    }

    #[test]
    fn allof_nested_chain_merges_all() {
        let spec = Spec::from_str(ALLOF_SPEC).expect("parse");
        let fields = spec.fields("FullResource").expect("fields");
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();

        // From BaseResource (via NamedResource chain)
        assert!(names.contains(&"id"), "should have id from BaseResource");
        assert!(
            names.contains(&"created_at"),
            "should have created_at from BaseResource"
        );
        // From NamedResource
        assert!(
            names.contains(&"name"),
            "should have name from NamedResource"
        );
        // From AuditFields
        assert!(
            names.contains(&"updated_by"),
            "should have updated_by from AuditFields"
        );
        assert!(
            names.contains(&"audit_trail"),
            "should have audit_trail from AuditFields"
        );
        // Own property
        assert!(names.contains(&"status"), "should have own status property");
    }

    #[test]
    fn allof_nested_with_enum() {
        let spec = Spec::from_str(ALLOF_SPEC).expect("parse");
        let fields = spec.fields("FullResource").expect("fields");
        let status = fields
            .iter()
            .find(|f| f.name == "status")
            .expect("status field");
        assert_eq!(
            status.enum_values,
            Some(vec!["active".to_string(), "archived".to_string()])
        );
    }

    // --- RpcCrudGrouper tests ---

    const AKEYLESS_SPEC: &str = r#"
openapi: "3.0.0"
info:
  title: Akeyless API
  version: "2.0"
paths:
  /auth-method-create-aws-iam:
    post:
      operationId: authMethodCreateAwsIam
      responses:
        "200":
          description: success
  /auth-method-update-aws-iam:
    post:
      operationId: authMethodUpdateAwsIam
      responses:
        "200":
          description: success
  /create-auth-method-azure-ad:
    post:
      operationId: createAuthMethodAzureAd
      responses:
        "200":
          description: success
  /update-auth-method-azure-ad:
    post:
      operationId: updateAuthMethodAzureAd
      responses:
        "200":
          description: success
  /get-auth-method:
    post:
      operationId: getAuthMethod
      responses:
        "200":
          description: success
  /delete-auth-method:
    post:
      operationId: deleteAuthMethod
      responses:
        "200":
          description: success
  /target-create-aws:
    post:
      operationId: targetCreateAws
      responses:
        "200":
          description: success
  /target-update-aws:
    post:
      operationId: targetUpdateAws
      responses:
        "200":
          description: success
  /target-get:
    post:
      operationId: targetGet
      responses:
        "200":
          description: success
  /target-delete:
    post:
      operationId: targetDelete
      responses:
        "200":
          description: success
  /dynamic-secret-create-aws:
    post:
      operationId: dynamicSecretCreateAws
      responses:
        "200":
          description: success
  /dynamic-secret-update-aws:
    post:
      operationId: dynamicSecretUpdateAws
      responses:
        "200":
          description: success
  /dynamic-secret-get:
    post:
      operationId: dynamicSecretGet
      responses:
        "200":
          description: success
  /dynamic-secret-delete:
    post:
      operationId: dynamicSecretDelete
      responses:
        "200":
          description: success
  /create-secret:
    post:
      operationId: createSecret
      responses:
        "200":
          description: success
  /update-secret-val:
    post:
      operationId: updateSecretVal
      responses:
        "200":
          description: success
  /get-secret-value:
    post:
      operationId: getSecretValue
      responses:
        "200":
          description: success
  /describe-item:
    post:
      operationId: describeItem
      responses:
        "200":
          description: success
  /delete-item:
    post:
      operationId: deleteItem
      responses:
        "200":
          description: success
  /gateway-create-producer-aws:
    post:
      operationId: gatewayCreateProducerAws
      responses:
        "200":
          description: success
  /gateway-update-producer-aws:
    post:
      operationId: gatewayUpdateProducerAws
      responses:
        "200":
          description: success
  /rotated-secret-create-mysql:
    post:
      operationId: rotatedSecretCreateMysql
      responses:
        "200":
          description: success
  /rotated-secret-update-mysql:
    post:
      operationId: rotatedSecretUpdateMysql
      responses:
        "200":
          description: success
  /rotated-secret-get:
    post:
      operationId: rotatedSecretGet
      responses:
        "200":
          description: success
  /rotated-secret-delete:
    post:
      operationId: rotatedSecretDelete
      responses:
        "200":
          description: success
components:
  schemas: {}
"#;

    #[test]
    fn rpc_grouper_auth_method_pattern_a() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let aws_iam = groups
            .iter()
            .find(|g| g.base_name == "auth_method_aws_iam")
            .expect("auth_method_aws_iam group");
        assert!(aws_iam.create.is_some(), "should have create");
        assert_eq!(
            aws_iam.create.as_ref().unwrap().path,
            "/auth-method-create-aws-iam"
        );
        assert!(aws_iam.update.is_some(), "should have update");
        assert_eq!(
            aws_iam.update.as_ref().unwrap().path,
            "/auth-method-update-aws-iam"
        );
    }

    #[test]
    fn rpc_grouper_auth_method_pattern_b() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let azure = groups
            .iter()
            .find(|g| g.base_name == "auth_method_azure_ad")
            .expect("auth_method_azure_ad group");
        assert!(azure.create.is_some(), "should have create");
        assert_eq!(
            azure.create.as_ref().unwrap().path,
            "/create-auth-method-azure-ad"
        );
        assert!(azure.update.is_some(), "should have update");
    }

    #[test]
    fn rpc_grouper_auth_method_shared_read_delete() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let auth = groups
            .iter()
            .find(|g| g.base_name == "auth_method")
            .expect("auth_method group");
        assert!(auth.read.is_some(), "should have read");
        assert_eq!(auth.read.as_ref().unwrap().path, "/get-auth-method");
        assert!(auth.delete.is_some(), "should have delete");
        assert_eq!(auth.delete.as_ref().unwrap().path, "/delete-auth-method");
    }

    #[test]
    fn rpc_grouper_targets() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let target_aws = groups
            .iter()
            .find(|g| g.base_name == "target_aws")
            .expect("target_aws group");
        assert!(target_aws.create.is_some());
        assert_eq!(
            target_aws.create.as_ref().unwrap().path,
            "/target-create-aws"
        );
        assert!(target_aws.update.is_some());
        assert_eq!(
            target_aws.update.as_ref().unwrap().path,
            "/target-update-aws"
        );

        let target = groups
            .iter()
            .find(|g| g.base_name == "target")
            .expect("target group");
        assert!(target.read.is_some());
        assert_eq!(target.read.as_ref().unwrap().path, "/target-get");
        assert!(target.delete.is_some());
        assert_eq!(target.delete.as_ref().unwrap().path, "/target-delete");
    }

    #[test]
    fn rpc_grouper_dynamic_secrets() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let ds_aws = groups
            .iter()
            .find(|g| g.base_name == "dynamic_secret_aws")
            .expect("dynamic_secret_aws group");
        assert!(ds_aws.create.is_some());
        assert_eq!(
            ds_aws.create.as_ref().unwrap().path,
            "/dynamic-secret-create-aws"
        );
        assert!(ds_aws.update.is_some());

        let ds = groups
            .iter()
            .find(|g| g.base_name == "dynamic_secret")
            .expect("dynamic_secret group");
        assert!(ds.read.is_some());
        assert_eq!(ds.read.as_ref().unwrap().path, "/dynamic-secret-get");
        assert!(ds.delete.is_some());
    }

    #[test]
    fn rpc_grouper_static_secrets() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let ss = groups
            .iter()
            .find(|g| g.base_name == "static_secret")
            .expect("static_secret group");
        assert!(ss.create.is_some());
        assert_eq!(ss.create.as_ref().unwrap().path, "/create-secret");
        assert!(ss.update.is_some());
        assert_eq!(ss.update.as_ref().unwrap().path, "/update-secret-val");
        assert!(ss.read.is_some());
        assert_eq!(ss.read.as_ref().unwrap().path, "/get-secret-value");
    }

    #[test]
    fn rpc_grouper_items() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let item = groups
            .iter()
            .find(|g| g.base_name == "item")
            .expect("item group");
        assert!(item.read.is_some());
        assert_eq!(item.read.as_ref().unwrap().path, "/describe-item");
        assert!(item.delete.is_some());
        assert_eq!(item.delete.as_ref().unwrap().path, "/delete-item");
    }

    #[test]
    fn rpc_grouper_gateway_producers() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let gp_aws = groups
            .iter()
            .find(|g| g.base_name == "gateway_producer_aws")
            .expect("gateway_producer_aws group");
        assert!(gp_aws.create.is_some());
        assert_eq!(
            gp_aws.create.as_ref().unwrap().path,
            "/gateway-create-producer-aws"
        );
        assert!(gp_aws.update.is_some());
        assert_eq!(
            gp_aws.update.as_ref().unwrap().path,
            "/gateway-update-producer-aws"
        );
    }

    #[test]
    fn rpc_grouper_rotated_secrets() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let rs_mysql = groups
            .iter()
            .find(|g| g.base_name == "rotated_secret_mysql")
            .expect("rotated_secret_mysql group");
        assert!(rs_mysql.create.is_some());
        assert_eq!(
            rs_mysql.create.as_ref().unwrap().path,
            "/rotated-secret-create-mysql"
        );
        assert!(rs_mysql.update.is_some());

        let rs = groups
            .iter()
            .find(|g| g.base_name == "rotated_secret")
            .expect("rotated_secret group");
        assert!(rs.read.is_some());
        assert_eq!(rs.read.as_ref().unwrap().path, "/rotated-secret-get");
        assert!(rs.delete.is_some());
    }

    #[test]
    fn rpc_grouper_custom_patterns() {
        let spec_str = r#"
openapi: "3.0.0"
info:
  title: Custom API
  version: "1.0"
paths:
  /v1/resource/create:
    post:
      operationId: resourceCreate
      responses:
        "200":
          description: success
  /v1/resource/get:
    post:
      operationId: resourceGet
      responses:
        "200":
          description: success
components:
  schemas: {}
"#;
        let spec = Spec::from_str(spec_str).expect("parse");
        let grouper = RpcCrudGrouper::new()
            .pattern(RpcPattern::new(
                RpcCrudVerb::Create,
                "/v1/{resource}/create",
                "{0}",
            ))
            .pattern(RpcPattern::new(
                RpcCrudVerb::Read,
                "/v1/{resource}/get",
                "{0}",
            ));
        let groups = grouper.group_spec(&spec);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].base_name, "resource");
        assert!(groups[0].create.is_some());
        assert!(groups[0].read.is_some());
    }

    #[test]
    fn rpc_grouper_default_patterns() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::default_patterns();
        let groups = grouper.group_spec(&spec);
        // /create-secret -> "secret", /get-secret-value -> "secret_value",
        // /update-secret-val -> "secret_val", /delete-item -> "item"
        assert!(!groups.is_empty());
    }

    #[test]
    fn rpc_pattern_try_match_exact() {
        let pat = RpcPattern::new(RpcCrudVerb::Read, "/get-auth-method", "auth_method");
        assert_eq!(
            pat.try_match("/get-auth-method"),
            Some("auth_method".to_string())
        );
        assert_eq!(pat.try_match("/get-auth-methods"), None);
        assert_eq!(pat.try_match("/delete-auth-method"), None);
    }

    #[test]
    fn rpc_pattern_try_match_with_resource() {
        let pat = RpcPattern::new(RpcCrudVerb::Create, "/create-{resource}", "{0}");
        assert_eq!(pat.try_match("/create-secret"), Some("secret".to_string()));
        assert_eq!(
            pat.try_match("/create-auth-method"),
            Some("auth_method".to_string())
        );
    }

    #[test]
    fn rpc_pattern_try_match_prefix_and_suffix() {
        let pat = RpcPattern::new(
            RpcCrudVerb::Create,
            "/create-{resource}-target",
            "target_{0}",
        );
        assert_eq!(
            pat.try_match("/create-aws-target"),
            Some("target_aws".to_string())
        );
        assert_eq!(pat.try_match("/create-target"), None);
    }

    #[test]
    fn rpc_grouper_default_impl() {
        // Verify Default trait works (empty grouper)
        let grouper = RpcCrudGrouper::default();
        assert!(grouper.group(&[]).is_empty());
    }

    // --- Existing group_by_crud_pattern still works ---

    #[test]
    fn legacy_crud_grouping_unchanged() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let groups = spec.group_by_crud_pattern();
        assert!(!groups.is_empty());
        // Verify at least one group has a create operation
        let has_create = groups.iter().any(|g| g.create.is_some());
        assert!(has_create, "at least one group should have a create");
    }

    // --- TypeInfo (FieldType) tests for enum variant ---

    #[test]
    fn type_info_enum_variant_from_takumi() {
        // Verify that takumi::FieldType::Enum is available through our TypeInfo alias
        let enum_type = TypeInfo::Enum {
            values: vec!["a".to_string(), "b".to_string()],
            underlying: Box::new(TypeInfo::String),
        };
        assert_eq!(
            enum_type,
            TypeInfo::Enum {
                values: vec!["a".to_string(), "b".to_string()],
                underlying: Box::new(TypeInfo::String),
            }
        );
    }

    #[test]
    fn resolve_type_delegates_to_takumi() {
        let spec = Spec::from_str(ENUM_SPEC).expect("parse");
        let schema = spec.schema("AccessPermission").expect("schema");
        let perm_prop = &schema.properties["permission"];
        let type_info = spec.resolve_type(perm_prop);
        // takumi resolves string enums as FieldType::Enum
        assert_eq!(
            type_info,
            TypeInfo::Enum {
                values: vec![
                    "read".to_string(),
                    "write".to_string(),
                    "admin".to_string()
                ],
                underlying: Box::new(TypeInfo::String),
            }
        );
    }

    // ========================================================================
    // JSON parsing — ensures the `starts_with('{')` branch in `from_str` works
    // ========================================================================

    #[test]
    fn parse_json_spec() {
        let json = r#"{
            "openapi": "3.0.0",
            "info": { "title": "JSON API", "version": "1.0" },
            "paths": {
                "/ping": {
                    "get": {
                        "operationId": "ping",
                        "responses": { "200": { "description": "pong" } }
                    }
                }
            },
            "components": {
                "schemas": {
                    "Pong": {
                        "type": "object",
                        "properties": {
                            "msg": { "type": "string" }
                        }
                    }
                }
            }
        }"#;
        let spec = Spec::from_str(json).expect("should parse JSON");
        assert_eq!(spec.endpoints().len(), 1);
        assert_eq!(spec.endpoints()[0].method, "get");
        assert_eq!(spec.schema_names(), vec!["Pong"]);
    }

    #[test]
    fn parse_json_with_leading_whitespace() {
        let json = "   \n\t  {\"info\":{\"title\":\"Ws\",\"version\":\"1\"},\"paths\":{}}";
        let spec = Spec::from_str(json).expect("should handle leading whitespace before {");
        assert!(spec.endpoints().is_empty());
    }

    #[test]
    fn parse_json_invalid_returns_error() {
        let result = Spec::from_str("{\"not_a_valid_spec\": true}");
        assert!(result.is_err());
    }

    // ========================================================================
    // Spec::load — file-based loading via tempfile
    // ========================================================================

    #[test]
    fn load_yaml_file() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().expect("create temp");
        write!(f, "{MINIMAL_SPEC}").expect("write");
        let spec = Spec::load(f.path()).expect("load");
        assert_eq!(spec.endpoints().len(), 4);
    }

    #[test]
    fn load_json_file() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::with_suffix(".json").expect("create temp");
        let json = r#"{"openapi":"3.0.0","info":{"title":"F","version":"1"},"paths":{}}"#;
        write!(f, "{json}").expect("write");
        let spec = Spec::load(f.path()).expect("load json file");
        assert!(spec.endpoints().is_empty());
    }

    #[test]
    fn load_nonexistent_file_returns_io_error() {
        let result = Spec::load(std::path::Path::new("/tmp/__no_such_file_openapi__.yaml"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, ForgeError::Io(_)),
            "expected Io error, got: {err:?}"
        );
    }

    // ========================================================================
    // schema() and schema_names() — error paths and edge cases
    // ========================================================================

    #[test]
    fn schema_not_found_returns_error() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let err = spec.schema("NoSuchSchema").unwrap_err();
        assert!(
            matches!(err, ForgeError::SchemaNotFound(_)),
            "expected SchemaNotFound, got: {err:?}"
        );
        assert!(err.to_string().contains("NoSuchSchema"));
    }

    #[test]
    fn schema_names_returns_all_names() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let names = spec.schema_names();
        assert!(names.contains(&"CreateSecret"));
        assert!(names.contains(&"GetSecretValue"));
        assert!(names.contains(&"UpdateSecretVal"));
        assert!(names.contains(&"DeleteItem"));
        assert!(names.contains(&"CreateSecretOutput"));
        assert!(names.contains(&"GetSecretValueOutput"));
    }

    #[test]
    fn schema_names_empty_when_no_components() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: No Components
  version: "1.0"
paths: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        assert!(spec.schema_names().is_empty());
    }

    // ========================================================================
    // resolve_schema_or_ref_type — Ref and Schema branches
    // ========================================================================

    #[test]
    fn resolve_schema_or_ref_type_ref_variant() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let sor = SchemaOrRef::Ref {
            ref_path: "#/components/schemas/CreateSecret".to_string(),
        };
        let ti = spec.resolve_schema_or_ref_type(&sor);
        assert_eq!(ti, TypeInfo::Object("CreateSecret".to_string()));
    }

    #[test]
    fn resolve_schema_or_ref_type_ref_no_slash() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let sor = SchemaOrRef::Ref {
            ref_path: "Standalone".to_string(),
        };
        let ti = spec.resolve_schema_or_ref_type(&sor);
        assert_eq!(ti, TypeInfo::Object("Standalone".to_string()));
    }

    #[test]
    fn resolve_schema_or_ref_type_schema_variant() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let schema = SchemaObject {
            schema_type: Some("string".to_string()),
            ..SchemaObject::default()
        };
        let sor = SchemaOrRef::Schema(Box::new(schema));
        let ti = spec.resolve_schema_or_ref_type(&sor);
        assert_eq!(ti, TypeInfo::String);
    }

    // ========================================================================
    // diff_schemas — detailed assertions and error paths
    // ========================================================================

    #[test]
    fn diff_schemas_identical_schemas() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let diff = spec.diff_schemas("CreateSecret", "CreateSecret").expect("diff");
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.changed.is_empty());
    }

    #[test]
    fn diff_schemas_added_and_removed() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let diff = spec
            .diff_schemas("CreateSecret", "UpdateSecretVal")
            .expect("diff");
        assert!(
            diff.removed.contains(&"metadata".to_string())
                || diff.removed.contains(&"delete_protection".to_string()),
            "CreateSecret has fields not in UpdateSecretVal: {:?}",
            diff.removed
        );
    }

    #[test]
    fn diff_schemas_with_type_change() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Diff
  version: "1.0"
paths: {}
components:
  schemas:
    A:
      type: object
      required:
        - x
      properties:
        x:
          type: string
    B:
      type: object
      properties:
        x:
          type: integer
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let diff = spec.diff_schemas("A", "B").expect("diff");
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].name, "x");
        assert_eq!(diff.changed[0].old_type, TypeInfo::String);
        assert_eq!(diff.changed[0].new_type, TypeInfo::Integer);
        assert!(diff.changed[0].required_changed);
    }

    #[test]
    fn diff_schemas_first_not_found() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let err = spec.diff_schemas("Missing", "CreateSecret").unwrap_err();
        assert!(matches!(err, ForgeError::SchemaNotFound(_)));
    }

    #[test]
    fn diff_schemas_second_not_found() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let err = spec.diff_schemas("CreateSecret", "Missing").unwrap_err();
        assert!(matches!(err, ForgeError::SchemaNotFound(_)));
    }

    // ========================================================================
    // endpoint_by_path — not-found case
    // ========================================================================

    #[test]
    fn endpoint_by_path_not_found() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        assert!(spec.endpoint_by_path("/no-such-path").is_none());
    }

    // ========================================================================
    // Multiple HTTP methods in endpoints()
    // ========================================================================

    #[test]
    fn endpoints_multiple_methods() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Multi
  version: "1.0"
paths:
  /resource:
    get:
      operationId: getResource
      responses:
        "200":
          description: ok
    post:
      operationId: createResource
      responses:
        "200":
          description: ok
    put:
      operationId: updateResource
      responses:
        "200":
          description: ok
    delete:
      operationId: deleteResource
      responses:
        "200":
          description: ok
    patch:
      operationId: patchResource
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let eps = spec.endpoints();
        assert_eq!(eps.len(), 5);
        let methods: Vec<&str> = eps.iter().map(|e| e.method.as_str()).collect();
        assert!(methods.contains(&"get"));
        assert!(methods.contains(&"post"));
        assert!(methods.contains(&"put"));
        assert!(methods.contains(&"delete"));
        assert!(methods.contains(&"patch"));
    }

    // ========================================================================
    // Empty paths
    // ========================================================================

    #[test]
    fn endpoints_empty_paths() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Empty
  version: "1.0"
paths: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        assert!(spec.endpoints().is_empty());
    }

    // ========================================================================
    // Endpoint fields: summary, tags, response_schema_ref
    // ========================================================================

    #[test]
    fn endpoint_summary_and_tags() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Tags
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      summary: List all items
      tags:
        - items
        - public
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let ep = spec.endpoint_by_path("/items").expect("found");
        assert_eq!(ep.summary.as_deref(), Some("List all items"));
        assert_eq!(ep.tags, vec!["items", "public"]);
    }

    #[test]
    fn endpoint_response_schema_ref() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let ep = spec.endpoint_by_path("/create-secret").expect("found");
        assert_eq!(ep.response_schema_ref.as_deref(), Some("CreateSecretOutput"));
    }

    #[test]
    fn endpoint_no_request_body() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: NoBody
  version: "1.0"
paths:
  /health:
    get:
      operationId: healthCheck
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let ep = spec.endpoint_by_path("/health").expect("found");
        assert!(ep.request_schema_ref.is_none());
        assert!(ep.response_schema_ref.is_none());
    }

    // ========================================================================
    // extract_response_ref fallbacks: 201, default
    // ========================================================================

    #[test]
    fn response_ref_fallback_201() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Resp201
  version: "1.0"
paths:
  /items:
    post:
      operationId: createItem
      responses:
        "201":
          description: created
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/Item'
components:
  schemas:
    Item:
      type: object
      properties:
        id:
          type: string
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let ep = spec.endpoint_by_path("/items").expect("found");
        assert_eq!(ep.response_schema_ref.as_deref(), Some("Item"));
    }

    #[test]
    fn response_ref_fallback_default() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: RespDefault
  version: "1.0"
paths:
  /items:
    get:
      operationId: getItem
      responses:
        default:
          description: default resp
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/GenericResp'
components:
  schemas:
    GenericResp:
      type: object
      properties:
        message:
          type: string
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let ep = spec.endpoint_by_path("/items").expect("found");
        assert_eq!(ep.response_schema_ref.as_deref(), Some("GenericResp"));
    }

    // ========================================================================
    // fields() — error path, metadata (description, default, format)
    // ========================================================================

    #[test]
    fn fields_missing_schema_returns_error() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let err = spec.fields("DoesNotExist").unwrap_err();
        assert!(matches!(err, ForgeError::SchemaNotFound(_)));
    }

    #[test]
    fn field_description_and_default() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: FieldMeta
  version: "1.0"
paths: {}
components:
  schemas:
    Config:
      type: object
      properties:
        timeout:
          type: integer
          description: Request timeout in ms
          default: 30000
          format: int32
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let fields = spec.fields("Config").expect("fields");
        let timeout = fields.iter().find(|f| f.name == "timeout").expect("timeout");
        assert_eq!(timeout.description.as_deref(), Some("Request timeout in ms"));
        assert_eq!(timeout.default, Some(serde_json::json!(30000)));
        assert_eq!(timeout.format.as_deref(), Some("int32"));
    }

    #[test]
    fn field_required_vs_optional() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let fields = spec.fields("CreateSecret").expect("fields");
        let name_f = fields.iter().find(|f| f.name == "name").unwrap();
        let value_f = fields.iter().find(|f| f.name == "value").unwrap();
        let token_f = fields.iter().find(|f| f.name == "token").unwrap();
        assert!(name_f.required);
        assert!(value_f.required);
        assert!(!token_f.required);
    }

    // ========================================================================
    // allOf — inline schema (no ref_path), circular ref protection
    // ========================================================================

    #[test]
    fn allof_inline_schema_merged() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: AllOfInline
  version: "1.0"
paths: {}
components:
  schemas:
    Combined:
      type: object
      allOf:
        - type: object
          properties:
            from_inline:
              type: string
      properties:
        own_field:
          type: integer
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let fields = spec.fields("Combined").expect("fields");
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"from_inline"));
        assert!(names.contains(&"own_field"));
    }

    #[test]
    fn allof_property_override_last_wins() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: AllOfOverride
  version: "1.0"
paths: {}
components:
  schemas:
    Base:
      type: object
      properties:
        field:
          type: string
    Override:
      type: object
      allOf:
        - $ref: '#/components/schemas/Base'
      required:
        - field
      properties:
        field:
          type: integer
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let fields = spec.fields("Override").expect("fields");
        let field = fields.iter().find(|f| f.name == "field").expect("field");
        assert_eq!(field.type_info, TypeInfo::Integer);
        assert!(field.required);
    }

    // ========================================================================
    // group_by_crud_pattern — edge cases
    // ========================================================================

    #[test]
    fn crud_grouping_skips_unrecognized_verbs() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Unknown Verbs
  version: "1.0"
paths:
  /custom-action:
    post:
      operationId: customAction
      responses:
        "200":
          description: ok
  /create-item:
    post:
      operationId: createItem
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let groups = spec.group_by_crud_pattern();
        let has_custom = groups.iter().any(|g| g.base_name.contains("custom"));
        assert!(
            !has_custom,
            "custom-action should not create a CRUD group"
        );
        let has_item = groups.iter().any(|g| g.create.is_some());
        assert!(has_item);
    }

    #[test]
    fn crud_grouping_empty_spec() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Empty
  version: "1.0"
paths: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let groups = spec.group_by_crud_pattern();
        assert!(groups.is_empty());
    }

    #[test]
    fn crud_grouping_all_verb_types() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: All Verbs
  version: "1.0"
paths:
  /create-widget:
    post:
      operationId: createWidget
      responses:
        "200":
          description: ok
  /get-widget:
    post:
      operationId: getWidget
      responses:
        "200":
          description: ok
  /update-widget:
    post:
      operationId: updateWidget
      responses:
        "200":
          description: ok
  /delete-widget:
    post:
      operationId: deleteWidget
      responses:
        "200":
          description: ok
  /list-widgets:
    post:
      operationId: listWidgets
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let groups = spec.group_by_crud_pattern();
        let widget = groups.iter().find(|g| g.base_name == "widget").expect("widget group");
        assert!(widget.create.is_some(), "create");
        assert!(widget.read.is_some(), "read");
        assert!(widget.update.is_some(), "update");
        assert!(widget.delete.is_some(), "delete");
        let list_group = groups.iter().find(|g| g.list.is_some());
        assert!(list_group.is_some(), "list group should exist");
    }

    #[test]
    fn crud_grouping_add_and_remove_verbs() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Add Remove
  version: "1.0"
paths:
  /add-user:
    post:
      operationId: addUser
      responses:
        "200":
          description: ok
  /remove-user:
    post:
      operationId: removeUser
      responses:
        "200":
          description: ok
  /describe-user:
    post:
      operationId: describeUser
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let groups = spec.group_by_crud_pattern();
        let user = groups.iter().find(|g| g.base_name == "user").expect("user group");
        assert!(user.create.is_some(), "add- should map to create");
        assert!(user.delete.is_some(), "remove- should map to delete");
        assert!(user.read.is_some(), "describe- should map to read");
    }

    // ========================================================================
    // detect_crud_verb / strip_verb_prefix — thorough edge case coverage
    // ========================================================================

    #[test]
    fn detect_crud_verb_with_hyphenated_operation_id() {
        let (verb, base) = Spec::detect_crud_verb("create-auth-method");
        assert!(matches!(verb, CrudVerb::Create));
        assert_eq!(base, "auth-method");
    }

    #[test]
    fn detect_crud_verb_no_match() {
        let (verb, base) = Spec::detect_crud_verb("custom-action");
        assert!(matches!(verb, CrudVerb::None));
        assert!(base.is_empty());
    }

    #[test]
    fn strip_verb_prefix_all_verbs() {
        assert_eq!(Spec::strip_verb_prefix("create-foo"), "foo");
        assert_eq!(Spec::strip_verb_prefix("add-foo"), "foo");
        assert_eq!(Spec::strip_verb_prefix("get-foo"), "foo");
        assert_eq!(Spec::strip_verb_prefix("describe-foo"), "foo");
        assert_eq!(Spec::strip_verb_prefix("update-foo"), "foo");
        assert_eq!(Spec::strip_verb_prefix("delete-foo"), "foo");
        assert_eq!(Spec::strip_verb_prefix("remove-foo"), "foo");
        assert_eq!(Spec::strip_verb_prefix("list-foo"), "foo");
    }

    #[test]
    fn strip_verb_prefix_no_match() {
        assert_eq!(Spec::strip_verb_prefix("custom-foo"), "");
    }

    #[test]
    fn strip_verb_prefix_case_insensitive() {
        assert_eq!(Spec::strip_verb_prefix("Create-Foo"), "foo");
        assert_eq!(Spec::strip_verb_prefix("GET-bar"), "bar");
    }

    // ========================================================================
    // RpcPattern — case insensitivity, edge cases
    // ========================================================================

    #[test]
    fn rpc_pattern_case_insensitive_match() {
        let pat = RpcPattern::new(RpcCrudVerb::Create, "/Create-{resource}", "{0}");
        assert_eq!(
            pat.try_match("/create-secret"),
            Some("secret".to_string())
        );
        assert_eq!(
            pat.try_match("/CREATE-SECRET"),
            Some("secret".to_string())
        );
    }

    #[test]
    fn rpc_pattern_empty_resource_returns_none() {
        let pat = RpcPattern::new(RpcCrudVerb::Create, "/create-{resource}", "{0}");
        assert_eq!(pat.try_match("/create-"), None);
    }

    #[test]
    fn rpc_pattern_no_resource_placeholder_exact() {
        let pat = RpcPattern::new(RpcCrudVerb::Read, "/health-check", "health");
        assert_eq!(pat.try_match("/health-check"), Some("health".to_string()));
        assert_eq!(pat.try_match("/health-checks"), None);
        assert_eq!(pat.try_match("/other"), None);
    }

    #[test]
    fn rpc_pattern_suffix_not_found() {
        let pat = RpcPattern::new(
            RpcCrudVerb::Create,
            "/create-{resource}-target",
            "target_{0}",
        );
        assert_eq!(pat.try_match("/create-aws-bucket"), None);
    }

    #[test]
    fn rpc_pattern_hyphen_to_underscore_in_group() {
        let pat = RpcPattern::new(RpcCrudVerb::Create, "/create-{resource}", "{0}");
        assert_eq!(
            pat.try_match("/create-my-resource"),
            Some("my_resource".to_string())
        );
    }

    #[test]
    fn rpc_pattern_prefix_no_match() {
        let pat = RpcPattern::new(RpcCrudVerb::Create, "/api/create-{resource}", "{0}");
        assert_eq!(pat.try_match("/create-foo"), None);
    }

    // ========================================================================
    // RpcCrudGrouper — patterns() method, unmatched endpoints, empty inputs
    // ========================================================================

    #[test]
    fn rpc_grouper_patterns_method() {
        let grouper = RpcCrudGrouper::new().patterns(vec![
            RpcPattern::new(RpcCrudVerb::Create, "/make-{resource}", "{0}"),
            RpcPattern::new(RpcCrudVerb::Delete, "/destroy-{resource}", "{0}"),
        ]);
        let endpoints = vec![
            Endpoint {
                path: "/make-widget".to_string(),
                method: "post".to_string(),
                operation_id: Some("makeWidget".to_string()),
                summary: None,
                tags: vec![],
                request_schema_ref: None,
                response_schema_ref: None,
            },
            Endpoint {
                path: "/destroy-widget".to_string(),
                method: "post".to_string(),
                operation_id: Some("destroyWidget".to_string()),
                summary: None,
                tags: vec![],
                request_schema_ref: None,
                response_schema_ref: None,
            },
        ];
        let groups = grouper.group(&endpoints);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].base_name, "widget");
        assert!(groups[0].create.is_some());
        assert!(groups[0].delete.is_some());
    }

    #[test]
    fn rpc_grouper_unmatched_endpoints_ignored() {
        let grouper = RpcCrudGrouper::new().pattern(RpcPattern::new(
            RpcCrudVerb::Create,
            "/create-{resource}",
            "{0}",
        ));
        let endpoints = vec![
            Endpoint {
                path: "/create-thing".to_string(),
                method: "post".to_string(),
                operation_id: None,
                summary: None,
                tags: vec![],
                request_schema_ref: None,
                response_schema_ref: None,
            },
            Endpoint {
                path: "/random-path".to_string(),
                method: "get".to_string(),
                operation_id: None,
                summary: None,
                tags: vec![],
                request_schema_ref: None,
                response_schema_ref: None,
            },
        ];
        let groups = grouper.group(&endpoints);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].base_name, "thing");
    }

    #[test]
    fn rpc_grouper_empty_endpoints() {
        let grouper = RpcCrudGrouper::default_patterns();
        let groups = grouper.group(&[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn rpc_grouper_first_pattern_wins() {
        let grouper = RpcCrudGrouper::new()
            .pattern(RpcPattern::new(
                RpcCrudVerb::Create,
                "/create-{resource}",
                "specific_{0}",
            ))
            .pattern(RpcPattern::new(
                RpcCrudVerb::Create,
                "/create-{resource}",
                "fallback_{0}",
            ));
        let endpoints = vec![Endpoint {
            path: "/create-item".to_string(),
            method: "post".to_string(),
            operation_id: None,
            summary: None,
            tags: vec![],
            request_schema_ref: None,
            response_schema_ref: None,
        }];
        let groups = grouper.group(&endpoints);
        assert_eq!(groups[0].base_name, "specific_item");
    }

    // ========================================================================
    // Akeyless patterns — list-auth-methods, list-targets, list-items, list-*
    // ========================================================================

    #[test]
    fn rpc_grouper_akeyless_list_auth_methods() {
        let spec_str = r#"
openapi: "3.0.0"
info:
  title: List Test
  version: "1.0"
paths:
  /list-auth-methods:
    post:
      operationId: listAuthMethods
      responses:
        "200":
          description: ok
  /list-targets:
    post:
      operationId: listTargets
      responses:
        "200":
          description: ok
  /list-items:
    post:
      operationId: listItems
      responses:
        "200":
          description: ok
  /list-dynamic-secrets:
    post:
      operationId: listDynamicSecrets
      responses:
        "200":
          description: ok
  /list-rotated-secrets:
    post:
      operationId: listRotatedSecrets
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(spec_str).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let auth = groups.iter().find(|g| g.base_name == "auth_method");
        assert!(auth.is_some(), "should have auth_method group for list-auth-methods");
        assert!(auth.unwrap().list.is_some());

        let target = groups.iter().find(|g| g.base_name == "target");
        assert!(target.is_some(), "should have target group for list-targets");
        assert!(target.unwrap().list.is_some());

        let item = groups.iter().find(|g| g.base_name == "item");
        assert!(item.is_some(), "should have item group for list-items");
        assert!(item.unwrap().list.is_some());
    }

    // ========================================================================
    // Enum values with non-string entries
    // ========================================================================

    #[test]
    fn enum_values_non_string_entries() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: MixedEnum
  version: "1.0"
paths: {}
components:
  schemas:
    MixedEnum:
      type: object
      properties:
        code:
          type: integer
          enum:
            - 100
            - 200
            - 300
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let fields = spec.fields("MixedEnum").expect("fields");
        let code = fields.iter().find(|f| f.name == "code").expect("code");
        let enums = code.enum_values.as_ref().expect("should have enum_values");
        assert_eq!(enums.len(), 3);
        assert!(enums.contains(&"100".to_string()));
        assert!(enums.contains(&"200".to_string()));
        assert!(enums.contains(&"300".to_string()));
    }

    // ========================================================================
    // Endpoint with operation_id from path fallback in group_by_crud_pattern
    // ========================================================================

    #[test]
    fn crud_grouping_uses_path_when_no_operation_id() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: No OpId
  version: "1.0"
paths:
  /create-thing:
    post:
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let groups = spec.group_by_crud_pattern();
        assert!(!groups.is_empty(), "should still group by path when no operation_id");
        let has_create = groups.iter().any(|g| g.create.is_some());
        assert!(has_create);
    }

    // ========================================================================
    // RpcCrudVerb — PartialEq, Debug, Clone, Copy
    // ========================================================================

    #[test]
    fn rpc_crud_verb_traits() {
        let verb = RpcCrudVerb::Create;
        let cloned = verb;
        assert_eq!(verb, cloned);
        assert_eq!(verb, RpcCrudVerb::Create);
        assert_ne!(verb, RpcCrudVerb::Delete);
        let dbg = format!("{verb:?}");
        assert!(dbg.contains("Create"));
    }

    // ========================================================================
    // CrudGroup, Endpoint, Field, SchemaDiff, FieldChange — Debug, Clone
    // ========================================================================

    #[test]
    fn structs_are_debug_and_clone() {
        let ep = Endpoint {
            path: "/test".to_string(),
            method: "get".to_string(),
            operation_id: Some("testOp".to_string()),
            summary: Some("A summary".to_string()),
            tags: vec!["tag1".to_string()],
            request_schema_ref: None,
            response_schema_ref: Some("Output".to_string()),
        };
        let cloned = ep.clone();
        assert_eq!(cloned.path, "/test");
        let dbg = format!("{ep:?}");
        assert!(dbg.contains("testOp"));

        let field = Field {
            name: "x".to_string(),
            type_info: TypeInfo::String,
            required: true,
            description: Some("desc".to_string()),
            default: Some(serde_json::json!("default")),
            format: Some("date".to_string()),
            enum_values: None,
        };
        let field_cloned = field.clone();
        assert_eq!(field_cloned.name, "x");
        let _ = format!("{field:?}");

        let diff = SchemaDiff {
            added: vec!["a".to_string()],
            removed: vec!["b".to_string()],
            changed: vec![FieldChange {
                name: "c".to_string(),
                old_type: TypeInfo::String,
                new_type: TypeInfo::Integer,
                required_changed: true,
            }],
        };
        let diff_cloned = diff.clone();
        assert_eq!(diff_cloned.added, vec!["a"]);
        let _ = format!("{diff:?}");

        let group = CrudGroup {
            base_name: "test".to_string(),
            create: Some(ep.clone()),
            read: None,
            update: None,
            delete: None,
            list: None,
        };
        let group_cloned = group.clone();
        assert_eq!(group_cloned.base_name, "test");
        let _ = format!("{group:?}");
    }

    // ========================================================================
    // Spec::schema_names with empty schemas map (components present but empty)
    // ========================================================================

    #[test]
    fn schema_names_with_empty_schemas() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Empty Schemas
  version: "1.0"
paths: {}
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        assert!(spec.schema_names().is_empty());
    }

    // ========================================================================
    // Request schema ref extraction edge: non-JSON content type
    // ========================================================================

    #[test]
    fn request_ref_non_json_content_type() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: NonJson
  version: "1.0"
paths:
  /upload:
    post:
      operationId: upload
      requestBody:
        content:
          multipart/form-data:
            schema:
              type: object
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let ep = spec.endpoint_by_path("/upload").expect("found");
        assert!(
            ep.request_schema_ref.is_none(),
            "non-JSON content type should not extract schema ref"
        );
    }

    // ========================================================================
    // Response without content
    // ========================================================================

    #[test]
    fn response_without_content() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: NoContent
  version: "1.0"
paths:
  /delete:
    delete:
      operationId: deleteItem
      responses:
        "204":
          description: No Content
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let ep = spec.endpoint_by_path("/delete").expect("found");
        assert!(ep.response_schema_ref.is_none());
    }

    // ========================================================================
    // Spec with allOf referencing nonexistent schema (resolve failure is silent)
    // ========================================================================

    #[test]
    fn allof_ref_to_missing_schema_silently_skips() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: MissingRef
  version: "1.0"
paths: {}
components:
  schemas:
    Derived:
      type: object
      allOf:
        - $ref: '#/components/schemas/DoesNotExist'
      properties:
        own:
          type: string
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let fields = spec.fields("Derived").expect("fields");
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"own"));
        assert!(!names.contains(&"DoesNotExist"));
    }

    // ========================================================================
    // FromStr trait implementation
    // ========================================================================

    #[test]
    fn from_str_trait_yaml() {
        let spec: Spec = MINIMAL_SPEC.parse().expect("FromStr should parse YAML");
        assert_eq!(spec.endpoints().len(), 4);
    }

    #[test]
    fn from_str_trait_json() {
        let json = r#"{"openapi":"3.0.0","info":{"title":"J","version":"1"},"paths":{}}"#;
        let spec: Spec = json.parse().expect("FromStr should parse JSON");
        assert!(spec.endpoints().is_empty());
    }

    #[test]
    fn from_str_trait_error() {
        let result: Result<Spec, _> = "{{totally broken".parse();
        assert!(result.is_err());
    }

    #[test]
    fn parse_convenience_method() {
        let spec = Spec::parse(MINIMAL_SPEC).expect("parse");
        assert_eq!(spec.endpoints().len(), 4);
    }

    // ========================================================================
    // Spec — Debug + Clone derives
    // ========================================================================

    #[test]
    fn spec_debug_and_clone() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let cloned = spec.clone();
        assert_eq!(cloned.endpoints().len(), spec.endpoints().len());
        let dbg = format!("{spec:?}");
        assert!(dbg.contains("Spec"));
    }

    // ========================================================================
    // RpcCrudGrouper — Debug + Clone derives
    // ========================================================================

    #[test]
    fn rpc_crud_grouper_debug_and_clone() {
        let grouper = RpcCrudGrouper::default_patterns();
        let cloned = grouper.clone();
        let dbg = format!("{grouper:?}");
        assert!(dbg.contains("RpcCrudGrouper"));
        // Cloned grouper should produce identical results
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let g1 = grouper.group_spec(&spec);
        let g2 = cloned.group_spec(&spec);
        assert_eq!(g1.len(), g2.len());
    }

    // ========================================================================
    // RpcPattern — Debug + Clone derives
    // ========================================================================

    #[test]
    fn rpc_pattern_debug_and_clone() {
        let pat = RpcPattern::new(RpcCrudVerb::Create, "/create-{resource}", "{0}");
        let cloned = pat.clone();
        assert_eq!(cloned.template, "/create-{resource}");
        assert_eq!(cloned.group_name, "{0}");
        assert_eq!(cloned.verb, RpcCrudVerb::Create);
        let dbg = format!("{pat:?}");
        assert!(dbg.contains("RpcPattern"));
    }

    // ========================================================================
    // RpcCrudVerb — all variant equality comparisons
    // ========================================================================

    #[test]
    fn rpc_crud_verb_all_variants_eq() {
        let variants = [
            RpcCrudVerb::Create,
            RpcCrudVerb::Read,
            RpcCrudVerb::Update,
            RpcCrudVerb::Delete,
            RpcCrudVerb::List,
        ];
        for (i, v1) in variants.iter().enumerate() {
            for (j, v2) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(v1, v2);
                } else {
                    assert_ne!(v1, v2);
                }
            }
        }
    }

    // ========================================================================
    // Spec::endpoints — field extraction completeness
    // ========================================================================

    #[test]
    fn endpoints_request_and_response_refs_complete() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let ep_create = spec.endpoint_by_path("/create-secret").unwrap();
        assert_eq!(ep_create.request_schema_ref.as_deref(), Some("CreateSecret"));
        assert_eq!(
            ep_create.response_schema_ref.as_deref(),
            Some("CreateSecretOutput")
        );

        let ep_get = spec.endpoint_by_path("/get-secret-value").unwrap();
        assert_eq!(
            ep_get.request_schema_ref.as_deref(),
            Some("GetSecretValue")
        );
        assert_eq!(
            ep_get.response_schema_ref.as_deref(),
            Some("GetSecretValueOutput")
        );

        let ep_update = spec.endpoint_by_path("/update-secret-val").unwrap();
        assert_eq!(
            ep_update.request_schema_ref.as_deref(),
            Some("UpdateSecretVal")
        );
        assert!(
            ep_update.response_schema_ref.is_none(),
            "update has no response ref"
        );

        let ep_delete = spec.endpoint_by_path("/delete-item").unwrap();
        assert_eq!(ep_delete.request_schema_ref.as_deref(), Some("DeleteItem"));
        assert!(ep_delete.response_schema_ref.is_none());
    }

    // ========================================================================
    // diff_schemas — symmetric diff properties
    // ========================================================================

    #[test]
    fn diff_schemas_symmetric_added_removed() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let forward = spec
            .diff_schemas("CreateSecret", "UpdateSecretVal")
            .expect("diff");
        let reverse = spec
            .diff_schemas("UpdateSecretVal", "CreateSecret")
            .expect("diff");
        assert_eq!(forward.added, reverse.removed);
        assert_eq!(forward.removed, reverse.added);
    }

    // ========================================================================
    // resolve_type — various schema types
    // ========================================================================

    #[test]
    fn resolve_type_integer() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let schema = SchemaObject {
            schema_type: Some("integer".to_string()),
            ..SchemaObject::default()
        };
        assert_eq!(spec.resolve_type(&schema), TypeInfo::Integer);
    }

    #[test]
    fn resolve_type_boolean() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let schema = SchemaObject {
            schema_type: Some("boolean".to_string()),
            ..SchemaObject::default()
        };
        assert_eq!(spec.resolve_type(&schema), TypeInfo::Boolean);
    }

    #[test]
    fn resolve_type_array_of_integers() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let item_schema = SchemaObject {
            schema_type: Some("integer".to_string()),
            ..SchemaObject::default()
        };
        let schema = SchemaObject {
            schema_type: Some("array".to_string()),
            items: Some(Box::new(item_schema)),
            ..SchemaObject::default()
        };
        assert_eq!(
            spec.resolve_type(&schema),
            TypeInfo::Array(Box::new(TypeInfo::Integer))
        );
    }

    // ========================================================================
    // resolve_schema_or_ref_type — empty ref_path edge case
    // ========================================================================

    #[test]
    fn resolve_schema_or_ref_type_empty_ref() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let sor = SchemaOrRef::Ref {
            ref_path: String::new(),
        };
        let ti = spec.resolve_schema_or_ref_type(&sor);
        assert_eq!(ti, TypeInfo::Object(String::new()));
    }

    // ========================================================================
    // Spec::schema — look up existing schemas
    // ========================================================================

    #[test]
    fn schema_lookup_returns_correct_schema() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let schema = spec.schema("CreateSecret").unwrap();
        assert!(schema.properties.contains_key("name"));
        assert!(schema.properties.contains_key("value"));
        assert!(schema.required.contains(&"name".to_string()));
    }

    // ========================================================================
    // group_spec — via RpcCrudGrouper on Spec directly
    // ========================================================================

    #[test]
    fn rpc_grouper_group_spec_same_as_group() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let via_spec = grouper.group_spec(&spec);
        let via_endpoints = grouper.group(&spec.endpoints());
        assert_eq!(via_spec.len(), via_endpoints.len());
        for (s, e) in via_spec.iter().zip(via_endpoints.iter()) {
            assert_eq!(s.base_name, e.base_name);
        }
    }

    // ========================================================================
    // CrudGroup — verb count helper test
    // ========================================================================

    #[test]
    fn crud_group_verb_coverage() {
        let spec = Spec::from_str(AKEYLESS_SPEC).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);

        let total_endpoints: usize = groups
            .iter()
            .map(|g| {
                [
                    g.create.is_some(),
                    g.read.is_some(),
                    g.update.is_some(),
                    g.delete.is_some(),
                    g.list.is_some(),
                ]
                .iter()
                .filter(|&&v| v)
                .count()
            })
            .sum();
        assert!(
            total_endpoints > 0,
            "should have matched at least some endpoints"
        );
    }

    // ========================================================================
    // Multiple patterns matching different verbs for same resource
    // ========================================================================

    #[test]
    fn rpc_grouper_multiple_verbs_same_resource() {
        let grouper = RpcCrudGrouper::default_patterns();
        let endpoints = vec![
            Endpoint {
                path: "/create-widget".to_string(),
                method: "post".to_string(),
                operation_id: None,
                summary: None,
                tags: vec![],
                request_schema_ref: None,
                response_schema_ref: None,
            },
            Endpoint {
                path: "/get-widget".to_string(),
                method: "get".to_string(),
                operation_id: None,
                summary: None,
                tags: vec![],
                request_schema_ref: None,
                response_schema_ref: None,
            },
            Endpoint {
                path: "/update-widget".to_string(),
                method: "put".to_string(),
                operation_id: None,
                summary: None,
                tags: vec![],
                request_schema_ref: None,
                response_schema_ref: None,
            },
            Endpoint {
                path: "/delete-widget".to_string(),
                method: "delete".to_string(),
                operation_id: None,
                summary: None,
                tags: vec![],
                request_schema_ref: None,
                response_schema_ref: None,
            },
            Endpoint {
                path: "/list-widget".to_string(),
                method: "get".to_string(),
                operation_id: None,
                summary: None,
                tags: vec![],
                request_schema_ref: None,
                response_schema_ref: None,
            },
        ];
        let groups = grouper.group(&endpoints);
        assert_eq!(groups.len(), 1);
        let w = &groups[0];
        assert_eq!(w.base_name, "widget");
        assert!(w.create.is_some());
        assert!(w.read.is_some());
        assert!(w.update.is_some());
        assert!(w.delete.is_some());
        assert!(w.list.is_some());
    }

    // ========================================================================
    // Error source chain tests (std::error::Error)
    // ========================================================================

    #[test]
    fn forge_error_source_io() {
        use std::error::Error;
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err = ForgeError::Io(inner);
        assert!(err.source().is_some());
    }

    #[test]
    fn forge_error_source_yaml() {
        use std::error::Error;
        let yaml_err = serde_yaml_ng::from_str::<serde_json::Value>("{{bad")
            .expect_err("should fail");
        let err = ForgeError::Yaml(yaml_err);
        assert!(err.source().is_some());
    }

    #[test]
    fn forge_error_source_json() {
        use std::error::Error;
        let json_err = serde_json::from_str::<serde_json::Value>("not json")
            .expect_err("should fail");
        let err = ForgeError::Json(json_err);
        assert!(err.source().is_some());
    }

    #[test]
    fn forge_error_source_schema_not_found() {
        use std::error::Error;
        let err = ForgeError::SchemaNotFound("X".into());
        assert!(err.source().is_none());
    }

    #[test]
    fn forge_error_source_unresolved_ref() {
        use std::error::Error;
        let err = ForgeError::UnresolvedRef("X".into());
        assert!(err.source().is_none());
    }

    #[test]
    fn forge_error_source_unsupported_version() {
        use std::error::Error;
        let err = ForgeError::UnsupportedVersion("2.0".into());
        assert!(err.source().is_none());
    }

    // ========================================================================
    // detect_crud_verb — all verb types
    // ========================================================================

    #[test]
    fn detect_crud_verb_get() {
        let (verb, base) = Spec::detect_crud_verb("get-user");
        assert!(matches!(verb, CrudVerb::Read));
        assert_eq!(base, "user");
    }

    #[test]
    fn detect_crud_verb_describe() {
        let (verb, base) = Spec::detect_crud_verb("describe-item");
        assert!(matches!(verb, CrudVerb::Read));
        assert_eq!(base, "item");
    }

    #[test]
    fn detect_crud_verb_update() {
        let (verb, base) = Spec::detect_crud_verb("update-config");
        assert!(matches!(verb, CrudVerb::Update));
        assert_eq!(base, "config");
    }

    #[test]
    fn detect_crud_verb_delete() {
        let (verb, base) = Spec::detect_crud_verb("delete-entry");
        assert!(matches!(verb, CrudVerb::Delete));
        assert_eq!(base, "entry");
    }

    #[test]
    fn detect_crud_verb_remove() {
        let (verb, base) = Spec::detect_crud_verb("remove-member");
        assert!(matches!(verb, CrudVerb::Delete));
        assert_eq!(base, "member");
    }

    #[test]
    fn detect_crud_verb_add() {
        let (verb, base) = Spec::detect_crud_verb("add-member");
        assert!(matches!(verb, CrudVerb::Create));
        assert_eq!(base, "member");
    }

    #[test]
    fn detect_crud_verb_list() {
        let (verb, base) = Spec::detect_crud_verb("list-users");
        assert!(matches!(verb, CrudVerb::List));
        assert_eq!(base, "users");
    }

    #[test]
    fn detect_crud_verb_camel_case() {
        let (verb, base) = Spec::detect_crud_verb("createUser");
        assert!(matches!(verb, CrudVerb::Create));
        assert!(!base.is_empty());
    }

    // ========================================================================
    // Spec with nested object types
    // ========================================================================

    #[test]
    fn fields_nested_object_type() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Nested
  version: "1.0"
paths: {}
components:
  schemas:
    Outer:
      type: object
      properties:
        inner:
          type: object
          properties:
            value:
              type: string
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let fields = spec.fields("Outer").expect("fields");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "inner");
    }

    // ========================================================================
    // Spec with number type
    // ========================================================================

    #[test]
    fn resolve_type_number() {
        let spec = Spec::from_str(MINIMAL_SPEC).expect("parse");
        let schema = SchemaObject {
            schema_type: Some("number".to_string()),
            ..SchemaObject::default()
        };
        let ti = spec.resolve_type(&schema);
        assert_eq!(ti, TypeInfo::Number);
    }

    // ========================================================================
    // Endpoint with all five HTTP methods properly handled
    // ========================================================================

    #[test]
    fn endpoint_method_ordering_is_deterministic() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Order
  version: "1.0"
paths:
  /resource:
    get:
      operationId: getR
      responses:
        "200":
          description: ok
    post:
      operationId: postR
      responses:
        "200":
          description: ok
    put:
      operationId: putR
      responses:
        "200":
          description: ok
    delete:
      operationId: deleteR
      responses:
        "200":
          description: ok
    patch:
      operationId: patchR
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let eps = spec.endpoints();
        let methods: Vec<&str> = eps.iter().map(|e| e.method.as_str()).collect();
        assert_eq!(methods, vec!["get", "post", "put", "delete", "patch"]);
    }

    // ========================================================================
    // Spec with multiple paths
    // ========================================================================

    #[test]
    fn multiple_paths_all_enumerated() {
        let yaml = r#"
openapi: "3.0.0"
info:
  title: Multi
  version: "1.0"
paths:
  /a:
    get:
      operationId: getA
      responses:
        "200":
          description: ok
  /b:
    post:
      operationId: postB
      responses:
        "200":
          description: ok
  /c:
    put:
      operationId: putC
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(yaml).expect("parse");
        let eps = spec.endpoints();
        assert_eq!(eps.len(), 3);
        let paths: Vec<&str> = eps.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"/a"));
        assert!(paths.contains(&"/b"));
        assert!(paths.contains(&"/c"));
    }

    // ========================================================================
    // SchemaOrRef::from_schema round-trip properties
    // ========================================================================

    #[test]
    #[allow(deprecated)]
    fn schema_or_ref_from_schema_ref_name_round_trip() {
        let schema = sekkei::Schema {
            ref_path: Some("#/components/schemas/Test".to_string()),
            ..sekkei::Schema::default()
        };
        let sor = SchemaOrRef::from_schema(&schema);
        assert_eq!(sor.ref_name(), Some("Test"));
    }

    // ========================================================================
    // Akeyless pattern B: create-{variant}-target
    // ========================================================================

    #[test]
    fn rpc_grouper_target_pattern_b() {
        let spec_str = r#"
openapi: "3.0.0"
info:
  title: Target B
  version: "1.0"
paths:
  /create-gke-target:
    post:
      operationId: createGkeTarget
      responses:
        "200":
          description: ok
  /update-gke-target:
    post:
      operationId: updateGkeTarget
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(spec_str).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);
        let tgt = groups
            .iter()
            .find(|g| g.base_name == "target_gke")
            .expect("target_gke group");
        assert!(tgt.create.is_some());
        assert!(tgt.update.is_some());
    }

    // ========================================================================
    // Akeyless dynamic-secret pattern B: {verb}-dynamic-secret-{variant}
    // ========================================================================

    #[test]
    fn rpc_grouper_dynamic_secret_pattern_b() {
        let spec_str = r#"
openapi: "3.0.0"
info:
  title: DynSec B
  version: "1.0"
paths:
  /create-dynamic-secret-mysql:
    post:
      operationId: createDynamicSecretMysql
      responses:
        "200":
          description: ok
  /update-dynamic-secret-mysql:
    post:
      operationId: updateDynamicSecretMysql
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(spec_str).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);
        let ds = groups
            .iter()
            .find(|g| g.base_name == "dynamic_secret_mysql")
            .expect("dynamic_secret_mysql group");
        assert!(ds.create.is_some());
        assert!(ds.update.is_some());
    }

    // ========================================================================
    // Akeyless rotated-secret pattern B: {verb}-rotated-secret-{variant}
    // ========================================================================

    #[test]
    fn rpc_grouper_rotated_secret_pattern_b() {
        let spec_str = r#"
openapi: "3.0.0"
info:
  title: RotSec B
  version: "1.0"
paths:
  /create-rotated-secret-postgres:
    post:
      operationId: createRotatedSecretPostgres
      responses:
        "200":
          description: ok
  /update-rotated-secret-postgres:
    post:
      operationId: updateRotatedSecretPostgres
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(spec_str).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);
        let rs = groups
            .iter()
            .find(|g| g.base_name == "rotated_secret_postgres")
            .expect("rotated_secret_postgres group");
        assert!(rs.create.is_some());
        assert!(rs.update.is_some());
    }

    // ========================================================================
    // Gateway delete-producer pattern
    // ========================================================================

    #[test]
    fn rpc_grouper_gateway_delete_producer() {
        let spec_str = r#"
openapi: "3.0.0"
info:
  title: GW Delete
  version: "1.0"
paths:
  /gateway-delete-producer-aws:
    post:
      operationId: gatewayDeleteProducerAws
      responses:
        "200":
          description: ok
components:
  schemas: {}
"#;
        let spec = Spec::from_str(spec_str).expect("parse");
        let grouper = RpcCrudGrouper::akeyless_patterns();
        let groups = grouper.group_spec(&spec);
        let gp = groups
            .iter()
            .find(|g| g.base_name == "gateway_producer_aws")
            .expect("gateway_producer_aws group");
        assert!(gp.delete.is_some());
    }

    // ========================================================================
    // Display / FromStr round-trip for RpcCrudVerb
    // ========================================================================

    #[test]
    fn rpc_crud_verb_display_round_trip() {
        let verbs = [
            RpcCrudVerb::Create,
            RpcCrudVerb::Read,
            RpcCrudVerb::Update,
            RpcCrudVerb::Delete,
            RpcCrudVerb::List,
        ];
        for verb in verbs {
            let s = verb.to_string();
            let parsed: RpcCrudVerb = s.parse().expect("round-trip parse");
            assert_eq!(parsed, verb);
        }
    }

    #[test]
    fn rpc_crud_verb_from_str_aliases() {
        assert_eq!("get".parse::<RpcCrudVerb>().unwrap(), RpcCrudVerb::Read);
        assert_eq!(
            "describe".parse::<RpcCrudVerb>().unwrap(),
            RpcCrudVerb::Read
        );
        assert_eq!(
            "remove".parse::<RpcCrudVerb>().unwrap(),
            RpcCrudVerb::Delete
        );
    }

    #[test]
    fn rpc_crud_verb_from_str_case_insensitive() {
        assert_eq!(
            "CREATE".parse::<RpcCrudVerb>().unwrap(),
            RpcCrudVerb::Create
        );
        assert_eq!(
            "Delete".parse::<RpcCrudVerb>().unwrap(),
            RpcCrudVerb::Delete
        );
    }

    #[test]
    fn rpc_crud_verb_from_str_invalid() {
        let err = "foobar".parse::<RpcCrudVerb>().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown CRUD verb"));
    }
}
