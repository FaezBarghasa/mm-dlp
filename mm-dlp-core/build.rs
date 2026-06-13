fn main() {
    // Generate the Rust scaffolding code required by UniFFI.
    // This parses the interface defined in the UDL file and automatically 
    // creates the necessary C-API/FFI bindings during the compilation step.
    uniffi::generate_scaffolding("./src/mm-dlp.udl").expect("Build script failed");
}
