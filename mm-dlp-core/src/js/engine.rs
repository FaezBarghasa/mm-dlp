use crate::client::EngineError;
use rquickjs::{Context, Function, Runtime};

pub struct SandboxJsEngine {
    #[allow(dead_code)]
    runtime: Runtime,
    context: Context,
}

impl SandboxJsEngine {
    pub fn new() -> Result<Self, EngineError> {
        let runtime = Runtime::new()
            .map_err(|e| EngineError::OsApiError(format!("Failed to create QuickJS runtime: {}", e)))?;
        
        // Restrict memory limit to 5 MB to prevent out-of-memory sandbox attacks
        runtime.set_memory_limit(5 * 1024 * 1024);
        // Restrict stack size to 512 KB to avoid deep recursion crashes
        runtime.set_max_stack_size(512 * 1024);
        
        let context = Context::full(&runtime)
            .map_err(|e| EngineError::OsApiError(format!("Failed to create QuickJS context: {}", e)))?;
        
        context.with(|ctx| {
            let global = ctx.globals();
            
            // Aggressively mask dangerous environment features to guarantee strict execution safety
            let _ = global.remove("eval");
            let _ = global.remove("Function");
            let _ = global.remove("globalThis");
            let _ = global.remove("process");
            let _ = global.remove("require");
            let _ = global.remove("console");
        });
        
        Ok(Self { runtime, context })
    }

    pub fn execute_decipher(&self, script: &str, argument: &str, target_fn: &str) -> Result<String, EngineError> {
        self.context.with(|ctx| {
            ctx.eval::<(), _>(script)
                .map_err(|e| EngineError::OsApiError(format!("Failed to evaluate JS script: {:?}", e)))?;
            
            let globals = ctx.globals();
            let func: Function = globals.get(target_fn)
                .map_err(|e| EngineError::OsApiError(format!("Target function '{}' not found: {:?}", target_fn, e)))?;
            
            func.call::<_, String>((argument,))
                .map_err(|e| EngineError::OsApiError(format!("Failed to execute decipher function: {:?}", e)))
        })
    }
}