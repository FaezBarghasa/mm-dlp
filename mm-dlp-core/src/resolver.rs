//! Resolver module for matching stream candidates across platforms.

pub struct StreamResolver;

impl StreamResolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StreamResolver {
    fn default() -> Self {
        Self::new()
    }
}
