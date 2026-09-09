use crate::client::EngineError;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use wasmer::{Module, Store};

#[derive(Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub wasm_file: String,
    pub public_key_hex: String,
    pub signature_hex: String,
}

pub struct PluginLoader {
    store: Store,
}

impl PluginLoader {
    pub fn new() -> Self {
        Self {
            store: Store::default(),
        }
    }

    /// Loads an extractor JSON manifest, verifies cryptographic identity, and compiles the WASM.
    pub fn load_plugin<P: AsRef<Path>>(&self, manifest_path: P) -> Result<Module, EngineError> {
        let manifest_content = fs::read_to_string(manifest_path.as_ref())
            .map_err(|e| EngineError::FileSystemError(e.to_string()))?;
            
        let manifest: PluginManifest = serde_json::from_str(&manifest_content)
            .map_err(|e| EngineError::FileSystemError(format!("Invalid manifest JSON: {}", e)))?;

        let dir = manifest_path.as_ref().parent().unwrap_or_else(|| Path::new(""));
        let wasm_path = dir.join(&manifest.wasm_file);
        
        let wasm_bytes = fs::read(&wasm_path)
            .map_err(|e| EngineError::FileSystemError(e.to_string()))?;

        // Cryptographic integrity checks using Dalek Ed25519
        let pub_key_bytes = hex::decode(&manifest.public_key_hex)
            .map_err(|_| EngineError::OsApiError("Failed to decode public key hex".into()))?;
        let sig_bytes = hex::decode(&manifest.signature_hex)
            .map_err(|_| EngineError::OsApiError("Failed to decode signature hex".into()))?;

        let pub_key_arr: [u8; 32] = pub_key_bytes.try_into()
            .map_err(|_| EngineError::OsApiError("Invalid Ed25519 public key length".into()))?;
        let sig_arr: [u8; 64] = sig_bytes.try_into()
            .map_err(|_| EngineError::OsApiError("Invalid Ed25519 signature length".into()))?;

        let verifying_key = VerifyingKey::from_bytes(&pub_key_arr)
            .map_err(|e| EngineError::OsApiError(format!("Malformed public key: {}", e)))?;
        let signature = Signature::from_bytes(&sig_arr);

        verifying_key.verify(&wasm_bytes, &signature)
            .map_err(|_| EngineError::OsApiError("Plugin signature verification failed! Untrusted extractor.".into()))?;

        Module::new(&self.store, &wasm_bytes)
            .map_err(|e| EngineError::OsApiError(format!("WASM compilation failed: {}", e)))
    }
}