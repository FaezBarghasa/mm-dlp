use crate::error::EngineError;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// Parse human-readable rate limits (e.g., "50K", "1.5M", "10M", "1G", "500000") into bytes per second.
pub fn parse_rate_limit(input: &str) -> Result<u64, EngineError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(EngineError::InvalidConfig(
            "Rate limit string cannot be empty".into(),
        ));
    }

    let upper = s.to_uppercase();
    let (num_str, multiplier) = if upper.ends_with("GIB") || upper.ends_with("GB") {
        (
            &s[..s.len() - if upper.ends_with("GIB") { 3 } else { 2 }],
            1024 * 1024 * 1024u64,
        )
    } else if upper.ends_with('G') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024u64)
    } else if upper.ends_with("MIB") || upper.ends_with("MB") {
        (
            &s[..s.len() - if upper.ends_with("MIB") { 3 } else { 2 }],
            1024 * 1024u64,
        )
    } else if upper.ends_with('M') {
        (&s[..s.len() - 1], 1024 * 1024u64)
    } else if upper.ends_with("KIB") || upper.ends_with("KB") {
        (
            &s[..s.len() - if upper.ends_with("KIB") { 3 } else { 2 }],
            1024u64,
        )
    } else if upper.ends_with('K') {
        (&s[..s.len() - 1], 1024u64)
    } else if upper.ends_with('B') {
        (&s[..s.len() - 1], 1u64)
    } else {
        (s, 1u64)
    };

    let num: f64 = num_str.trim().parse().map_err(|_| {
        EngineError::InvalidConfig(format!("Invalid numeric value in rate limit: '{}'", input))
    })?;

    if num <= 0.0 {
        return Err(EngineError::InvalidConfig(
            "Rate limit must be greater than zero".into(),
        ));
    }

    Ok((num * multiplier as f64).round() as u64)
}

struct BucketState {
    tokens: f64,
    last_replenish: Instant,
}

/// Token-bucket rate limiter for pacing byte streaming.
#[derive(Clone)]
pub struct RateLimiter {
    bytes_per_sec: u64,
    state: Arc<Mutex<BucketState>>,
}

impl RateLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        let max_rate = bytes_per_sec.max(1) as f64;
        Self {
            bytes_per_sec: bytes_per_sec.max(1),
            state: Arc::new(Mutex::new(BucketState {
                tokens: max_rate,
                last_replenish: Instant::now(),
            })),
        }
    }

    /// Asynchronously wait until the requested number of bytes can be transferred.
    /// Employs a token-debt algorithm so chunks larger than `bytes_per_sec` (e.g. 64KB chunk on 20KB/s limit)
    /// never deadlock or busy-loop.
    pub async fn acquire(&self, bytes: usize) {
        if self.bytes_per_sec == 0 || bytes == 0 {
            return;
        }

        let needed = bytes as f64;
        let rate = self.bytes_per_sec as f64;
        let capacity = rate;

        let wait_duration = {
            let mut state = self.state.lock().await;
            let now = Instant::now();
            let elapsed = now.duration_since(state.last_replenish).as_secs_f64();
            state.last_replenish = now;

            // Replenish tokens up to capacity
            state.tokens = (state.tokens + elapsed * rate).min(capacity);

            // Deduct requested tokens (token debt permitted)
            state.tokens -= needed;

            if state.tokens < 0.0 {
                let deficit = -state.tokens;
                Some(tokio::time::Duration::from_secs_f64(deficit / rate))
            } else {
                None
            }
        };

        if let Some(dur) = wait_duration {
            tokio::time::sleep(dur).await;
        }
    }

    pub fn bytes_per_sec(&self) -> u64 {
        self.bytes_per_sec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rate_limit_units() {
        assert_eq!(parse_rate_limit("500000").unwrap(), 500_000);
        assert_eq!(parse_rate_limit("50K").unwrap(), 50 * 1024);
        assert_eq!(parse_rate_limit("50k").unwrap(), 50 * 1024);
        assert_eq!(
            parse_rate_limit("1.5M").unwrap(),
            (1.5 * 1024.0 * 1024.0) as u64
        );
        assert_eq!(parse_rate_limit("10MB").unwrap(), 10 * 1024 * 1024);
        assert_eq!(parse_rate_limit("1G").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_rate_limit_invalid() {
        assert!(parse_rate_limit("").is_err());
        assert!(parse_rate_limit("-50K").is_err());
        assert!(parse_rate_limit("abc").is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter_pacing() {
        let limiter = RateLimiter::new(10_000); // 10 KB/s
        let start = Instant::now();
        // Consume initial burst
        limiter.acquire(10_000).await;
        // Acquire next 10 KB, which should force ~1s delay
        limiter.acquire(10_000).await;
        let elapsed = start.elapsed().as_millis();
        assert!(
            elapsed >= 800,
            "Expected at least 800ms pacing, got {}ms",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_rate_limiter_chunk_larger_than_rate() {
        // Test chunk (64KB) strictly larger than bytes_per_sec (20KB/s)
        let limiter = RateLimiter::new(20_000);
        let start = Instant::now();
        // First chunk consumes burst
        limiter.acquire(65_536).await;
        // Second chunk goes into debt, forcing ~3.2s pacing without deadlocking
        limiter.acquire(65_536).await;
        let elapsed = start.elapsed().as_millis();
        assert!(
            elapsed >= 2500,
            "Expected pacing for large chunk debt, got {}ms",
            elapsed
        );
    }
}
