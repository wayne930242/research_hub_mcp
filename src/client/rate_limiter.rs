use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, info};

/// Simple rate limiter for requests to respect server resources
pub struct RateLimiter {
    requests_per_second: f64,
    last_request_time: Option<Instant>,
    min_interval: Duration,
}

/// Provider-aware rate limiter that uses configuration and progress feedback
pub struct ProviderRateLimiter {
    provider_name: String,
    inner: AdaptiveRateLimiter,
    show_progress: bool,
    allow_burst: bool,
    burst_size: u32,
    burst_count: u32,
    burst_start: Option<Instant>,
}

impl RateLimiter {
    /// Create a new rate limiter with the specified rate (requests per second)
    pub fn new(requests_per_second: f64) -> Self {
        let min_interval = if requests_per_second > 0.0 {
            Duration::from_millis((1000.0 / requests_per_second) as u64)
        } else {
            Duration::from_secs(1)
        };

        debug!(
            "Created rate limiter: {} requests per second",
            requests_per_second
        );

        Self {
            requests_per_second,
            last_request_time: None,
            min_interval,
        }
    }

    /// Create a new rate limiter with the specified rate (legacy u32 support)
    #[must_use]
    pub fn new_legacy(requests_per_second: u32) -> Self {
        Self::new(f64::from(requests_per_second))
    }

    /// Wait until it's safe to make a request (respects rate limit)
    pub async fn acquire(&mut self) {
        let now = Instant::now();

        if let Some(last_time) = self.last_request_time {
            let elapsed = now.duration_since(last_time);
            if elapsed < self.min_interval {
                let wait_time = self.min_interval.saturating_sub(elapsed);
                debug!("Rate limiter: waiting {}ms", wait_time.as_millis());
                sleep(wait_time).await;
            }
        }

        self.last_request_time = Some(Instant::now());
        debug!("Rate limiter: request permitted");
    }

    /// Check if a request would be allowed without waiting
    #[must_use]
    pub fn check(&self) -> bool {
        self.last_request_time.map_or(true, |last_time| {
            Instant::now().duration_since(last_time) >= self.min_interval
        })
    }

    /// Get the current rate limit (requests per second)
    #[must_use]
    pub const fn rate_per_second(&self) -> f64 {
        self.requests_per_second
    }

    /// Update the rate limit
    pub fn update_rate(&mut self, requests_per_second: f64) {
        if (requests_per_second - self.requests_per_second).abs() > f64::EPSILON {
            self.requests_per_second = requests_per_second;
            self.min_interval = if requests_per_second > 0.0 {
                Duration::from_millis((1000.0 / requests_per_second) as u64)
            } else {
                Duration::from_secs(1)
            };

            debug!(
                "Updated rate limiter: {} requests per second",
                requests_per_second
            );
        }
    }

    /// Get time until next request is allowed
    #[must_use]
    pub fn time_until_ready(&self) -> Option<Duration> {
        self.last_request_time.and_then(|last_time| {
            let elapsed = Instant::now().duration_since(last_time);
            if elapsed >= self.min_interval {
                None
            } else {
                Some(self.min_interval.saturating_sub(elapsed))
            }
        })
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(1.0) // 1 request per second by default
    }
}

/// Configuration for rate limiting behavior
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Base requests per second
    pub requests_per_second: f64,
    /// Whether to use adaptive rate limiting based on response times
    pub adaptive: bool,
    /// Minimum rate when adapting (requests per second)
    pub min_rate: f64,
    /// Maximum rate when adapting (requests per second)
    pub max_rate: f64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 1.0,
            adaptive: true,
            min_rate: 0.25,
            max_rate: 5.0,
        }
    }
}

/// Adaptive rate limiter that adjusts based on server response times
pub struct AdaptiveRateLimiter {
    inner: RateLimiter,
    config: RateLimitConfig,
    response_times: Vec<Duration>,
    max_samples: usize,
}

impl AdaptiveRateLimiter {
    /// Create a new adaptive rate limiter
    #[must_use]
    pub fn new(config: RateLimitConfig) -> Self {
        let inner = RateLimiter::new(config.requests_per_second);

        Self {
            inner,
            config,
            response_times: Vec::new(),
            max_samples: 10, // Keep last 10 response times
        }
    }

    /// Record a response time and potentially adjust the rate
    pub fn record_response_time(&mut self, response_time: Duration) {
        if !self.config.adaptive {
            return;
        }

        self.response_times.push(response_time);

        // Keep only the most recent samples
        if self.response_times.len() > self.max_samples {
            self.response_times.remove(0);
        }

        // Adjust rate based on average response time
        if self.response_times.len() >= 3 {
            let avg_response_time = self.response_times.iter().sum::<Duration>()
                / u32::try_from(self.response_times.len()).unwrap_or(1);

            let new_rate = if avg_response_time > Duration::from_millis(5000) {
                // Slow responses - decrease rate
                (self.inner.rate_per_second() - 0.25).max(self.config.min_rate)
            } else if avg_response_time < Duration::from_millis(1000) {
                // Fast responses - increase rate
                (self.inner.rate_per_second() + 0.25).min(self.config.max_rate)
            } else {
                // Keep current rate
                self.inner.rate_per_second()
            };

            if (new_rate - self.inner.rate_per_second()).abs() > f64::EPSILON {
                debug!("Adaptive rate limiting: adjusting from {} to {} requests/sec (avg response time: {}ms)",
                      self.inner.rate_per_second(), new_rate, avg_response_time.as_millis());
                self.inner.update_rate(new_rate);
            }
        }
    }

    /// Wait until it's safe to make a request
    pub async fn acquire(&mut self) {
        self.inner.acquire().await;
    }

    /// Check if a request would be allowed without waiting
    #[must_use]
    pub fn check(&self) -> bool {
        self.inner.check()
    }

    /// Get the current rate limit
    #[must_use]
    pub const fn current_rate(&self) -> f64 {
        self.inner.rate_per_second()
    }

    /// Get average response time from recent samples
    #[must_use]
    pub fn average_response_time(&self) -> Option<Duration> {
        if self.response_times.is_empty() {
            None
        } else {
            Some(
                self.response_times.iter().sum::<Duration>()
                    / u32::try_from(self.response_times.len()).unwrap_or(1),
            )
        }
    }
}

impl ProviderRateLimiter {
    /// Create a new provider-specific rate limiter using configuration
    pub fn new(provider_name: String, config: &crate::config::RateLimitingConfig) -> Self {
        let rate = config
            .providers
            .get(&provider_name)
            .copied()
            .unwrap_or(config.default_rate);

        let rate_config = RateLimitConfig {
            requests_per_second: rate,
            adaptive: config.adaptive,
            min_rate: config.min_rate,
            max_rate: config.max_rate,
        };

        let inner = AdaptiveRateLimiter::new(rate_config);

        debug!(
            "Created provider rate limiter for '{}' at {} req/sec",
            provider_name, rate
        );

        Self {
            provider_name,
            inner,
            show_progress: config.show_progress,
            allow_burst: config.allow_burst,
            burst_size: config.burst_size,
            burst_count: 0,
            burst_start: None,
        }
    }

    /// Wait until it's safe to make a request with progress indication
    pub async fn acquire(&mut self) -> Result<(), crate::Error> {
        let start = Instant::now();

        // Check if we can use burst allowance
        if self.allow_burst && self.can_burst() {
            self.use_burst();
            return Ok(());
        }

        // Calculate wait time before actual wait
        let wait_time = self.inner.inner.time_until_ready();

        if let Some(duration) = wait_time {
            if self.show_progress && duration > Duration::from_millis(500) {
                info!(
                    "⏳ Rate limiting {} - waiting {:.1}s",
                    self.provider_name,
                    duration.as_secs_f64()
                );
            }
        }

        self.inner.acquire().await;

        if self.show_progress {
            let elapsed = start.elapsed();
            if elapsed > Duration::from_millis(100) {
                debug!(
                    "Rate limit wait for {}: {:.1}s",
                    self.provider_name,
                    elapsed.as_secs_f64()
                );
            }
        }

        Ok(())
    }

    /// Record response time for adaptive rate limiting
    pub fn record_response_time(&mut self, response_time: Duration) {
        self.inner.record_response_time(response_time);
    }

    /// Check if request can use burst allowance
    fn can_burst(&self) -> bool {
        let now = Instant::now();

        match self.burst_start {
            None => true, // First request can always burst
            Some(start) => {
                // Reset burst window after 1 minute
                if now.duration_since(start) > Duration::from_secs(60) {
                    true
                } else {
                    self.burst_count < self.burst_size
                }
            }
        }
    }

    /// Use one burst allowance
    fn use_burst(&mut self) {
        let now = Instant::now();

        match self.burst_start {
            None => {
                // Start new burst window
                self.burst_start = Some(now);
                self.burst_count = 1;
            }
            Some(start) => {
                if now.duration_since(start) > Duration::from_secs(60) {
                    // Reset burst window
                    self.burst_start = Some(now);
                    self.burst_count = 1;
                } else {
                    // Continue in current window
                    self.burst_count += 1;
                }
            }
        }

        if self.show_progress {
            debug!(
                "Using burst allowance for {} ({}/{})",
                self.provider_name, self.burst_count, self.burst_size
            );
        }
    }

    /// Get current rate
    #[must_use]
    pub const fn current_rate(&self) -> f64 {
        self.inner.current_rate()
    }

    /// Get provider name
    #[must_use]
    pub fn provider_name(&self) -> &str {
        &self.provider_name
    }

    /// Get average response time
    #[must_use]
    pub fn average_response_time(&self) -> Option<Duration> {
        self.inner.average_response_time()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Instant;

    #[tokio::test]
    async fn test_rate_limiter_basic() {
        let mut limiter = RateLimiter::new(2.0); // 2 requests per second

        // First request should be immediate
        let start = Instant::now();
        limiter.acquire().await;
        let first_duration = start.elapsed();
        assert!(first_duration < Duration::from_millis(100));

        // Second request should wait for the interval
        limiter.acquire().await;
        let second_duration = start.elapsed();
        assert!(second_duration >= Duration::from_millis(400)); // At least 500ms between requests for 2/sec
    }

    #[test]
    fn test_rate_limiter_check() {
        let limiter = RateLimiter::new(1.0);

        // Should be ready initially
        assert!(limiter.check());
    }

    #[test]
    fn test_rate_limiter_update() {
        let mut limiter = RateLimiter::new(1.0);
        assert!((limiter.rate_per_second() - 1.0).abs() < f64::EPSILON);

        limiter.update_rate(5.0);
        assert!((limiter.rate_per_second() - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_adaptive_rate_limiter() {
        let config = RateLimitConfig {
            requests_per_second: 2.0,
            adaptive: true,
            min_rate: 1.0,
            max_rate: 5.0,
        };

        let mut limiter = AdaptiveRateLimiter::new(config);
        assert!((limiter.current_rate() - 2.0).abs() < f64::EPSILON);

        // Record slow response times
        for _ in 0..5 {
            limiter.record_response_time(Duration::from_millis(6000));
        }

        // Rate should decrease due to slow responses
        assert!(limiter.current_rate() < 2.0);
    }

    #[test]
    fn test_adaptive_rate_limiter_fast_responses() {
        let config = RateLimitConfig {
            requests_per_second: 2.0,
            adaptive: true,
            min_rate: 1.0,
            max_rate: 5.0,
        };

        let mut limiter = AdaptiveRateLimiter::new(config);

        // Record fast response times
        for _ in 0..5 {
            limiter.record_response_time(Duration::from_millis(500));
        }

        // Rate should increase due to fast responses
        assert!(limiter.current_rate() > 2.0);
    }
}
