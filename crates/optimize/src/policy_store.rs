//! Durable storage for validated per-file-kind codec policies.
//!
//! Each policy is written atomically to a separate JSON file. Loading always checks the schema and
//! requested file kind; stale or malformed policies are reported rather than silently activated.

use crate::policy::{CodecPolicy, FileKind, PolicyError, POLICY_SCHEMA_VERSION};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum PolicyStoreError {
    Io(io::Error),
    Serialization(serde_json::Error),
    InvalidPolicy(PolicyError),
    FileKindMismatch { expected: FileKind, actual: FileKind },
}

impl From<io::Error> for PolicyStoreError {
    fn from(error: io::Error) -> Self { Self::Io(error) }
}
impl From<serde_json::Error> for PolicyStoreError {
    fn from(error: serde_json::Error) -> Self { Self::Serialization(error) }
}
impl From<PolicyError> for PolicyStoreError {
    fn from(error: PolicyError) -> Self { Self::InvalidPolicy(error) }
}

/// Stores policies under `<root>/codec-policies/v<schema>/<file-kind>.json`.
#[derive(Clone, Debug)]
pub struct CodecPolicyStore {
    root: PathBuf,
}

impl CodecPolicyStore {
    pub fn new(root: impl Into<PathBuf>) -> Self { Self { root: root.into() } }

    pub fn path_for(&self, kind: FileKind) -> PathBuf {
        self.root
            .join("codec-policies")
            .join(format!("v{POLICY_SCHEMA_VERSION}"))
            .join(format!("{}.json", kind.as_str()))
    }

    /// Save only policies that pass current-schema validation. Uses a same-directory temporary file
    /// and rename so readers never observe a partially written JSON document.
    pub fn save(&self, policy: &CodecPolicy) -> Result<PathBuf, PolicyStoreError> {
        policy.validate()?;
        let path = self.path_for(policy.file_kind);
        let parent = path.parent().expect("policy path always has a parent");
        fs::create_dir_all(parent)?;
        let bytes = serde_json::to_vec_pretty(policy)?;
        let temp = parent.join(format!(".{}.{}.tmp", policy.file_kind.as_str(), std::process::id()));
        fs::write(&temp, bytes)?;
        if let Err(error) = fs::rename(&temp, &path) {
            let _ = fs::remove_file(&temp);
            return Err(PolicyStoreError::Io(error));
        }
        Ok(path)
    }

    /// Load a policy for exactly this file kind. Missing policy returns `Ok(None)`; stale schema,
    /// invalid settings, malformed JSON, and mismatched kinds are explicit errors.
    pub fn load(&self, kind: FileKind) -> Result<Option<CodecPolicy>, PolicyStoreError> {
        let path = self.path_for(kind);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(PolicyStoreError::Io(error)),
        };
        let policy: CodecPolicy = serde_json::from_slice(&bytes)?;
        policy.validate()?;
        if policy.file_kind != kind {
            return Err(PolicyStoreError::FileKindMismatch { expected: kind, actual: policy.file_kind });
        }
        Ok(Some(policy))
    }

    /// Remove only the policy for the requested kind and current schema.
    pub fn remove(&self, kind: FileKind) -> Result<bool, PolicyStoreError> {
        match fs::remove_file(self.path_for(kind)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(PolicyStoreError::Io(error)),
        }
    }

    /// Exposes the configured root without allowing callers to mutate the store's path.
    pub fn root(&self) -> &Path { &self.root }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::CodecFamily;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("codec-policy-store-{}-{nonce}", std::process::id()))
    }

    fn sample(kind: FileKind) -> CodecPolicy {
        CodecPolicy {
            schema_version: POLICY_SCHEMA_VERSION,
            file_kind: kind,
            enabled_codecs: vec![CodecFamily::Lines],
            allow_composition: false,
            max_candidates: 8,
            latency_weight: 0.25,
        }
    }

    #[test]
    fn round_trips_policy_and_separates_file_kinds() {
        let root = temp_root();
        let store = CodecPolicyStore::new(&root);
        let json = sample(FileKind::Json);
        let yaml = sample(FileKind::Yaml);
        store.save(&json).unwrap();
        store.save(&yaml).unwrap();
        assert_eq!(store.load(FileKind::Json).unwrap(), Some(json));
        assert_eq!(store.load(FileKind::Yaml).unwrap(), Some(yaml));
        assert!(store.path_for(FileKind::Json).exists());
        assert_ne!(store.path_for(FileKind::Json), store.path_for(FileKind::Yaml));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_policy_is_not_an_error() {
        let root = temp_root();
        let store = CodecPolicyStore::new(&root);
        assert!(store.load(FileKind::Markdown).unwrap().is_none());
        assert!(!store.remove(FileKind::Markdown).unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_invalid_policy_and_rejects_stale_schema() {
        let root = temp_root();
        let store = CodecPolicyStore::new(&root);
        let mut invalid = sample(FileKind::Json);
        invalid.schema_version += 1;
        assert!(matches!(store.save(&invalid), Err(PolicyStoreError::InvalidPolicy(_))));
        let path = store.path_for(FileKind::Json);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, br#"{"schema_version":999,"file_kind":"json","enabled_codecs":["lines"],"allow_composition":false,"max_candidates":8,"latency_weight":0.25}"#).unwrap();
        assert!(matches!(store.load(FileKind::Json), Err(PolicyStoreError::InvalidPolicy(PolicyError::UnsupportedSchema(999)))));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unknown_fields_in_persisted_documents() {
        let root = temp_root();
        let store = CodecPolicyStore::new(&root);
        let path = store.path_for(FileKind::Json);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, br#"{"schema_version":1,"file_kind":"json","enabled_codecs":["lines"],"allow_composition":false,"max_candidates":8,"latency_weight":0.25,"surprise":true}"#).unwrap();
        assert!(matches!(store.load(FileKind::Json), Err(PolicyStoreError::Serialization(_))));
        fs::remove_dir_all(root).unwrap();
    }
}
