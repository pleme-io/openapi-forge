use std::path::Path;

use indexmap::IndexMap;

use crate::error::ForgeError;
use crate::types::{OpenApiSpec, Operation, SchemaObject, SchemaOrRef};

/// The CRUD verb detected from an RPC-style operation path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcCrudVerb {
    Create,
    Read,
    Update,
    Delete,
    List,
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
            } else if let Some(end_idx) = rest.find(&*suffix) {
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
                        create: None,
                        read: None,
                        update: None,
                        delete: None,
                        list: None,
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
    pub enum_values: Option<Vec<String>>,
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
            serde_yaml_ng::from_str(content)?
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
        self.resolve_fields_recursive(schema, &mut Vec::new())
    }

    fn resolve_fields_recursive(
        &self,
        schema: &SchemaObject,
        visited: &mut Vec<String>,
    ) -> Vec<Field> {
        let mut fields = Vec::new();

        // Handle allOf by merging properties from all referenced schemas
        if let Some(all_of) = &schema.all_of {
            for item in all_of {
                match item {
                    SchemaOrRef::Ref { ref_path } => {
                        if let Some(name) = ref_path.rsplit('/').next() {
                            // Prevent infinite recursion on circular refs
                            if !visited.contains(&name.to_string()) {
                                visited.push(name.to_string());
                                if let Ok(referenced) = self.schema(name) {
                                    fields
                                        .extend(self.resolve_fields_recursive(referenced, visited));
                                }
                            }
                        }
                    }
                    SchemaOrRef::Schema(s) => {
                        fields.extend(self.resolve_fields_recursive(s, visited));
                    }
                }
            }
        }

        for (name, prop) in &schema.properties {
            let required = schema.required.contains(name);
            let type_info = self.resolve_type(prop);
            let (description, default, format, enum_values) = match prop {
                SchemaOrRef::Schema(s) => {
                    let ev = s.enum_values.as_ref().map(|vals| {
                        vals.iter()
                            .filter_map(|v| match v {
                                serde_json::Value::String(s) => Some(s.clone()),
                                other => Some(other.to_string()),
                            })
                            .collect()
                    });
                    (
                        s.description.clone(),
                        s.default.clone(),
                        s.format.clone(),
                        ev,
                    )
                }
                SchemaOrRef::Ref { .. } => (None, None, None, None),
            };

            // Avoid duplicates from allOf merging — last definition wins
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
}
