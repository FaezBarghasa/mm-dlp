use rquickjs::{Runtime, Context};
use crate::error::{Result, EngineError};

pub struct SandboxJsEngine {
    _runtime: Runtime,
    context: Context,
}

impl SandboxJsEngine {
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new()
            .map_err(|e| EngineError::InternalPanic(format!("Failed to spawn QuickJS thread: {}", e)))?;

        let context = Context::full(&runtime)
            .map_err(|e| EngineError::InternalPanic(format!("Context setup failed: {}", e)))?;

        Ok(Self { _runtime: runtime, context })
    }

    pub fn execute_decipher(&self, script: &str, argument: &str, target_fn: &str) -> Result<String> {
        self.context.with(|ctx| {
            // Setup sandbox boundary parameters
            ctx.eval::<(), _>("var window = {}; var global = {};")
                .map_err(|e| EngineError::ExtractorBanned { reason: e.to_string() })?;

            // Compile signature code payload
            ctx.eval::<(), _>(script)
                .map_err(|e| EngineError::ExtractorBanned { reason: format!("Compilation failed: {}", e) })?;

            let execution_call = format!("{0}(\"{1}\");", target_fn, argument);
            let result: String = ctx.eval(&execution_call)
                .map_err(|e| EngineError::ExtractorBanned { reason: format!("Execution failed: {}", e) })?;

            Ok(result)
        })
    }
}
