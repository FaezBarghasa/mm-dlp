use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag controlling whether QUIC/HTTP3 is enabled.
/// Read by `QuicHttpClient` before each connection attempt.
pub static QUIC_ENABLED: AtomicBool = AtomicBool::new(true);

/// Returns the current QUIC-enabled state.
pub fn is_quic_enabled() -> bool {
    QUIC_ENABLED.load(Ordering::Relaxed)
}

/// Sets the global QUIC-enabled flag. This takes effect on the next connection attempt.
pub fn set_quic_enabled(enabled: bool) {
    QUIC_ENABLED.store(enabled, Ordering::Relaxed);
}
