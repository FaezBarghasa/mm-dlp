use crate::client::EngineError;
use crate::js::engine::SandboxJsEngine;
use regex::Regex;

pub struct JsDecipher {
    engine: SandboxJsEngine,
}

impl JsDecipher {
    pub fn new() -> Result<Self, EngineError> {
        let engine = SandboxJsEngine::new()?;
        Ok(Self { engine })
    }

    pub fn decipher(&self, js_payload: &str, signature: &str) -> Result<String, EngineError> {
        // Regex patterns to identify cipher swap/reverse blocks inside dynamically scrambled payloads.
        // Matches common obfuscated function signatures: `decipher = function(a) { var b=a.split(""); ... return b.join("") }`
        let main_func_regex = Regex::new(r#"(?x)
            (?P<func_name>[a-zA-Z0-9$]+)\s*=\s*function\(\s*(?P<param>[a-zA-Z0-9$]+)\s*\)\s*\{
                \s*(?P<param2>[a-zA-Z0-9$]+)=(?P=param)\.split\(""\);\s*
                (?P<body>.+?)\s*
                return\s+(?P=param2)\.join\(""\)
            \}
        "#).map_err(|e| EngineError::OsApiError(e.to_string()))?;

        let captures = main_func_regex.captures(js_payload).ok_or_else(|| {
            EngineError::OsApiError("Failed to find decipher function block in JS payload".to_string())
        })?;

        let func_name = captures.name("func_name").map(|m| m.as_str()).unwrap_or("decipherFn");
        let param = captures.name("param").map(|m| m.as_str()).unwrap_or("a");
        let param2 = captures.name("param2").map(|m| m.as_str()).unwrap_or("a");
        let body = captures.name("body").map(|m| m.as_str()).unwrap_or("");

        // Identify the localized helper object being used inside the body for reversing & swapping
        let helper_obj_regex = Regex::new(r#"([a-zA-Z0-9$]+)\.[a-zA-Z0-9$]+\("#)
            .map_err(|e| EngineError::OsApiError(e.to_string()))?;
            
        let helper_obj_name = if let Some(cap) = helper_obj_regex.captures(body) {
            cap.get(1).map(|m| m.as_str()).unwrap_or("helperObj")
        } else {
            return Err(EngineError::OsApiError("Failed to find bound helper object in decipher function body".to_string()));
        };

        // Extract helper object definitions
        let obj_def_regex = Regex::new(&format!(r#"(?x)
            var\s+{}=\{{
                [\s\S]*?
            \}};
        "#, regex::escape(helper_obj_name))).map_err(|e| EngineError::OsApiError(e.to_string()))?;

        let obj_def = obj_def_regex.find(js_payload).map(|m| m.as_str()).unwrap_or(""); 

        // Reconstruct the script block containing strictly necessary cipher assets
        let compiled_script = format!(
            "{}\nvar {} = function({}) {{\n var {}={}.split(\"\");\n {}\n return {}.join(\"\");\n}};",
            obj_def, func_name, param, param2, param, body, param2
        );

        // Load the isolated script chunks directly into our compiled QuickJS Sandbox runtime
        self.engine.execute_decipher(&compiled_script, signature, func_name)
    }
}