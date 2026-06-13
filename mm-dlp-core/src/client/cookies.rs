use crate::client::EngineError;
use bytes::Bytes;
use std::path::{Path, PathBuf};

pub struct CookieExtractor {
    browser_path: PathBuf,
}

impl CookieExtractor {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            browser_path: path.as_ref().to_path_buf(),
        }
    }

    pub fn extract(&self) -> Result<Bytes, EngineError> {
        let db_path = self.browser_path.join("Network/Cookies");

        // Safe fallback if the file is locked by a running browser instance
        let _file = std::fs::OpenOptions::new()
            .read(true)
            .open(&db_path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied || e.kind() == std::io::ErrorKind::WouldBlock {
                    EngineError::FileSystemError("Cookies database is locked by a running browser instance.".to_string())
                } else {
                    EngineError::FileSystemError(e.to_string())
                }
            })?;

        #[cfg(target_os = "windows")]
        return self.extract_windows(&db_path);

        #[cfg(target_os = "macos")]
        return self.extract_macos(&db_path);

        #[cfg(target_os = "linux")]
        return self.extract_linux(&db_path);

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        return Err(EngineError::OsApiError("Unsupported OS for cookie extraction".to_string()));
    }

    #[cfg(target_os = "windows")]
    fn extract_windows(&self, _db_path: &Path) -> Result<Bytes, EngineError> {
        use std::ptr;
        use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPTOAPI_BLOB};
        use windows_sys::Win32::System::Memory::LocalFree;

        let local_state_path = self.browser_path.join("Local State");
        let _local_state_content = std::fs::read_to_string(&local_state_path)
            .map_err(|e| EngineError::FileSystemError(e.to_string()))?;

        // Example JSON parsing logic would normally extract `os_crypt.encrypted_key` and base64 decode it here.
        let key_bytes: Vec<u8> = vec![]; 

        // Strip the "DPAPI" version string prefix universally used by Chromium instances
        let dpapi_key = if key_bytes.len() > 5 { &key_bytes[5..] } else { &key_bytes };

        let mut data_in = CRYPTOAPI_BLOB {
            cbData: dpapi_key.len() as u32,
            pbData: dpapi_key.as_ptr() as *mut u8,
        };
        let mut data_out = CRYPTOAPI_BLOB { cbData: 0, pbData: ptr::null_mut() };

        let success = unsafe {
            CryptUnprotectData(
                &mut data_in,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                &mut data_out,
            )
        };

        if success == 0 {
            return Err(EngineError::DecryptionError("Windows DPAPI decryption failed".into()));
        }

        let decrypted_key = unsafe { std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize) }.to_vec();
        
        // Safely free the OS-allocated memory
        unsafe {
            LocalFree(data_out.pbData as _);
        }

        Ok(Bytes::from(decrypted_key))
    }

    #[cfg(target_os = "macos")]
    fn extract_macos(&self, _db_path: &Path) -> Result<Bytes, EngineError> {
        use security_framework::passwords::get_generic_password;

        let service = "Chrome Safe Storage";
        let account = "Chrome";

        let password = get_generic_password(service, account)
            .map_err(|e| EngineError::DecryptionError(format!("Keychain decryption failed: {}", e)))?;

        Ok(Bytes::from(password.to_vec()))
    }

    #[cfg(target_os = "linux")]
    fn extract_linux(&self, _db_path: &Path) -> Result<Bytes, EngineError> {
        use secret_service::{EncryptionType, SecretService};

        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| EngineError::OsApiError("No tokio runtime found".into()))?;

        handle.block_on(async {
            let ss = SecretService::connect(EncryptionType::Dh).await
                .map_err(|e| EngineError::DecryptionError(format!("SecretService connection failed: {}", e)))?;

            let collection = ss.get_default_collection().await
                .map_err(|e| EngineError::DecryptionError(format!("Failed to get default collection: {}", e)))?;

            let search = collection.search_items(std::collections::HashMap::from([("application", "chrome")])).await;
            let items = search.map_err(|e| EngineError::DecryptionError(format!("Search failed: {}", e)))?;

            if let Some(item) = items.first() {
                let secret = item.get_secret().await
                    .map_err(|e| EngineError::DecryptionError(format!("Failed to get secret: {}", e)))?;
                Ok(Bytes::from(secret))
            } else {
                Err(EngineError::DecryptionError("No Chrome secret found in keyring".into()))
            }
        })
    }
}