//! Core engine implementation for mm-dlp.

use crate::config::EngineConfig;

#[derive(Debug)]
pub struct Engine {
    config: EngineConfig,
}

impl Engine {
    pub fn new(config: EngineConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }
}
