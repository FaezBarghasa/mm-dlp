use crate::client::EngineError;
use bytes::Bytes;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieItem {
    pub domain: String,
    pub include_subdomains: bool,
    pub path: String,
    pub secure: bool,
    pub expires: i64,
    pub name: String,
    pub value: String,
    pub http_only: bool,
}

impl CookieItem {
    pub fn to_netscape_line(&self) -> String {
        let subdomains_str = if self.include_subdomains || self.domain.starts_with('.') {
            "TRUE"
        } else {
            "FALSE"
        };
        let secure_str = if self.secure { "TRUE" } else { "FALSE" };
        let domain_clean = if self.include_subdomains && !self.domain.starts_with('.') {
            format!(".{}", self.domain)
        } else {
            self.domain.clone()
        };
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            domain_clean,
            subdomains_str,
            if self.path.is_empty() {
                "/"
            } else {
                &self.path
            },
            secure_str,
            self.expires,
            self.name,
            self.value
        )
    }
}

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
                if e.kind() == std::io::ErrorKind::PermissionDenied
                    || e.kind() == std::io::ErrorKind::WouldBlock
                {
                    EngineError::FileSystemError(
                        "Cookies database is locked by a running browser instance.".to_string(),
                    )
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

        #[cfg(target_os = "ios")]
        return Err(EngineError::OsApiError(
            "Direct browser cookie extraction is forbidden on iOS due to app sandbox isolation. Inject cookies via Netscape format or HTTP headers instead.".to_string(),
        ));

        #[cfg(not(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "ios"
        )))]
        return Err(EngineError::OsApiError(
            "Unsupported OS for cookie extraction".to_string(),
        ));
    }

    /// Parse standard Netscape format cookie files, universal across all platforms including iOS.
    pub fn parse_netscape_cookies(content: &str) -> Vec<(String, String)> {
        let mut result = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 7 {
                let name = parts[5].to_string();
                let value = parts[6].to_string();
                result.push((name, value));
            }
        }
        result
    }

    /// Parses Netscape cookie content into structured CookieItem elements.
    pub fn parse_netscape_cookie_items(content: &str) -> Vec<CookieItem> {
        let mut result = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 7 {
                let domain = parts[0].to_string();
                let subdomains = parts[1].eq_ignore_ascii_case("TRUE");
                let path = parts[2].to_string();
                let secure = parts[3].eq_ignore_ascii_case("TRUE");
                let expires = parts[4].parse::<i64>().unwrap_or(0);
                let name = parts[5].to_string();
                let value = parts[6].to_string();
                result.push(CookieItem {
                    domain,
                    include_subdomains: subdomains,
                    path,
                    secure,
                    expires,
                    name,
                    value,
                    http_only: false,
                });
            }
        }
        result
    }

    #[cfg(target_os = "windows")]
    fn extract_windows(&self, _db_path: &Path) -> Result<Bytes, EngineError> {
        use std::ptr;
        use windows_sys::Win32::Security::Cryptography::{CRYPTOAPI_BLOB, CryptUnprotectData};
        use windows_sys::Win32::System::Memory::LocalFree;

        let local_state_path = self.browser_path.join("Local State");
        let _local_state_content = std::fs::read_to_string(&local_state_path)
            .map_err(|e| EngineError::FileSystemError(e.to_string()))?;

        // Example JSON parsing logic would normally extract `os_crypt.encrypted_key` and base64 decode it here.
        let key_bytes: Vec<u8> = vec![];

        // Strip the "DPAPI" version string prefix universally used by Chromium instances
        let dpapi_key = if key_bytes.len() > 5 {
            &key_bytes[5..]
        } else {
            &key_bytes
        };

        let mut data_in = CRYPTOAPI_BLOB {
            cbData: dpapi_key.len() as u32,
            pbData: dpapi_key.as_ptr() as *mut u8,
        };
        let mut data_out = CRYPTOAPI_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

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
            return Err(EngineError::DecryptionError(
                "Windows DPAPI decryption failed".into(),
            ));
        }

        let decrypted_key =
            unsafe { std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize) }
                .to_vec();

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

        let password = get_generic_password(service, account).map_err(|e| {
            EngineError::DecryptionError(format!("Keychain decryption failed: {}", e))
        })?;

        Ok(Bytes::from(password.to_vec()))
    }

    #[cfg(target_os = "linux")]
    fn extract_linux(&self, _db_path: &Path) -> Result<Bytes, EngineError> {
        use secret_service::{EncryptionType, SecretService};

        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| EngineError::OsApiError("No tokio runtime found".into()))?;

        handle.block_on(async {
            let ss = SecretService::connect(EncryptionType::Dh)
                .await
                .map_err(|e| {
                    EngineError::DecryptionError(format!("SecretService connection failed: {}", e))
                })?;

            let collection = ss.get_default_collection().await.map_err(|e| {
                EngineError::DecryptionError(format!("Failed to get default collection: {}", e))
            })?;

            let search = collection
                .search_items(std::collections::HashMap::from([("application", "chrome")]))
                .await;
            let items = search
                .map_err(|e| EngineError::DecryptionError(format!("Search failed: {}", e)))?;

            if let Some(item) = items.first() {
                let secret = item.get_secret().await.map_err(|e| {
                    EngineError::DecryptionError(format!("Failed to get secret: {}", e))
                })?;
                Ok(Bytes::from(secret))
            } else {
                Err(EngineError::DecryptionError(
                    "No Chrome secret found in keyring".into(),
                ))
            }
        })
    }
}

pub struct FirefoxCookieExtractor;

impl FirefoxCookieExtractor {
    pub fn candidate_profile_roots() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            let home = PathBuf::from(home);
            #[cfg(target_os = "linux")]
            {
                dirs.push(home.join(".mozilla/firefox"));
                dirs.push(home.join("snap/firefox/common/.mozilla/firefox"));
                dirs.push(home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"));
                dirs.push(home.join(".librewolf"));
                dirs.push(home.join(".waterfox"));
            }
            #[cfg(target_os = "macos")]
            {
                dirs.push(home.join("Library/Application Support/Firefox/Profiles"));
                dirs.push(home.join("Library/Application Support/Firefox"));
                dirs.push(home.join("Library/Application Support/LibreWolf/Profiles"));
                dirs.push(home.join("Library/Application Support/Waterfox/Profiles"));
            }
        }
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let appdata = PathBuf::from(appdata);
                dirs.push(appdata.join("Mozilla/Firefox/Profiles"));
                dirs.push(appdata.join("Mozilla/Firefox"));
                dirs.push(appdata.join("LibreWolf/Profiles"));
                dirs.push(appdata.join("Waterfox/Profiles"));
            }
        }
        dirs
    }

    pub fn resolve_profile_path(spec: Option<&str>) -> Result<PathBuf, EngineError> {
        if let Some(s) = spec {
            let p = PathBuf::from(s);
            if p.is_file() && p.file_name().is_some_and(|n| n == "cookies.sqlite") {
                return Ok(p.parent().unwrap_or(Path::new(".")).to_path_buf());
            }
            if p.is_dir() && p.join("cookies.sqlite").exists() {
                return Ok(p);
            }
        }

        let roots = Self::candidate_profile_roots();
        for root in &roots {
            if !root.exists() {
                continue;
            }

            let ini_path = if root.join("profiles.ini").exists() {
                root.join("profiles.ini")
            } else if root
                .parent()
                .is_some_and(|p| p.join("profiles.ini").exists())
            {
                root.parent().unwrap().join("profiles.ini")
            } else {
                PathBuf::new()
            };

            if ini_path.exists()
                && let Ok(ini_content) = std::fs::read_to_string(&ini_path)
                && let Some(profile_dir) = Self::parse_profiles_ini(&ini_content, root, spec)
                && profile_dir.join("cookies.sqlite").exists()
            {
                return Ok(profile_dir);
            }

            let scan_dirs = vec![root.clone(), root.join("Profiles")];
            let mut candidates = Vec::new();
            for dir in scan_dirs {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            let sqlite = path.join("cookies.sqlite");
                            if sqlite.exists() {
                                if let Some(req_name) = spec
                                    && path.to_string_lossy().contains(req_name)
                                {
                                    return Ok(path);
                                }
                                let mtime = entry.metadata().and_then(|m| m.modified()).ok();
                                candidates.push((path, mtime));
                            }
                        }
                    }
                }
            }

            if !candidates.is_empty() {
                candidates.sort_by_key(|a| std::cmp::Reverse(a.1));
                return Ok(candidates[0].0.clone());
            }
        }

        Err(EngineError::FileSystemError(
            "No Firefox / Gecko profile with cookies.sqlite found.".to_string(),
        ))
    }

    pub fn parse_profiles_ini(
        ini_content: &str,
        base_dir: &Path,
        spec: Option<&str>,
    ) -> Option<PathBuf> {
        let mut default_path = None;
        let mut matched_path = None;
        let mut current_path: Option<String> = None;
        let mut current_name: Option<String> = None;
        let mut current_is_relative = true;
        let mut current_is_default = false;

        let commit_profile = |current_path: &Option<String>,
                              current_name: &Option<String>,
                              current_is_relative: bool,
                              current_is_default: bool,
                              spec: Option<&str>,
                              default_path: &mut Option<PathBuf>,
                              matched_path: &mut Option<PathBuf>| {
            if let Some(rel_or_abs) = current_path {
                let resolved = if current_is_relative {
                    base_dir.join(rel_or_abs)
                } else {
                    PathBuf::from(rel_or_abs)
                };

                if let Some(s) = spec {
                    if let Some(name) = current_name
                        && name.eq_ignore_ascii_case(s)
                    {
                        *matched_path = Some(resolved.clone());
                    }
                    if rel_or_abs.contains(s) {
                        *matched_path = Some(resolved.clone());
                    }
                }

                if current_is_default && default_path.is_none() {
                    *default_path = Some(resolved);
                }
            }
        };

        for line in ini_content.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                commit_profile(
                    &current_path,
                    &current_name,
                    current_is_relative,
                    current_is_default,
                    spec,
                    &mut default_path,
                    &mut matched_path,
                );
                current_path = None;
                current_name = None;
                current_is_relative = true;
                current_is_default = false;
            } else if let Some((k, v)) = line.split_once('=') {
                let key = k.trim();
                let val = v.trim();
                if key.eq_ignore_ascii_case("Path") {
                    current_path = Some(val.to_string());
                } else if key.eq_ignore_ascii_case("Name") {
                    current_name = Some(val.to_string());
                } else if key.eq_ignore_ascii_case("IsRelative") {
                    current_is_relative = val == "1";
                } else if key.eq_ignore_ascii_case("Default") {
                    current_is_default = val == "1";
                }
            }
        }

        commit_profile(
            &current_path,
            &current_name,
            current_is_relative,
            current_is_default,
            spec,
            &mut default_path,
            &mut matched_path,
        );

        matched_path.or(default_path)
    }

    pub fn extract_from_sqlite(sqlite_path: &Path) -> Result<Vec<CookieItem>, EngineError> {
        if !sqlite_path.exists() {
            return Err(EngineError::FileSystemError(format!(
                "SQLite cookie database not found at {}",
                sqlite_path.display()
            )));
        }

        let temp_dir = std::env::temp_dir();
        let unique_id = format!(
            "{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let temp_db = temp_dir.join(format!("mm_dlp_gecko_{}.sqlite", unique_id));
        let temp_wal = temp_dir.join(format!("mm_dlp_gecko_{}.sqlite-wal", unique_id));
        let temp_shm = temp_dir.join(format!("mm_dlp_gecko_{}.sqlite-shm", unique_id));

        std::fs::copy(sqlite_path, &temp_db).map_err(|e| {
            EngineError::FileSystemError(format!("Failed to copy cookies.sqlite: {}", e))
        })?;

        let orig_wal = sqlite_path.with_file_name("cookies.sqlite-wal");
        if orig_wal.exists() {
            let _ = std::fs::copy(&orig_wal, &temp_wal);
        }
        let orig_shm = sqlite_path.with_file_name("cookies.sqlite-shm");
        if orig_shm.exists() {
            let _ = std::fs::copy(&orig_shm, &temp_shm);
        }

        let query_result = (|| -> Result<Vec<CookieItem>, rusqlite::Error> {
            let conn = rusqlite::Connection::open_with_flags(
                &temp_db,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
            )?;
            conn.busy_timeout(std::time::Duration::from_secs(3))?;

            let mut stmt = conn.prepare(
                "SELECT host, name, value, path, expiry, isSecure, isHttpOnly FROM moz_cookies",
            )?;

            let rows = stmt.query_map([], |row| {
                let host: String = row.get(0)?;
                let name: String = row.get(1)?;
                let value: String = row.get(2)?;
                let path: String = row.get(3)?;
                let expiry: i64 = row.get(4)?;
                let is_secure: i64 = row.get(5)?;
                let is_http_only: i64 = row.get(6)?;

                Ok(CookieItem {
                    include_subdomains: host.starts_with('.'),
                    domain: host,
                    name,
                    value,
                    path,
                    expires: expiry,
                    secure: is_secure != 0,
                    http_only: is_http_only != 0,
                })
            })?;

            let mut items = Vec::new();
            for item in rows.flatten() {
                items.push(item);
            }
            Ok(items)
        })();

        let _ = std::fs::remove_file(&temp_db);
        let _ = std::fs::remove_file(&temp_wal);
        let _ = std::fs::remove_file(&temp_shm);

        let items = query_result.map_err(|e| {
            EngineError::DecryptionError(format!("Failed to query Firefox moz_cookies: {}", e))
        })?;

        Ok(items)
    }

    pub fn extract_to_netscape(spec: Option<&str>) -> Result<String, EngineError> {
        let profile_dir = Self::resolve_profile_path(spec)?;
        let sqlite_path = profile_dir.join("cookies.sqlite");
        let items = Self::extract_from_sqlite(&sqlite_path)?;

        let mut out = String::from(
            "# Netscape HTTP Cookie File\n# http://curl.haxx.se/rfc/cookie_spec.html\n# Extracted by mm-dlp (Firefox/Gecko)\n\n",
        );
        for item in &items {
            out.push_str(&item.to_netscape_line());
            out.push('\n');
        }
        Ok(out)
    }
}

pub struct SafariCookieExtractor;

impl SafariCookieExtractor {
    pub fn candidate_cookie_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            let home = PathBuf::from(home);
            // macOS standard path
            paths.push(home.join("Library/Cookies/Cookies.binarycookies"));
            // macOS sandboxed Safari container path
            paths.push(home.join(
                "Library/Containers/com.apple.Safari/Data/Library/Cookies/Cookies.binarycookies",
            ));
        }
        // iOS paths (standard container or jailbroken root)
        paths.push(PathBuf::from(
            "/private/var/mobile/Library/Cookies/Cookies.binarycookies",
        ));
        paths.push(PathBuf::from(
            "/var/mobile/Library/Cookies/Cookies.binarycookies",
        ));
        paths
    }

    pub fn resolve_cookie_path(spec: Option<&str>) -> Result<PathBuf, EngineError> {
        if let Some(s) = spec {
            let p = PathBuf::from(s);
            if p.is_file() {
                return Ok(p);
            }
            if p.is_dir() {
                let candidate = p.join("Cookies.binarycookies");
                if candidate.exists() {
                    return Ok(candidate);
                }
                let candidate_lc = p.join("cookies.binarycookies");
                if candidate_lc.exists() {
                    return Ok(candidate_lc);
                }
            }
        }

        for path in Self::candidate_cookie_paths() {
            if path.exists() {
                return Ok(path);
            }
        }

        Err(EngineError::FileSystemError(
            "No Safari Cookies.binarycookies found. Ensure Safari has cookies saved or pass direct path via --cookies-from-browser safari:/path/to/Cookies.binarycookies.".to_string(),
        ))
    }

    /// Parses Apple Safari / WebKit proprietary Cookies.binarycookies binary data format.
    ///
    /// Layout:
    /// - 4 bytes: magic `cook`
    /// - 4 bytes: number of pages (big-endian u32)
    /// - num_pages * 4 bytes: page sizes (big-endian u32)
    /// - Pages sequentially:
    ///   - 4 bytes: page header tag (0x00000100)
    ///   - 4 bytes: number of cookies (little-endian u32)
    ///   - num_cookies * 4 bytes: cookie offsets relative to start of page (little-endian u32)
    ///   - 4 bytes: page end tag (0x00000000)
    ///   - Cookie records at each offset:
    ///     - 0..4: size (u32 LE)
    ///     - 4..8: unknown1 (u32 LE)
    ///     - 8..12: flags (u32 LE, bit 0 = secure, bit 2 = http_only)
    ///     - 12..16: unknown2 (u32 LE)
    ///     - 16..20: domain/url offset (u32 LE)
    ///     - 20..24: name offset (u32 LE)
    ///     - 24..28: path offset (u32 LE)
    ///     - 28..32: value offset (u32 LE)
    ///     - 32..40: comment offset / padding
    ///     - 40..48: expiry timestamp (f64 LE, Mac epoch offset: +978307200 seconds)
    ///     - 48..56: creation timestamp (f64 LE)
    ///     - Null-terminated UTF-8 strings at respective offsets
    pub fn parse_binarycookies(data: &[u8]) -> Result<Vec<CookieItem>, EngineError> {
        if data.len() < 8 {
            return Err(EngineError::FileSystemError(
                "Binarycookies file is too short (< 8 bytes)".to_string(),
            ));
        }
        if &data[0..4] != b"cook" {
            return Err(EngineError::FileSystemError(
                "Invalid binarycookies magic header (expected 'cook')".to_string(),
            ));
        }

        let num_pages = u32::from_be_bytes(
            data[4..8]
                .try_into()
                .map_err(|_| EngineError::FileSystemError("Corrupted page count".to_string()))?,
        ) as usize;

        let header_size = 8 + num_pages * 4;
        if data.len() < header_size {
            return Err(EngineError::FileSystemError(
                "Binarycookies truncated header page sizes table".to_string(),
            ));
        }

        let mut page_sizes = Vec::with_capacity(num_pages);
        for i in 0..num_pages {
            let offset = 8 + i * 4;
            let p_size = u32::from_be_bytes(
                data[offset..offset + 4]
                    .try_into()
                    .map_err(|_| EngineError::FileSystemError("Corrupted page size".to_string()))?,
            ) as usize;
            page_sizes.push(p_size);
        }

        let mut current_offset = header_size;
        let mut all_cookies = Vec::new();

        for page_size in page_sizes {
            if current_offset + page_size > data.len() {
                return Err(EngineError::FileSystemError(
                    "Binarycookies page size exceeds data length".to_string(),
                ));
            }
            let page_data = &data[current_offset..current_offset + page_size];
            current_offset += page_size;

            if page_data.len() < 8 {
                continue;
            }

            // Page tag: 0x00000100 (in BE: [0x00, 0x00, 0x01, 0x00], or in LE: [0x00, 0x01, 0x00, 0x00])
            if page_data[0..4] != [0x00, 0x00, 0x01, 0x00]
                && page_data[0..4] != [0x00, 0x01, 0x00, 0x00]
            {
                continue;
            }

            let num_cookies =
                u32::from_le_bytes(page_data[4..8].try_into().map_err(|_| {
                    EngineError::FileSystemError("Corrupted cookie count".to_string())
                })?) as usize;

            if page_data.len() < 8 + num_cookies * 4 {
                continue;
            }

            let mut cookie_offsets = Vec::with_capacity(num_cookies);
            for i in 0..num_cookies {
                let off = 8 + i * 4;
                let c_offset =
                    u32::from_le_bytes(page_data[off..off + 4].try_into().map_err(|_| {
                        EngineError::FileSystemError("Corrupted cookie offset".to_string())
                    })?) as usize;
                cookie_offsets.push(c_offset);
            }

            for c_offset in cookie_offsets {
                if c_offset + 48 > page_data.len() {
                    continue;
                }
                let cookie_record = &page_data[c_offset..];
                let cookie_size =
                    u32::from_le_bytes(cookie_record[0..4].try_into().map_err(|_| {
                        EngineError::FileSystemError("Corrupted cookie size".to_string())
                    })?) as usize;

                if cookie_size < 48 || cookie_record.len() < cookie_size {
                    continue;
                }

                let flags = u32::from_le_bytes(cookie_record[8..12].try_into().unwrap_or([0; 4]));
                let secure = (flags & 1) != 0;
                let http_only = (flags & 4) != 0;

                let url_offset =
                    u32::from_le_bytes(cookie_record[16..20].try_into().unwrap_or([0; 4])) as usize;
                let name_offset =
                    u32::from_le_bytes(cookie_record[20..24].try_into().unwrap_or([0; 4])) as usize;
                let path_offset =
                    u32::from_le_bytes(cookie_record[24..28].try_into().unwrap_or([0; 4])) as usize;
                let value_offset =
                    u32::from_le_bytes(cookie_record[28..32].try_into().unwrap_or([0; 4])) as usize;

                let mac_expiry =
                    f64::from_le_bytes(cookie_record[40..48].try_into().unwrap_or([0; 8]));
                let unix_expiry = if mac_expiry > 0.0 {
                    (mac_expiry + 978_307_200.0) as i64
                } else {
                    0
                };

                let read_cstring = |offset: usize| -> Option<String> {
                    if offset >= cookie_size {
                        return None;
                    }
                    let slice = &cookie_record[offset..cookie_size];
                    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
                    std::str::from_utf8(&slice[..end])
                        .ok()
                        .map(|s| s.to_string())
                };

                let domain = read_cstring(url_offset).unwrap_or_default();
                let name = read_cstring(name_offset).unwrap_or_default();
                let path = read_cstring(path_offset).unwrap_or_else(|| "/".to_string());
                let value = read_cstring(value_offset).unwrap_or_default();

                if !name.is_empty() && !domain.is_empty() {
                    let include_subdomains = domain.starts_with('.');
                    all_cookies.push(CookieItem {
                        include_subdomains,
                        domain,
                        path,
                        secure,
                        expires: unix_expiry,
                        name,
                        value,
                        http_only,
                    });
                }
            }
        }

        Ok(all_cookies)
    }

    pub fn extract_from_file(path: &Path) -> Result<Vec<CookieItem>, EngineError> {
        let data = std::fs::read(path).map_err(|e| {
            EngineError::FileSystemError(format!(
                "Failed to read Safari binarycookies file '{}': {}",
                path.display(),
                e
            ))
        })?;
        Self::parse_binarycookies(&data)
    }

    pub fn extract_to_netscape(spec: Option<&str>) -> Result<String, EngineError> {
        let path = Self::resolve_cookie_path(spec)?;
        let items = Self::extract_from_file(&path)?;

        let mut out = String::from(
            "# Netscape HTTP Cookie File\n# http://curl.haxx.se/rfc/cookie_spec.html\n# Extracted by mm-dlp (Safari/WebKit/CFNetwork)\n\n",
        );
        for item in &items {
            out.push_str(&item.to_netscape_line());
            out.push('\n');
        }
        Ok(out)
    }
}

pub struct BrowserCookieExtractor;

impl BrowserCookieExtractor {
    pub fn extract(browser_spec: &str) -> Result<String, EngineError> {
        let (browser, profile) = match browser_spec.split_once(':') {
            Some((b, p)) => (b.trim().to_lowercase(), Some(p.trim())),
            None => (browser_spec.trim().to_lowercase(), None),
        };

        match browser.as_str() {
            "firefox" | "gecko" | "librewolf" | "waterfox" | "torbrowser" => {
                FirefoxCookieExtractor::extract_to_netscape(profile)
            }
            "safari" | "apple" | "webkit" => SafariCookieExtractor::extract_to_netscape(profile),
            "chrome" | "chromium" | "brave" | "edge" => Err(EngineError::FileSystemError(format!(
                "Direct cookie extraction for '{}' requires browser profile path.",
                browser
            ))),
            other => Err(EngineError::FileSystemError(format!(
                "Unsupported browser '{}' for cookie extraction. Supported: firefox (gecko), safari (webkit/apple), librewolf, waterfox, chrome, chromium, brave, edge",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_netscape_cookies() {
        let sample = "\
# Netscape HTTP Cookie File
# http://curl.haxx.se/rfc/cookie_spec.html
.youtube.com\tTRUE\t/\tTRUE\t1750000000\tSID\tsample_sid_value
.youtube.com\tTRUE\t/\tTRUE\t1750000000\tHSID\tsample_hsid_value
";
        let parsed = CookieExtractor::parse_netscape_cookies(sample);
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0],
            ("SID".to_string(), "sample_sid_value".to_string())
        );
        assert_eq!(
            parsed[1],
            ("HSID".to_string(), "sample_hsid_value".to_string())
        );

        let items = CookieExtractor::parse_netscape_cookie_items(sample);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].domain, ".youtube.com");
        assert_eq!(items[0].name, "SID");
        assert_eq!(items[0].value, "sample_sid_value");
        assert!(items[0].secure);
        assert!(items[0].include_subdomains);
    }

    #[test]
    fn test_parse_profiles_ini() {
        let ini = "\
[Profile1]
Name=work
IsRelative=1
Path=Profiles/work.profile
Default=0

[Profile0]
Name=default-release
IsRelative=1
Path=Profiles/default.profile
Default=1
";
        let base = Path::new("/home/user/.mozilla/firefox");
        let def = FirefoxCookieExtractor::parse_profiles_ini(ini, base, None).unwrap();
        assert_eq!(def, base.join("Profiles/default.profile"));

        let named = FirefoxCookieExtractor::parse_profiles_ini(ini, base, Some("work")).unwrap();
        assert_eq!(named, base.join("Profiles/work.profile"));
    }

    #[test]
    fn test_firefox_cookie_sqlite_extraction() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join(format!("test_moz_cookies_{}.sqlite", std::process::id()));

        // Create mock Firefox cookies.sqlite database
        let conn = rusqlite::Connection::open(&db_path).expect("Failed to create test db");
        conn.execute_batch(
            "CREATE TABLE moz_cookies (
                id INTEGER PRIMARY KEY,
                originAttributes TEXT,
                name TEXT,
                value TEXT,
                host TEXT,
                path TEXT,
                expiry INTEGER,
                lastAccessed INTEGER,
                creationTime INTEGER,
                isSecure INTEGER,
                isHttpOnly INTEGER,
                inBrowserElement INTEGER,
                sameSite INTEGER,
                rawSameSite INTEGER,
                schemeMap INTEGER
            );
            INSERT INTO moz_cookies (name, value, host, path, expiry, isSecure, isHttpOnly)
            VALUES ('PREF', 'f1=50000000', '.youtube.com', '/', 1800000000, 1, 0);
            INSERT INTO moz_cookies (name, value, host, path, expiry, isSecure, isHttpOnly)
            VALUES ('SSID', 'gecko_auth_token', '.youtube.com', '/', 1800000000, 1, 1);
            ",
        )
        .expect("Failed to populate test moz_cookies");
        drop(conn);

        let extracted = FirefoxCookieExtractor::extract_from_sqlite(&db_path)
            .expect("Failed to extract sqlite cookies");
        assert_eq!(extracted.len(), 2);
        assert_eq!(extracted[0].name, "PREF");
        assert_eq!(extracted[0].value, "f1=50000000");
        assert_eq!(extracted[0].domain, ".youtube.com");
        assert!(extracted[0].secure);

        assert_eq!(extracted[1].name, "SSID");
        assert_eq!(extracted[1].value, "gecko_auth_token");
        assert!(extracted[1].http_only);

        let netscape = extracted[0].to_netscape_line();
        assert!(netscape.contains(".youtube.com\tTRUE\t/\tTRUE\t1800000000\tPREF\tf1=50000000"));

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_safari_binarycookies_parsing() {
        let domain = ".apple.com\0";
        let name = "my_token\0";
        let path = "/account\0";
        let value = "secret_safari_val\0";

        // Cookie record header = 56 bytes (including 8 bytes creation date)
        let url_offset = 56u32;
        let name_offset = url_offset + domain.len() as u32;
        let path_offset = name_offset + name.len() as u32;
        let value_offset = path_offset + path.len() as u32;
        let cookie_size = value_offset + value.len() as u32;

        let mut cookie_record = Vec::new();
        cookie_record.extend_from_slice(&cookie_size.to_le_bytes()); // 0..4 size
        cookie_record.extend_from_slice(&[0u8; 4]); // 4..8 unknown1
        cookie_record.extend_from_slice(&5u32.to_le_bytes()); // 8..12 flags (1 = secure, 4 = http_only)
        cookie_record.extend_from_slice(&[0u8; 4]); // 12..16 unknown2
        cookie_record.extend_from_slice(&url_offset.to_le_bytes()); // 16..20 url_offset
        cookie_record.extend_from_slice(&name_offset.to_le_bytes()); // 20..24 name_offset
        cookie_record.extend_from_slice(&path_offset.to_le_bytes()); // 24..28 path_offset
        cookie_record.extend_from_slice(&value_offset.to_le_bytes()); // 28..32 value_offset
        cookie_record.extend_from_slice(&[0u8; 8]); // 32..40 comment/reserved
        // mac_expiry = 821692800.0 (which + 978307200 = 1800000000)
        let mac_expiry: f64 = 821_692_800.0;
        cookie_record.extend_from_slice(&mac_expiry.to_le_bytes()); // 40..48 expiry
        let mac_creation: f64 = 800_000_000.0;
        cookie_record.extend_from_slice(&mac_creation.to_le_bytes()); // 48..56 creation
        cookie_record.extend_from_slice(domain.as_bytes());
        cookie_record.extend_from_slice(name.as_bytes());
        cookie_record.extend_from_slice(path.as_bytes());
        cookie_record.extend_from_slice(value.as_bytes());

        // Page header = 4 (tag) + 4 (num_cookies) + 4 (cookie_offset[0]) + 4 (page end tag) = 16 bytes
        let cookie_offset_in_page = 16u32;
        let page_size = cookie_offset_in_page + cookie_record.len() as u32;

        let mut page = Vec::new();
        page.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]); // page tag
        page.extend_from_slice(&1u32.to_le_bytes()); // 1 cookie
        page.extend_from_slice(&cookie_offset_in_page.to_le_bytes()); // offset 0
        page.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // page end
        page.extend_from_slice(&cookie_record);

        // File header = 4 (cook) + 4 (num_pages) + 4 (page_size[0]) = 12 bytes
        let mut file_bytes = Vec::new();
        file_bytes.extend_from_slice(b"cook");
        file_bytes.extend_from_slice(&1u32.to_be_bytes()); // 1 page
        file_bytes.extend_from_slice(&page_size.to_be_bytes()); // page size
        file_bytes.extend_from_slice(&page);

        let parsed = SafariCookieExtractor::parse_binarycookies(&file_bytes).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].domain, ".apple.com");
        assert_eq!(parsed[0].name, "my_token");
        assert_eq!(parsed[0].path, "/account");
        assert_eq!(parsed[0].value, "secret_safari_val");
        assert_eq!(parsed[0].expires, 1_800_000_000);
        assert!(parsed[0].secure);
        assert!(parsed[0].http_only);
        assert!(parsed[0].include_subdomains);

        let netscape = parsed[0].to_netscape_line();
        assert_eq!(
            netscape,
            ".apple.com\tTRUE\t/account\tTRUE\t1800000000\tmy_token\tsecret_safari_val"
        );
    }
}
