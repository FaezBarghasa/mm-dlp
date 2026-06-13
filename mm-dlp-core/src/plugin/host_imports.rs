use wasmer::{FunctionEnvMut, Memory, MemoryView, WasmPtr};

#[derive(Clone)]
pub struct PluginEnv {
    pub memory: Option<Memory>,
}

impl PluginEnv {
    pub fn new() -> Self {
        Self { memory: None }
    }
}

/// Safely checks if the requested memory bounds fall within the WASM guest's allocated memory.
fn check_memory_bounds(view: &MemoryView, ptr: u32, len: u32) -> bool {
    let size = view.data_size();
    (ptr as u64 + len as u64) <= size
}

/// Host function: Logs memory-safely from the WASM guest.
pub fn host_log(mut env: FunctionEnvMut<PluginEnv>, msg_ptr: u32, msg_len: u32) {
    let (env_data, store) = env.data_and_store_mut();
    let memory = match env_data.memory.as_ref() {
        Some(mem) => mem,
        None => return,
    };

    let view = memory.view(&store);
    
    if !check_memory_bounds(&view, msg_ptr, msg_len) {
        eprintln!("[Host] WASM memory bounds violation on log execution.");
        return;
    }

    let ptr = WasmPtr::<u8>::new(msg_ptr);
    if let Ok(msg) = ptr.read_utf8_string(&view, msg_len) {
        println!("[WASM Plugin] {}", msg);
    }
}

/// Host function: Executes a network GET request on behalf of the guest, injecting
/// the response directly into the guest's memory buffer (zero-copy sharing pattern).
pub fn host_network_get(
    mut env: FunctionEnvMut<PluginEnv>,
    url_ptr: u32,
    url_len: u32,
    out_ptr: u32,
    out_max_len: u32,
) -> u32 {
    let (env_data, store) = env.data_and_store_mut();
    let memory = match env_data.memory.as_ref() {
        Some(mem) => mem,
        None => return 0,
    };

    let view = memory.view(&store);

    // Enforce dual memory boundary verification for input strings and output buffers
    if !check_memory_bounds(&view, url_ptr, url_len) {
        return 0;
    }
    if !check_memory_bounds(&view, out_ptr, out_max_len) {
        return 0;
    }

    let ptr = WasmPtr::<u8>::new(url_ptr);
    let url_str = match ptr.read_utf8_string(&view, url_len) {
        Ok(url) => url,
        Err(_) => return 0,
    };

    // Spawn blocking HTTP client on a separate thread to prevent panicking Tokio executor runtimes
    let fetch_handle = std::thread::spawn(move || {
        if let Ok(resp) = reqwest::blocking::get(&url_str) {
            if let Ok(bytes) = resp.bytes() {
                return bytes;
            }
        }
        bytes::Bytes::new()
    });

    let network_bytes = fetch_handle.join().unwrap_or_default();
    let bytes_to_write = std::cmp::min(network_bytes.len() as u32, out_max_len);

    if bytes_to_write > 0 {
        let out_wasm_ptr = WasmPtr::<u8>::new(out_ptr);
        if let Ok(slice) = out_wasm_ptr.slice(&view, bytes_to_write) {
            let _ = slice.write_slice(&network_bytes[..bytes_to_write as usize]);
            return bytes_to_write;
        }
    }

    0
}