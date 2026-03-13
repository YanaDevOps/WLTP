//! WLTP - Modern WinMTR for Windows/macOS
//! 
//! This module provides the core types used throughout the application.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::net::IpAddr;

/// Severity level for hop and session interpretation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Everything looks good
    Ok,
    /// Something worth attention, but may not be a problem
    Warning,
    /// Significant issue detected
    Critical,
    /// Unable to determine status (e.g., no response)
    Unknown,
}

/// Protocol mode for tracing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProtocolMode {
    Icmp,
    Udp,
    Tcp,
}

/// Configuration for a trace session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConfig {
    /// Target hostname or IP address
    pub target: String,
    /// Protocol to use for probing
    #[serde(default = "default_protocol")]
    pub protocol: ProtocolMode,
    /// Interval between probes in milliseconds
    #[serde(default = "default_interval")]
    pub interval_ms: u64,
    /// Maximum number of hops to trace
    #[serde(default = "default_max_hops")]
    pub max_hops: u8,
    /// Timeout for each probe in milliseconds
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    /// Number of packets to send per hop (0 = infinite)
    #[serde(default = "default_count")]
    pub count: u32,
}

fn default_protocol() -> ProtocolMode { ProtocolMode::Icmp }
fn default_interval() -> u64 { 1000 }
fn default_max_hops() -> u8 { 30 }
fn default_timeout() -> u64 { 1000 }
fn default_count() -> u32 { 0 }

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            target: String::new(),
            protocol: default_protocol(),
            interval_ms: default_interval(),
            max_hops: default_max_hops(),
            timeout_ms: default_timeout(),
            count: default_count(),
        }
    }
}

/// Running state of a trace session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    /// Session is being initialized
    Initializing,
    /// Session is actively probing
    Running,
    /// Session has been paused
    Paused,
    /// Session has completed (reached count limit or stopped)
    Completed,
    /// Session encountered an error
    Error,
}

/// A single trace session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSession {
    /// Unique session identifier
    pub id: String,
    /// Configuration used for this session
    pub config: TraceConfig,
    /// Resolved target IP address
    pub target_ip: Option<IpAddr>,
    /// Current state of the session
    pub state: SessionState,
    /// When the session was started
    pub started_at: Option<DateTime<Utc>>,
    /// When the session ended (if completed)
    pub ended_at: Option<DateTime<Utc>>,
    /// Error message if state is Error
    pub error: Option<String>,
}

impl TraceSession {
    pub fn new(config: TraceConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            config,
            target_ip: None,
            state: SessionState::Initializing,
            started_at: None,
            ended_at: None,
            error: None,
        }
    }
}

/// Statistics for a single hop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HopStats {
    /// Number of packets sent
    pub sent: u32,
    /// Number of packets received
    pub received: u32,
    /// Packet loss percentage (0-100)
    pub loss_percent: f64,
    /// Best latency in milliseconds
    pub best_ms: Option<f64>,
    /// Worst latency in milliseconds
    pub worst_ms: Option<f64>,
    /// Average latency in milliseconds
    pub avg_ms: Option<f64>,
    /// Most recent latency in milliseconds
    pub last_ms: Option<f64>,
    /// Jitter (variation) in milliseconds
    pub jitter_ms: Option<f64>,
}

impl Default for HopStats {
    fn default() -> Self {
        Self {
            sent: 0,
            received: 0,
            loss_percent: 0.0,
            best_ms: None,
            worst_ms: None,
            avg_ms: None,
            last_ms: None,
            jitter_ms: None,
        }
    }
}

impl HopStats {
    /// Update statistics with a new latency sample
    pub fn add_sample(&mut self, latency_ms: f64) {
        self.sent += 1;
        self.received += 1;
        
        self.last_ms = Some(latency_ms);
        
        match self.best_ms {
            None => self.best_ms = Some(latency_ms),
            Some(best) if latency_ms < best => self.best_ms = Some(latency_ms),
            _ => {}
        }
        
        match self.worst_ms {
            None => self.worst_ms = Some(latency_ms),
            Some(worst) if latency_ms > worst => self.worst_ms = Some(latency_ms),
            _ => {}
        }
        
        // Calculate running average
        let prev_avg = self.avg_ms.unwrap_or(0.0);
        let n = self.received as f64;
        self.avg_ms = Some(prev_avg + (latency_ms - prev_avg) / n);
        
        // Calculate jitter as mean deviation from average
        if let Some(avg) = self.avg_ms {
            let deviation = (latency_ms - avg).abs();
            let prev_jitter = self.jitter_ms.unwrap_or(0.0);
            self.jitter_ms = Some(prev_jitter + (deviation - prev_jitter) / n);
        }
    }
    
    /// Record a timeout (packet sent but not received)
    pub fn add_timeout(&mut self) {
        self.sent += 1;
        self.loss_percent = if self.sent > 0 {
            ((self.sent - self.received) as f64 / self.sent as f64) * 100.0
        } else {
            0.0
        };
    }
}

/// A single hop in the trace route
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HopSample {
    /// Hop index (1-based, as shown in traceroute)
    pub index: u8,
    /// Resolved hostname (if available)
    pub hostname: Option<String>,
    /// IP address of the hop
    pub ip: Option<IpAddr>,
    /// Statistics for this hop
    pub stats: HopStats,
    /// Current status based on statistics
    pub status: Severity,
    /// Interpretation of this hop's behavior
    pub interpretation: Option<HopInterpretation>,
}

impl HopSample {
    pub fn new(index: u8) -> Self {
        Self {
            index,
            hostname: None,
            ip: None,
            stats: HopStats::default(),
            status: Severity::Unknown,
            interpretation: None,
        }
    }
}

/// Interpretation of a hop's behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HopInterpretation {
    /// Severity level
    pub severity: Severity,
    /// Short headline (e.g., "High latency detected")
    pub headline: String,
    /// Detailed explanation
    pub explanation: String,
    /// Possible causes for this behavior
    pub probable_causes: Vec<String>,
    /// Confidence level (0.0 - 1.0)
    pub confidence: f64,
}

/// Overall session summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Overall status of the route
    pub overall_status: Severity,
    /// Primary finding (main issue or "looks good")
    pub primary_finding: String,
    /// Secondary findings (additional observations)
    pub secondary_findings: Vec<String>,
    /// Recommended next steps
    pub recommended_next_steps: Vec<String>,
    /// Index of the problem hop (if applicable)
    pub problem_hop_index: Option<u8>,
    /// Whether the destination is reachable
    pub destination_reachable: bool,
}

impl Default for SessionSummary {
    fn default() -> Self {
        Self {
            overall_status: Severity::Unknown,
            primary_finding: "Analysis pending...".to_string(),
            secondary_findings: Vec::new(),
            recommended_next_steps: Vec::new(),
            problem_hop_index: None,
            destination_reachable: false,
        }
    }
}

/// Event emitted during tracing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceEvent {
    /// Session has started
    SessionStarted {
        session: TraceSession,
    },
    /// A new hop was discovered
    HopDiscovered {
        session_id: String,
        hop: HopSample,
    },
    /// A hop received a response
    HopResponse {
        session_id: String,
        hop_index: u8,
        latency_ms: f64,
    },
    /// A hop timed out
    HopTimeout {
        session_id: String,
        hop_index: u8,
    },
    /// Statistics updated for a hop
    HopStatsUpdate {
        session_id: String,
        hop_index: u8,
        stats: HopStats,
    },
    /// Session completed
    SessionCompleted {
        session_id: String,
        summary: SessionSummary,
        hops: Vec<HopSample>,
    },
    /// Session encountered an error
    SessionError {
        session_id: String,
        error: String,
    },
    /// DNS resolution result
    DnsResolved {
        session_id: String,
        hostname: String,
        ip: IpAddr,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hop_stats_sample() {
        let mut stats = HopStats::default();
        stats.add_sample(10.0);
        stats.add_sample(20.0);
        stats.add_sample(15.0);
        
        assert_eq!(stats.sent, 3);
        assert_eq!(stats.received, 3);
        assert_eq!(stats.loss_percent, 0.0);
        assert_eq!(stats.best_ms, Some(10.0));
        assert_eq!(stats.worst_ms, Some(20.0));
        assert_eq!(stats.last_ms, Some(15.0));
        assert!(stats.avg_ms.unwrap() > 0.0);
    }
    
    #[test]
    fn test_hop_stats_timeout() {
        let mut stats = HopStats::default();
        stats.add_sample(10.0);
        stats.add_timeout();
        stats.add_sample(20.0);
        
        assert_eq!(stats.sent, 3);
        assert_eq!(stats.received, 2);
        assert!((stats.loss_percent - 33.333).abs() < 1.0);
    }
}
