//! Portable traceroute implementation for Windows and macOS.
//!
//! This module preserves the public API expected by `commands.rs` while using
//! system traceroute commands instead of raw sockets. That keeps the build
//! compatible with current dependencies and avoids administrator-only socket
//! requirements.

use crate::types::*;
use std::net::IpAddr;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

/// Result type for traceroute operations.
pub type TraceResult<T> = Result<T, TraceError>;

/// Errors that can occur during tracing.
#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("DNS resolution failed: {0}")]
    DnsResolution(String),

    #[error("Socket error: {0}")]
    Socket(String),

    #[error("Permission denied (requires administrator/root)")]
    PermissionDenied,

    #[error("Invalid target: {0}")]
    InvalidTarget(String),

    #[error("Trace already running")]
    AlreadyRunning,

    #[error("Trace not running")]
    NotRunning,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Resolve a hostname to an IP address.
pub fn resolve_target(target: &str) -> TraceResult<IpAddr> {
    if let Ok(ip) = target.parse::<IpAddr>() {
        return Ok(ip);
    }

    dns_lookup::lookup_host(target)
        .map_err(|e| TraceError::DnsResolution(format!("{}: {}", target, e)))?
        .into_iter()
        .next()
        .ok_or_else(|| TraceError::DnsResolution(format!("No addresses found for {}", target)))
}

/// Active trace session state.
pub struct TraceRunner {
    config: TraceConfig,
    target_ip: IpAddr,
    hops: Vec<HopSample>,
    running: Arc<AtomicBool>,
    session_id: String,
}

impl TraceRunner {
    pub fn new(session: &TraceSession) -> TraceResult<Self> {
        let target_ip = resolve_target(&session.config.target)?;

        Ok(Self {
            config: session.config.clone(),
            target_ip,
            hops: Vec::new(),
            running: Arc::new(AtomicBool::new(false)),
            session_id: session.id.clone(),
        })
    }

    /// Get the resolved target IP.
    pub fn target_ip(&self) -> IpAddr {
        self.target_ip
    }

    /// Stop the trace.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    /// Run the trace, sending coarse-grained hop events as results are parsed.
    pub async fn run(&mut self, event_tx: mpsc::Sender<TraceEvent>) -> TraceResult<()> {
        if self.running.swap(true, Ordering::Relaxed) {
            return Err(TraceError::AlreadyRunning);
        }

        let target = self.config.target.clone();
        let max_hops = self.config.max_hops;
        let timeout_ms = self.config.timeout_ms;

        let result = tokio::task::spawn_blocking(move || {
            run_system_traceroute(&target, max_hops, timeout_ms)
        })
        .await
        .map_err(|e| TraceError::Internal(format!("Traceroute task failed: {}", e)))?;

        let hops = match result {
            Ok(hops) => hops,
            Err(err) => {
                self.running.store(false, Ordering::Relaxed);
                return Err(err);
            }
        };

        self.hops = hops;

        for hop in &self.hops {
            if !self.running.load(Ordering::Relaxed) {
                break;
            }

            let _ = event_tx
                .send(TraceEvent::HopDiscovered {
                    session_id: self.session_id.clone(),
                    hop: hop.clone(),
                })
                .await;

            let _ = event_tx
                .send(TraceEvent::HopStatsUpdate {
                    session_id: self.session_id.clone(),
                    hop_index: hop.index,
                    stats: hop.stats.clone(),
                })
                .await;
        }

        self.running.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Get current hop data.
    pub fn get_hops(&self) -> Vec<HopSample> {
        self.hops.clone()
    }
}

fn run_system_traceroute(
    target: &str,
    max_hops: u8,
    timeout_ms: u64,
) -> TraceResult<Vec<HopSample>> {
    info!("Starting portable trace to {}", target);

    #[cfg(windows)]
    {
        let output = Command::new("tracert")
            .arg("-h")
            .arg(max_hops.to_string())
            .arg("-w")
            .arg(timeout_ms.to_string())
            .arg(target)
            .output()
            .map_err(|e| TraceError::Socket(format!("Failed to run tracert: {}", e)))?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(TraceError::Internal(format!(
                "tracert failed: {}",
                error_msg.trim()
            )));
        }

        return parse_windows_tracert_output(&String::from_utf8_lossy(&output.stdout));
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("traceroute")
            .arg("-m")
            .arg(max_hops.to_string())
            .arg("-w")
            .arg((timeout_ms / 1000).max(1).to_string())
            .arg(target)
            .output()
            .map_err(|e| TraceError::Socket(format!("Failed to run traceroute: {}", e)))?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(TraceError::Internal(format!(
                "traceroute failed: {}",
                error_msg.trim()
            )));
        }

        return parse_macos_traceroute_output(&String::from_utf8_lossy(&output.stdout));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (target, max_hops, timeout_ms);
        Err(TraceError::Internal("Platform not supported".to_string()))
    }
}

#[cfg(windows)]
fn parse_windows_tracert_output(output: &str) -> TraceResult<Vec<HopSample>> {
    let mut hops = Vec::new();

    for raw_line in output.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with("Tracing route")
            || line.starts_with("over a maximum")
            || line.starts_with("Trace complete")
        {
            continue;
        }

        if let Some(hop) = parse_windows_hop_line(line) {
            hops.push(hop);
        }
    }

    Ok(hops)
}

#[cfg(windows)]
fn parse_windows_hop_line(line: &str) -> Option<HopSample> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let hop_index = parts.first()?.parse::<u8>().ok()?;

    let mut hop = HopSample::new(hop_index);
    let mut saw_non_timeout = false;
    let mut i = 1;

    while i < parts.len() {
        let part = parts[i];

        if part == "*" {
            hop.stats.add_timeout();
            i += 1;
            continue;
        }

        if i + 1 < parts.len() && parts[i + 1].eq_ignore_ascii_case("ms") {
            let value = part.trim_start_matches('<');
            if let Ok(latency) = value.parse::<f64>() {
                hop.stats.add_sample(latency);
                saw_non_timeout = true;
                i += 2;
                continue;
            }
        }

        if part.ends_with("ms") {
            let value = part.trim_end_matches("ms").trim_start_matches('<');
            if let Ok(latency) = value.parse::<f64>() {
                hop.stats.add_sample(latency);
                saw_non_timeout = true;
                i += 1;
                continue;
            }
        }

        let token = part.trim_matches(|c| matches!(c, '[' | ']' | '(' | ')'));
        if let Ok(ip) = token.parse::<IpAddr>() {
            hop.ip = Some(ip);
        } else if !matches!(token, "Request" | "timed" | "out." | "ms") && hop.hostname.is_none() {
            hop.hostname = Some(token.to_string());
        }

        i += 1;
    }

    if hop.stats.sent == 0 && !saw_non_timeout {
        return None;
    }

    if hop.stats.sent > 0 {
        hop.stats.loss_percent = ((hop.stats.sent.saturating_sub(hop.stats.received)) as f64
            / hop.stats.sent as f64)
            * 100.0;
    }

    hop.status = if hop.stats.received == 0 {
        Severity::Unknown
    } else {
        Severity::Ok
    };

    Some(hop)
}

#[cfg(target_os = "macos")]
fn parse_macos_traceroute_output(output: &str) -> TraceResult<Vec<HopSample>> {
    let mut hops = Vec::new();

    for raw_line in output.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("traceroute to") {
            continue;
        }

        if let Some(hop) = parse_macos_hop_line(line) {
            hops.push(hop);
        }
    }

    Ok(hops)
}

#[cfg(target_os = "macos")]
fn parse_macos_hop_line(line: &str) -> Option<HopSample> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let hop_index = parts.first()?.parse::<u8>().ok()?;

    let mut hop = HopSample::new(hop_index);
    let mut i = 1;

    while i < parts.len() {
        let part = parts[i];

        if part == "*" {
            hop.stats.add_timeout();
            i += 1;
            continue;
        }

        let token = part.trim_matches(|c| matches!(c, '(' | ')'));
        if let Ok(ip) = token.parse::<IpAddr>() {
            hop.ip = Some(ip);
            i += 1;
            continue;
        }

        if i + 1 < parts.len() && parts[i + 1].eq_ignore_ascii_case("ms") {
            if let Ok(latency) = part.parse::<f64>() {
                hop.stats.add_sample(latency);
                i += 2;
                continue;
            }
        }

        if hop.hostname.is_none() {
            hop.hostname = Some(token.to_string());
        }

        i += 1;
    }

    if hop.stats.sent == 0 && hop.ip.is_none() {
        return None;
    }

    if hop.stats.sent > 0 {
        hop.stats.loss_percent = ((hop.stats.sent.saturating_sub(hop.stats.received)) as f64
            / hop.stats.sent as f64)
            * 100.0;
    }

    hop.status = if hop.stats.received == 0 {
        Severity::Unknown
    } else {
        Severity::Ok
    };

    Some(hop)
}
