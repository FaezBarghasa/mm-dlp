//! Utility functions for URL sanitization, filename cleaning, and path validation.

pub fn sanitize_url(raw_url: &str) -> String {
    raw_url.trim().to_string()
}
