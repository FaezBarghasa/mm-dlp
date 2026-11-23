use crate::error::EngineError;
use crate::js::engine::SandboxJsEngine;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// Safely extracts a balanced `{ ... }` block starting at `open_brace_pos`.
fn extract_brace_block(input: &str, open_brace_pos: usize) -> Option<&str> {
    let bytes = input.as_bytes();
    if open_brace_pos >= bytes.len() || bytes[open_brace_pos] != b'{' {
        return None;
    }
    let mut depth = 0;
    let mut in_str = None;
    let mut escaped = false;

    for (i, &b) in bytes[open_brace_pos..].iter().enumerate() {
        let abs_pos = open_brace_pos + i;
        if escaped {
            escaped = false;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            continue;
        }
        if let Some(quote) = in_str {
            if b == quote {
                in_str = None;
            }
        } else {
            match b {
                b'"' | b'\'' | b'`' => in_str = Some(b),
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&input[open_brace_pos..=abs_pos]);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

pub struct JsDecipher {
    engine: SandboxJsEngine,
    n_cache: Mutex<HashMap<String, String>>,
}

impl JsDecipher {
    pub fn new() -> Result<Self, EngineError> {
        let engine = SandboxJsEngine::new()?;
        Ok(Self {
            engine,
            n_cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn decipher(&self, js_payload: &str, signature: &str) -> Result<String, EngineError> {
        // Regex patterns to identify cipher swap/reverse blocks inside dynamically scrambled payloads.
        // Matches common obfuscated function signatures: `decipher = function(a) { var b=a.split(""); ... return b.join("") }`
        let main_func_regex = Regex::new(r#"(?x)
            (?:var\s+)?(?P<func_name>[a-zA-Z0-9$]+)\s*=\s*function\(\s*(?P<param>[a-zA-Z0-9$]+)\s*\)\s*\{
                \s*(?:var\s+)?(?P<param2>[a-zA-Z0-9$]+)\s*=\s*[a-zA-Z0-9$]+\.split\(""\);\s*
                (?P<body>[\s\S]+?)\s*
                return\s+[a-zA-Z0-9$]+\.join\(""\);?\s*
            \}
        "#).map_err(|e| EngineError::StreamNotFound(e.to_string()))?;

        let captures = main_func_regex.captures(js_payload).ok_or_else(|| {
            EngineError::StreamNotFound(
                "Failed to find decipher function block in JS payload".to_string(),
            )
        })?;

        let func_name = captures
            .name("func_name")
            .map(|m| m.as_str())
            .unwrap_or("decipherFn");
        let param = captures.name("param").map(|m| m.as_str()).unwrap_or("a");
        let param2 = captures.name("param2").map(|m| m.as_str()).unwrap_or("a");
        let body = captures.name("body").map(|m| m.as_str()).unwrap_or("");

        // Identify the localized helper object being used inside the body for reversing & swapping
        let helper_obj_regex = Regex::new(r#"([a-zA-Z0-9$]+)\.[a-zA-Z0-9$]+\("#)
            .map_err(|e| EngineError::StreamNotFound(e.to_string()))?;

        let helper_obj_name = if let Some(cap) = helper_obj_regex.captures(body) {
            cap.get(1).map(|m| m.as_str()).unwrap_or("helperObj")
        } else {
            return Err(EngineError::StreamNotFound(
                "Failed to find bound helper object in decipher function body".to_string(),
            ));
        };

        // Extract helper object definitions
        let obj_def_regex = Regex::new(&format!(
            r#"(?x)
            (?:var\s+)?{}\s*=\s*\{{
                [\s\S]*?
            \}};\s*
        "#,
            regex::escape(helper_obj_name)
        ))
        .map_err(|e| EngineError::StreamNotFound(e.to_string()))?;

        let obj_def = obj_def_regex
            .find(js_payload)
            .map(|m| m.as_str())
            .unwrap_or("");

        // Reconstruct the script block containing strictly necessary cipher assets
        let compiled_script = format!(
            "{}\nvar {} = function({}) {{\n var {}={}.split(\"\");\n {}\n return {}.join(\"\");\n}};",
            obj_def, func_name, param, param2, param, body, param2
        );

        // Load the isolated script chunks directly into our compiled QuickJS Sandbox runtime
        self.engine
            .execute_decipher(&compiled_script, signature, func_name)
    }

    /// Deciphers YouTube's `n`-challenge token to prevent bandwidth throttling (~40 KB/s trap) and 403 Forbidden errors.
    pub fn decipher_n(&self, js_payload: &str, n_token: &str) -> Result<String, EngineError> {
        if n_token.is_empty() {
            return Ok(String::new());
        }

        // Fast-path: check memoization cache
        if let Ok(guard) = self.n_cache.lock()
            && let Some(cached) = guard.get(n_token)
        {
            return Ok(cached.clone());
        }

        // Pattern 1: .get("n") entry point patterns in YouTube base.js
        let n_entry_regexes = [
            // Matches: .get("n") && (b = fnName(b)) or .get("n") && (b = fnName[0](b))
            Regex::new(r#"(?x)(?:\.get\("n"\)|[\w$]+\s*=\s*[\w$]+\.get\("n"\))\s*&&\s*(?:\(?\s*[\w$]+\s*=\s*)?([a-zA-Z0-9$]+)(?:\[(\d+)\])?\s*\("#)
                .map_err(|e| EngineError::StreamNotFound(e.to_string()))?,
            // Matches: var = fnName(arg) ... .set("n", var)
            Regex::new(r#"(?x)([\w$]+)\s*=\s*([a-zA-Z0-9$]+)(?:\[(\d+)\])?\([\w$]+\)[\s\S]{0,120}?\.set\("n"#)
                .map_err(|e| EngineError::StreamNotFound(e.to_string()))?,
        ];

        let mut candidate_name = None;
        for (i, re) in n_entry_regexes.iter().enumerate() {
            if let Some(caps) = re.captures(js_payload) {
                let name_idx = if i == 1 { 2 } else { 1 };
                let arr_idx = if i == 1 { 3 } else { 2 };
                let name = caps.get(name_idx).map(|m| m.as_str());
                let idx = caps.get(arr_idx).map(|m| m.as_str());

                if let Some(name) = name {
                    if idx.is_some() {
                        // Look for array definition: var name = [realName];
                        let arr_re = Regex::new(&format!(
                            r#"(?:var\s+)?{}\s*=\s*\[\s*([a-zA-Z0-9$]+)\s*\]"#,
                            regex::escape(name)
                        ))
                        .map_err(|e| EngineError::StreamNotFound(e.to_string()))?;
                        if let Some(arr_cap) = arr_re.captures(js_payload)
                            && let Some(real_fn) = arr_cap.get(1)
                        {
                            candidate_name = Some(real_fn.as_str().to_string());
                            break;
                        }
                    }
                    candidate_name = Some(name.to_string());
                    break;
                }
            }
        }

        let func_name = match candidate_name {
            Some(n) => n,
            None => {
                // Fallback: look for standard obfuscated function splitting param and returning join("")
                let fallback_re = Regex::new(r#"(?x)
                    (?:var\s+)?(?P<func>[a-zA-Z0-9$]+)\s*=\s*function\(\s*(?P<param>[a-zA-Z0-9$]+)\s*\)\s*\{
                        \s*(?:var\s+)?(?P<param2>[a-zA-Z0-9$]+)\s*=\s*[a-zA-Z0-9$]+\.split\(""\);\s*
                        [\s\S]+?
                        return\s+[a-zA-Z0-9$]+\.join\(""\);?\s*
                    \}
                "#).map_err(|e| EngineError::StreamNotFound(e.to_string()))?;

                fallback_re
                    .captures(js_payload)
                    .and_then(|c| c.name("func").map(|m| m.as_str().to_string()))
                    .ok_or_else(|| {
                        EngineError::StreamNotFound(
                            "Failed to find n-challenge function in JS payload".to_string(),
                        )
                    })?
            }
        };

        // Extract the function signature and body block using balanced braces
        let def_regex = Regex::new(&format!(
            r#"(?x)(?:var\s+)?{}\s*=\s*function\s*\((?P<p1>[^)]*)\)\s*\{{|function\s+{}\s*\((?P<p2>[^)]*)\)\s*\{{"#,
            regex::escape(&func_name),
            regex::escape(&func_name)
        )).map_err(|e| EngineError::StreamNotFound(e.to_string()))?;

        let (param, body_block) = if let Some(caps) = def_regex.captures(js_payload) {
            let p = caps
                .name("p1")
                .or_else(|| caps.name("p2"))
                .map(|m| m.as_str())
                .unwrap_or("a");
            let full_match = caps
                .get(0)
                .ok_or_else(|| EngineError::StreamNotFound("Regex match error".to_string()))?;
            let brace_pos = full_match.end() - 1;
            let block = extract_brace_block(js_payload, brace_pos).ok_or_else(|| {
                EngineError::StreamNotFound(format!(
                    "Failed to extract brace block for function '{}'",
                    func_name
                ))
            })?;
            (p, block)
        } else {
            return Err(EngineError::StreamNotFound(format!(
                "Definition for function '{}' not found",
                func_name
            )));
        };

        // Find all helper object calls: <helper>.<method>(
        let helper_obj_re = Regex::new(r#"([a-zA-Z0-9$]+)\.[a-zA-Z0-9$]+\("#)
            .map_err(|e| EngineError::StreamNotFound(e.to_string()))?;

        let mut helper_defs = Vec::new();
        let mut seen_helpers = HashSet::new();

        for cap in helper_obj_re.captures_iter(body_block) {
            if let Some(h_match) = cap.get(1) {
                let h_name = h_match.as_str();
                if seen_helpers.insert(h_name.to_string()) {
                    if matches!(
                        h_name,
                        "Math" | "Array" | "String" | "Object" | "console" | "window"
                    ) {
                        continue;
                    }
                    let obj_re = Regex::new(&format!(
                        r#"(?x)(?:var\s+)?{}\s*=\s*\{{"#,
                        regex::escape(h_name)
                    ))
                    .map_err(|e| EngineError::StreamNotFound(e.to_string()))?;

                    if let Some(obj_cap) = obj_re.captures(js_payload)
                        && let Some(obj_full) = obj_cap.get(0)
                    {
                        let brace_pos = obj_full.end() - 1;
                        if let Some(obj_body) = extract_brace_block(js_payload, brace_pos) {
                            helper_defs.push(format!("var {} = {};", h_name, obj_body));
                        }
                    }
                }
            }
        }

        // Reconstruct executable script block with a unified solver entry point
        let compiled_script = format!(
            "{}\nvar {} = function({}) {};\nvar __mm_dlp_n_solver = {};",
            helper_defs.join("\n"),
            func_name,
            param,
            body_block,
            func_name
        );

        let result =
            self.engine
                .execute_decipher(&compiled_script, n_token, "__mm_dlp_n_solver")?;

        if let Ok(mut guard) = self.n_cache.lock() {
            guard.insert(n_token.to_string(), result.clone());
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_decipher_mock() {
        let mock_js = r#"
            var helperObj = {
                reverse: function(a) { a.reverse(); },
                swap: function(a, b) { var c = a[0]; a[0] = a[b % a.length]; a[b % a.length] = c; },
                splice: function(a, b) { a.splice(0, b); }
            };
            var decipherFn = function(a) {
                var b = a.split("");
                helperObj.reverse(b);
                helperObj.swap(b, 2);
                return b.join("");
            };
        "#;

        let decipherer = JsDecipher::new().expect("Failed to initialize JsDecipher");
        let result = decipherer
            .decipher(mock_js, "abcdefg")
            .expect("Decipher failed");
        // "abcdefg" reversed is "gfedcba".
        // swap b[0] and b[2]: 'g' and 'e' swapped -> "efgdcba"
        assert_eq!(result, "efgdcba");
    }

    #[test]
    fn test_js_decipher_n_mock_with_get_entry() {
        let mock_js = r#"
            var nHelper = {
                swap: function(a, b) { var c = a[0]; a[0] = a[b % a.length]; a[b % a.length] = c; },
                reverse: function(a) { a.reverse(); }
            };
            var nFunc = function(a) {
                var b = a.split("");
                nHelper.reverse(b);
                nHelper.swap(b, 1);
                return b.join("");
            };
            // Entry point simulation
            var dummy = a.get("n") && (b = nFunc(b));
        "#;

        let decipherer = JsDecipher::new().expect("Failed to initialize JsDecipher");
        let result = decipherer
            .decipher_n(mock_js, "12345")
            .expect("Decipher n failed");
        // "12345" reversed: "54321".
        // swap b[0] and b[1 % 5 = 1]: '5' and '4' -> "45321".
        assert_eq!(result, "45321");
    }

    #[test]
    fn test_js_decipher_n_with_array_indirection() {
        let mock_js = r#"
            var nHelper = {
                swap: function(a, b) { var c = a[0]; a[0] = a[b % a.length]; a[b % a.length] = c; }
            };
            var actualFn = function(a) {
                var b = a.split("");
                nHelper.swap(b, 3);
                return b.join("");
            };
            var fnArr = [actualFn];
            var test = a.get("n") && (b = fnArr[0](b));
        "#;

        let decipherer = JsDecipher::new().expect("Failed to initialize JsDecipher");
        let result = decipherer
            .decipher_n(mock_js, "abcdef")
            .expect("Decipher n array indirection failed");
        // swap b[0] and b[3]: 'a' and 'd' -> "dbcaef"
        assert_eq!(result, "dbcaef");

        // Verify caching returns same result without re-evaluating
        let cached = decipherer.decipher_n(mock_js, "abcdef").expect("Cache hit");
        assert_eq!(cached, "dbcaef");
    }
}
