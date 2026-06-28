/// The build script for the `mm-dlp-core` crate.
///
/// This script is executed by Cargo prior to compiling the main crate.
fn main() {
    // Generate the Rust scaffolding code required by UniFFI to enable cross-language FFI bindings.
    // This parses the interface defined in the UDL file and automatically creates 
    // the necessary C-API boilerplate during the build step.
    uniffi::generate_scaffolding("./src/mm-dlp.udl").expect("Build script failed");
}
