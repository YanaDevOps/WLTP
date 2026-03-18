//! Traceroute implementation for Windows and macOS.
//!
//! Windows uses hidden system tools to provide a WinMTR-like in-app loop:
//! route discovery is done once, then each discovered hop is probed repeatedly
//! with live event updates. macOS keeps a simpler fallback path for now.

use crate::types::*;
use std::net::IpAddr;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tracing::info;

#[cfg(windows)]
use std::io::{BufRead, BufReader};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
    hops: Arc<Mutex<Vec<HopSample>>>,
    running: Arc<AtomicBool>,
    session_id: String,
}

impl TraceRunner {
    pub fn new(session: &TraceSession) -> TraceResult<Self> {
        let target_ip = resolve_target(&session.config.target)?;

        Ok(Self {
            config: session.config.clone(),
            target_ip,
            hops: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(false)),
            session_id: session.id.clone(),
        })
    }

    /// Get the resolved target IP.
    pub fn target_ip(&self) -> IpAddr {
        self.target_ip
    }

    /// Shared hop storage for session access.
    pub fn hops_handle(&self) -> Arc<Mutex<Vec<HopSample>>> {
        self.hops.clone()
    }

    /// Shared cancellation flag for stop requests.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// Stop the trace.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    /// Get current hop data.
    pub fn get_hops(&self) -> Vec<HopSample> {
        self.hops.lock().unwrap().clone()
    }

    /// Run the trace with live event updates.
    pub async fn run(&mut self, event_tx: mpsc::Sender<TraceEvent>) -> TraceResult<()> {
        if self.running.swap(true, Ordering::Relaxed) {
            return Err(TraceError::AlreadyRunning);
        }

        #[cfg(windows)]
        let result = self.run_windows(event_tx).await;

        #[cfg(target_os = "macos")]
        let result = self.run_macos(event_tx).await;

        #[cfg(not(any(windows, target_os = "macos")))]
        let result = Err(TraceError::Internal("Platform not supported".to_string()));

        self.running.store(false, Ordering::Relaxed);
        result
    }

    #[cfg(windows)]
    async fn run_windows(&mut self, event_tx: mpsc::Sender<TraceEvent>) -> TraceResult<()> {
        let session_id = self.session_id.clone();
        let target = self.config.target.clone();
        let max_hops = self.config.max_hops;
        let timeout_ms = self.config.timeout_ms;
        let running = self.running.clone();
        let hops_handle = self.hops.clone();
        let discovery_tx = event_tx.clone();

        let discovered_hops = tokio::task::spawn_blocking(move || {
            discover_windows_route(
                &session_id,
                &target,
                max_hops,
                timeout_ms,
                running,
                hops_handle,
                discovery_tx,
            )
        })
        .await
        .map_err(|e| TraceError::Internal(format!("Route discovery task failed: {}", e)))??;

        if discovered_hops.is_empty() {
            return Err(TraceError::Internal(
                "Could not discover any hops for the route".to_string(),
            ));
        }

        let max_cycles = if self.config.count == 0 {
            None
        } else {
            Some(self.config.count)
        };
        let cycle_delay = Duration::from_millis(self.config.interval_ms.max(250));
        let mut cycle = 0_u32;
        let mut hops = discovered_hops;

        while self.running.load(Ordering::Relaxed) {
            if let Some(limit) = max_cycles {
                if cycle >= limit {
                    break;
                }
            }

            for index in 0..hops.len() {
                if !self.running.load(Ordering::Relaxed) {
                    break;
                }

                let (hop_index, hop_stats, probe_result) = {
                    let hop = &mut hops[index];
                    let probe_result = probe_windows_hop(hop, timeout_ms)?;
                    (hop.index, hop.stats.clone(), probe_result)
                };

                sync_hops(&self.hops, &hops);

                match probe_result {
                    Some(latency_ms) => {
                        let _ = event_tx
                            .send(TraceEvent::HopResponse {
                                session_id: self.session_id.clone(),
                                hop_index,
                                latency_ms,
                            })
                            .await;
                    }
                    None => {
                        let _ = event_tx
                            .send(TraceEvent::HopTimeout {
                                session_id: self.session_id.clone(),
                                hop_index,
                            })
                            .await;
                    }
                }

                let _ = event_tx
                    .send(TraceEvent::HopStatsUpdate {
                        session_id: self.session_id.clone(),
                        hop_index,
                        stats: hop_stats,
                    })
                    .await;
            }

            cycle += 1;

            if self.running.load(Ordering::Relaxed) {
                sleep(cycle_delay).await;
            }
        }

        sync_hops(&self.hops, &hops);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    async fn run_macos(&mut self, event_tx: mpsc::Sender<TraceEvent>) -> TraceResult<()> {
        let target = self.config.target.clone();
        let max_hops = self.config.max_hops;
        let timeout_ms = self.config.timeout_ms;

        let hops = tokio::task::spawn_blocking(move || {
            run_macos_traceroute(&target, max_hops, timeout_ms)
        })
        .await
        .map_err(|e| TraceError::Internal(format!("Traceroute task failed: {}", e)))??;

        sync_hops(&self.hops, &hops);

        for hop in &hops {
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

        Ok(())
    }
}

fn sync_hops(target: &Arc<Mutex<Vec<HopSample>>>, hops: &[HopSample]) {
    *target.lock().unwrap() = hops.to_vec();
}

#[cfg(windows)]
fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(windows)]
fn discover_windows_route(
    session_id: &str,
    target: &str,
    max_hops: u8,
    timeout_ms: u64,
    running: Arc<AtomicBool>,
    hops_handle: Arc<Mutex<Vec<HopSample>>>,
    event_tx: mpsc::Sender<TraceEvent>,
) -> TraceResult<Vec<HopSample>> {
    info!("Discovering route to {}", target);

    let mut child = hidden_command("tracert");
    child
        .arg("-d")
        .arg("-h")
        .arg(max_hops.to_string())
        .arg("-w")
        .arg(timeout_ms.to_string())
        .arg(target)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = child
        .spawn()
        .map_err(|e| TraceError::Socket(format!("Failed to start tracert: {}", e)))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| TraceError::Internal("Failed to capture tracert stdout".to_string()))?;

    let reader = BufReader::new(stdout);
    let mut hops = Vec::new();

    for line in reader.lines() {
        let line =
            line.map_err(|e| TraceError::Socket(format!("Failed to read tracert output: {}", e)))?;

        if !running.load(Ordering::Relaxed) {
            let _ = child.kill();
            break;
        }

        if let Some(hop) = parse_windows_route_line(&line) {
            let is_new = hops
                .iter()
                .all(|existing: &HopSample| existing.index != hop.index);
            if is_new {
                hops.push(hop.clone());
                sync_hops(&hops_handle, &hops);
                let _ = event_tx.blocking_send(TraceEvent::HopDiscovered {
                    session_id: session_id.to_string(),
                    hop,
                });
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| TraceError::Socket(format!("Failed to wait for tracert: {}", e)))?;

    if !status.success() && hops.is_empty() && running.load(Ordering::Relaxed) {
        return Err(TraceError::Internal(
            "tracert did not return any usable hop information".to_string(),
        ));
    }

    Ok(hops)
}

#[cfg(windows)]
fn parse_windows_route_line(line: &str) -> Option<HopSample> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("Tracing route")
        || trimmed.starts_with("over a maximum")
        || trimmed.starts_with("Trace complete")
    {
        return None;
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let hop_index = parts.first()?.parse::<u8>().ok()?;
    let mut hop = HopSample::new(hop_index);

    for token in parts.iter().skip(1) {
        let clean = token.trim_matches(|c| matches!(c, '[' | ']' | '(' | ')'));
        if let Ok(ip) = clean.parse::<IpAddr>() {
            hop.ip = Some(ip);
        } else if !matches!(clean, "*" | "ms" | "Request" | "timed" | "out.")
            && hop.hostname.is_none()
        {
            hop.hostname = Some(clean.to_string());
        }
    }

    Some(hop)
}

#[cfg(windows)]
fn probe_windows_hop(hop: &mut HopSample, timeout_ms: u64) -> TraceResult<Option<f64>> {
    let probe = match hop.ip {
        Some(ip) => ping_windows_ip(ip, timeout_ms)?,
        None => None,
    };

    match probe {
        Some(latency_ms) => {
            hop.stats.add_sample(latency_ms);
            hop.status = Severity::Ok;
            Ok(Some(latency_ms))
        }
        None => {
            hop.stats.add_timeout();
            hop.status = Severity::Unknown;
            Ok(None)
        }
    }
}

#[cfg(windows)]
fn ping_windows_ip(ip: IpAddr, timeout_ms: u64) -> TraceResult<Option<f64>> {
    let mut command = hidden_command("ping");
    command
        .arg("-n")
        .arg("1")
        .arg("-w")
        .arg(timeout_ms.to_string())
        .arg(ip.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = command
        .output()
        .map_err(|e| TraceError::Socket(format!("Failed to run ping: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_windows_ping_latency(&stdout))
}

#[cfg(windows)]
fn parse_windows_ping_latency(output: &str) -> Option<f64> {
    for raw_line in output.lines() {
        let line = raw_line.trim();
        if line.contains("Request timed out.") || line.contains("Destination host unreachable") {
            return None;
        }

        if let Some(value) = extract_ping_time(line, "time=") {
            return Some(value);
        }

        if let Some(value) = extract_ping_time(line, "time<") {
            return Some(value);
        }
    }

    None
}

#[cfg(windows)]
fn extract_ping_time(line: &str, marker: &str) -> Option<f64> {
    let rest = line.split(marker).nth(1)?;
    let token = rest.split_whitespace().next()?;
    let value = token.trim_end_matches("ms").trim_start_matches('<');
    value.parse::<f64>().ok()
}

#[cfg(target_os = "macos")]
fn run_macos_traceroute(
    target: &str,
    max_hops: u8,
    timeout_ms: u64,
) -> TraceResult<Vec<HopSample>> {
    info!("Starting macOS traceroute to {}", target);

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

    parse_macos_traceroute_output(&String::from_utf8_lossy(&output.stdout))
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
