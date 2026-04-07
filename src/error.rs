use thiserror::Error;

/// Errors that can occur when loading, parsing, or querying an `OpenAPI` spec.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ForgeError {
    /// An I/O error occurred while reading the spec file from disk.
    #[error("failed to read spec file: {0}")]
    Io(#[from] std::io::Error),

    /// The spec content could not be parsed as valid YAML.
    #[error("failed to parse YAML: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),

    /// The spec content could not be parsed as valid JSON.
    #[error("failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// A `$ref` pointer could not be resolved within the spec.
    #[error("unresolved $ref: {0}")]
    UnresolvedRef(String),

    /// A referenced schema name does not exist in `components/schemas`.
    #[error("schema not found: {0}")]
    SchemaNotFound(String),

    /// The spec declares a version other than 3.0.x.
    #[error("unsupported spec version: {0} (expected 3.0.x)")]
    UnsupportedVersion(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_io_error() {
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err = ForgeError::Io(inner);
        let msg = err.to_string();
        assert!(
            msg.contains("failed to read spec file"),
            "IO error display should include context prefix, got: {msg}"
        );
        assert!(msg.contains("gone"), "IO error display should preserve inner message");
    }

    #[test]
    fn display_yaml_error() {
        let yaml_err = serde_yaml_ng::from_str::<serde_json::Value>("{{bad")
            .expect_err("should fail");
        let err = ForgeError::Yaml(yaml_err);
        let msg = err.to_string();
        assert!(
            msg.contains("failed to parse YAML"),
            "YAML error display should include context prefix, got: {msg}"
        );
    }

    #[test]
    fn display_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json")
            .expect_err("should fail");
        let err = ForgeError::Json(json_err);
        let msg = err.to_string();
        assert!(
            msg.contains("failed to parse JSON"),
            "JSON error display should include context prefix, got: {msg}"
        );
    }

    #[test]
    fn display_unresolved_ref() {
        let err = ForgeError::UnresolvedRef("#/components/schemas/Missing".into());
        assert_eq!(
            err.to_string(),
            "unresolved $ref: #/components/schemas/Missing"
        );
    }

    #[test]
    fn display_schema_not_found() {
        let err = ForgeError::SchemaNotFound("NoSuchSchema".into());
        assert_eq!(err.to_string(), "schema not found: NoSuchSchema");
    }

    #[test]
    fn display_unsupported_version() {
        let err = ForgeError::UnsupportedVersion("2.0".into());
        assert_eq!(
            err.to_string(),
            "unsupported spec version: 2.0 (expected 3.0.x)"
        );
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let forge_err: ForgeError = io_err.into();
        assert!(matches!(forge_err, ForgeError::Io(_)));
    }

    #[test]
    fn from_yaml_error() {
        let yaml_err = serde_yaml_ng::from_str::<serde_json::Value>("{{bad")
            .expect_err("should fail");
        let forge_err: ForgeError = yaml_err.into();
        assert!(matches!(forge_err, ForgeError::Yaml(_)));
    }

    #[test]
    fn from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json")
            .expect_err("should fail");
        let forge_err: ForgeError = json_err.into();
        assert!(matches!(forge_err, ForgeError::Json(_)));
    }

    #[test]
    fn error_is_debug() {
        let err = ForgeError::SchemaNotFound("X".into());
        let debug = format!("{err:?}");
        assert!(debug.contains("SchemaNotFound"));
    }
}
