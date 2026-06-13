use mm_dlp_core::client::h2::{H2Impersonator, CHROME_INITIAL_WINDOW_SIZE};
use mm_dlp_core::client::tls::TlsImpersonator;
use std::io::Read;
use std::net::TcpListener;
use std::thread;

#[test]
fn test_tls_impersonation_against_mock_server() {
    // Setup a mock HTTPS server listener to intercept the TLS ClientHello
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();
        assert!(n > 0, "No data received from client");
        
        // TLS ClientHello strictly starts with `0x16` (Handshake Layer Payload Record)
        assert_eq!(buf[0], 0x16, "Expected an outgoing TLS Handshake record");
    });

    let tls_impersonator = TlsImpersonator::new().expect("Failed to initialize TLS impersonator");
    let connector = tls_impersonator.get_connector();
    let stream = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();

    // Attempt to connect. The SSL handshaking certification check will fail because there is no mocked cert verification response, but the socket successfully yields out a correct ClientHello payload regardless.
    let _ = connector.connect("localhost", stream);

    server.join().unwrap();
}

#[test]
fn test_h2_pseudo_headers_ordering() {
    let headers = H2Impersonator::format_pseudo_headers("GET", "example.com", "https", "/video");

    assert_eq!(headers[0].0, ":method");
    assert_eq!(headers[0].1.as_ref(), b"GET");

    assert_eq!(headers[1].0, ":authority");
    assert_eq!(headers[1].1.as_ref(), b"example.com");

    assert_eq!(headers[2].0, ":scheme");
    assert_eq!(headers[2].1.as_ref(), b"https");

    assert_eq!(headers[3].0, ":path");
    assert_eq!(headers[3].1.as_ref(), b"/video");
}

#[test]
fn test_h2_window_size() {
    assert_eq!(CHROME_INITIAL_WINDOW_SIZE, 6_291_456);
}