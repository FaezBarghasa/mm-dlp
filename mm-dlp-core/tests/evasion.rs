use mm_dlp_core::client::h2::H2Impersonator;
use mm_dlp_core::client::tls::TlsImpersonator;
use std::io::Read;
use std::net::TcpListener;
use std::thread;

#[test]
fn test_tls_ja3_ciphers_match_chrome_profiles() {
    // Spin up a mock socket to intercept the ClientHello and ALPN requests natively
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap();
        
        assert!(n > 0, "No payload sent");
        assert_eq!(buf[0], 0x16, "First byte must match TLS Handshake (0x16)");

        let hex_dump = hex::encode(&buf[..n]);

        // Asserts presence of exact Chrome/Chromium ALPN & Cipher footprints 
        // (1301 = AES_128_GCM_SHA256, 1302 = AES_256_GCM_SHA384, 1303 = CHACHA20_POLY1305_SHA256)
        assert!(hex_dump.contains("1301"), "TLS cipher AES_128_GCM_SHA256 is missing");
        assert!(hex_dump.contains("1302"), "TLS cipher AES_256_GCM_SHA384 is missing");
        assert!(hex_dump.contains("1303"), "TLS cipher CHACHA20_POLY1305_SHA256 is missing");
    });

    let tls_impersonator = TlsImpersonator::new().expect("Failed to construct TLS impersonator");
    let connector = tls_impersonator.get_connector();
    let stream = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();

    // Intentionally fails verification since we mock the port, but correctly emits the Handshake bytes
    let _ = connector.connect("localhost", stream);
    server.join().unwrap();
}

#[test]
fn test_h2_pseudo_headers_evasion_layout() {
    // WAF systems fingerprint HTTP/2 clients by checking pseudo-header arrangements. 
    // Standard Chrome follows exactly: :method, :authority, :scheme, :path.
    let headers = H2Impersonator::format_pseudo_headers("GET", "google.com", "https", "/dl");
    
    assert_eq!(headers.len(), 4);
    assert_eq!(headers[0].0, ":method");
    assert_eq!(headers[1].0, ":authority");
    assert_eq!(headers[2].0, ":scheme");
    assert_eq!(headers[3].0, ":path");
}