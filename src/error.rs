use thiserror::Error;

#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("failed to read spec file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("unresolved $ref: {0}")]
    UnresolvedRef(String),

    #[error("schema not found: {0}")]
    SchemaNotFound(String),

    #[error("unsupported spec version: {0} (expected 3.0.x)")]
    UnsupportedVersion(String),
}
