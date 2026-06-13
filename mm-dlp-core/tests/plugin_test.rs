use mm_dlp_core::plugin::host_imports::{host_log, host_network_get, PluginEnv};
use wasmer::{imports, Function, FunctionEnv, Instance, Module, Store};

#[test]
fn test_wasm_syscall_execution() {
    let mut store = Store::default();
    
    // Compile WebAssembly Text format defining a dummy community plugin 
    // calling out to our enforced host execution APIs.
    let wat = r#"
    (module
      (import "env" "host_log" (func $host_log (param i32 i32)))
      (import "env" "host_network_get" (func $host_network_get (param i32 i32 i32 i32) (result i32)))
      
      (memory (export "memory") 1)
      
      ;; Data declarations for bounds testing
      (data (i32.const 0) "Hello, WASM!")
      (data (i32.const 20) "https://httpbin.org/get")
      
      (func (export "run_test") (result i32)
        ;; Log string at offset 0, length 12
        (call $host_log (i32.const 0) (i32.const 12))
        
        ;; Fetch URL at offset 20, length 23, dumping response to offset 100 max 256
        (call $host_network_get (i32.const 20) (i32.const 23) (i32.const 100) (i32.const 256))
      )
    )
    "#;

    let module = Module::new(&store, wat).expect("Failed to compile dummy WAT");
    let plugin_env = PluginEnv::new();
    let env = FunctionEnv::new(&mut store, plugin_env);

    let import_object = imports! {
        "env" => {
            "host_log" => Function::new_typed_with_env(&mut store, &env, host_log),
            "host_network_get" => Function::new_typed_with_env(&mut store, &env, host_network_get),
        }
    };

    let instance = Instance::new(&mut store, &module, &import_object).expect("Failed to instantiate WASM plugin");
    
    // Link the WASM instance memory directly to our isolated environment state.
    let memory = instance.exports.get_memory("memory").expect("Failed to export memory").clone();
    env.as_mut(&mut store).memory = Some(memory);

    let run_test = instance.exports.get_function("run_test").expect("Failed to find 'run_test'");
    let results = run_test.call(&mut store, &[]).expect("Failed to execute WASM function");

    assert_eq!(results.len(), 1);
}