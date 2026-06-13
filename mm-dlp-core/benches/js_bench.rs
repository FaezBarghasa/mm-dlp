use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mm_dlp_core::js::engine::SandboxJsEngine;

fn js_decipher_benchmark(c: &mut Criterion) {
    // Compiling the entire sandbox wrapper locally and initializing JS contexts.
    let engine = SandboxJsEngine::new().expect("Failed to initialize QuickJS SandboxJsEngine");
    
    let script = r#"
        var cipher_helper = {
            reverse: function(a) { a.reverse(); },
            splice: function(a, b) { a.splice(0, b); },
            swap: function(a, b) { var c = a[0]; a[0] = a[b % a.length]; a[b % a.length] = c; }
        };
        var decipher_signature = function(a) {
            var b = a.split("");
            cipher_helper.reverse(b);
            cipher_helper.splice(b, 2);
            cipher_helper.swap(b, 3);
            return b.join("");
        };
    "#;
    
    let argument = "0123456789abcdef";
    let target_fn = "decipher_signature";
    
    c.bench_function("quickjs_decipher_execution", |b| {
        b.iter(|| {
            let result = engine.execute_decipher(black_box(script), black_box(argument), black_box(target_fn)).unwrap();
            black_box(result);
        });
    });
}

criterion_group!(benches, js_decipher_benchmark);
criterion_main!(benches);