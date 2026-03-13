//! Portable traceroute implementation using system tracert/ping commands
//! 
//! This version doesn't require raw sockets or admin privileges.
//! It spawns the system's tracert command and parses the output.

use crate::types::*;
use std::process::Command;
use std::str::Lines;
use tracing::{debug, info, warn};

/// Portable traceroute that uses system commands
pub struct PortableTraceRunner {
    config: TraceConfig,
    target_ip: IpAddr,
    hops: Vec<HopSample>,
}

impl PortableTraceRunner {
    pub fn new(session: &TraceSession) -> Result<Self, TraceError> {
        let target_ip = resolve_target(&session.config.target)?;
        
        Ok(Self {
            config: session.config.clone(),
            target_ip,
            hops: Vec::new(),
        })
    }
    
    pub fn target_ip(&self) -> IpAddr {
        self.target_ip
    }
    
    /// Run trace using Windows tracert command
    pub fn run(&mut self) -> Result<Vec<HopSample>, TraceError> {
        info!("Starting portable trace to {}", self.config.target);
        
        #[cfg(windows)]
        return self.run_windows_tracert();
        
        #[cfg(target_os = "macos")]
        return self.run_macos_traceroute();
        
        #[cfg(not(any(windows, target_os = "macos")))]
        Err(TraceError::Internal("Platform not supported".to_string()))
    }
    
    #[cfg(windows)]
    fn run_windows_tracert(&mut self) -> Result<Vec<HopSample>, TraceError> {
        let max_hops = self.config.max_hops;
        let timeout_ms = self.config.timeout_ms;
        let target = &self.config.target;
        
        // Build tracert command
        // tracert -h max_hops -w timeout target
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
            return Err(TraceError::Internal(format!("tracert failed: {}", error_msg)));
        }
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        self.parse_tracert_output(&stdout)
    }
    
    #[cfg(target_os = "macos")]
    fn run_macos_traceroute(&mut self) -> Result<Vec<HopSample>, TraceError> {
        let max_hops = self.config.max_hops;
        let target = &self.config.target;
        
        // Build traceroute command
        let output = Command::new("traceroute")
            .arg("-m")
            .arg(max_hops.to_string())
            .arg("-w")
            .arg((self.config.timeout_ms / 1000).to_string())
            .arg(target)
            .output()
            .map_err(|e| TraceError::Socket(format!("Failed to run traceroute: {}", e)))?;
        
        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(TraceError::Internal(format!("traceroute failed: {}", error_msg)));
        }
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        self.parse_macos_traceroute_output(&stdout)
    }
    
    #[cfg(windows)]
    fn parse_tracert_output(&mut self, output: &str) -> Result<Vec<HopSample>, TraceError> {
        let mut hops: Vec<HopSample> = Vec::new();
        
        for line in output.lines() {
            // Skip empty lines and header
            let line = line.trim();
            if line.is_empty() || line.starts_with("Tracing route") || line.starts_with("over a maximum") {
                continue;
            }
            
            // Parse hop line
            // Format: "1  <1 ms    <1 ms    <1 ms  192.168.1.1"
            // Or: "2     *        *     * Request timed out."
            if let Some(hop) = self.parse_tracert_line(line) {
                hops.push(hop);
            }
        }
        
        Ok(hops)
    }
    
    #[cfg(windows)]
    fn parse_tracert_line(&self, line: &str) -> Option<HopSample> {
        // Split by whitespace
        let parts: Vec<&str> = line.split_whitespace().collect();
        
        if parts.is_empty() {
            return None;
        }
        
        // First element should be hop number
        let hop_index = parts.get(0)?.parse::<u8>().ok()?;
        
        let mut hop = HopSample::new(hop_index);
        
        // Check if all timeouts (* * *)
        let timeout_count = parts.iter().filter(|&&p| p == "*").count();
        
        if timeout_count >= 3 {
            // All requests timed out
            hop.stats.sent = 3;
            hop.stats.received = 0;
            hop.stats.loss_percent = 100.0;
            hop.status = Severity::Unknown;
        } else {
            // Parse IP address and latencies
            for (i, part) in parts.iter().enumerate() {
                if i == 0 {
                    continue; // Skip hop number
                }
                
                // Try to parse as IP address
                if let Ok(ip) = part.parse::<IpAddr>() {
                    if hop.ip.is_none() {
                        hop.ip = Some(ip);
                    }
                }
                
                // Try to parse as latency (ms)
                if part.ends_with("ms") {
                    let latency_str = part.trim_end_matches("ms");
                    if let Ok(latency) = latency_str.parse::<f64>() {
                        hop.stats.add_sample(latency);
                    }
                }
            }
        }
        
        Some(hop)
    }
    
    #[cfg(target_os = "macos")]
    fn parse_macos_traceroute_output(&mut self, output: &str) -> Result<Vec<HopSample>, TraceError> {
        let mut hops: Vec<HopSample> = Vec::new();
        
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("traceroute to") {
                continue;
            }
            
            if let Some(hop) = self.parse_macos_traceroute_line(line) {
                hops.push(hop);
            }
        }
        
        Ok(hops)
    }
    
    #[cfg(target_os = "macos")]
    fn parse_macos_traceroute_line(&self, line: &str) -> Option<HopSample> {
        // Format: "1  192.168.1.1 (192.168.1.1)  0.543 ms  0.456 ms  0.321 ms"
        let parts: Vec<&str> = line.split_whitespace().collect();
        
        if parts.is_empty() {
            return None;
        }
        
        let hop_index = parts.get(0)?.parse::<u8>().ok()?;
        let mut hop = HopSample::new(hop_index);
        
        for (i, part) in parts.iter().enumerate() {
            if i == 0 {
                continue;
            }
            
            // Parse IP address (may be in parentheses)
            if let Ok(ip) = part.trim_matches('(').trim_matches(')').parse::<IpAddr>() {
                if hop.ip.is_none() {
                    hop.ip = Some(ip);
                }
            }
            
            // Parse latency
            if part.ends_with("ms") {
                let latency_str = part.trim_end_matches("ms");
                if let Ok(latency) = latency_str.parse::<f64>() {
                    hop.stats.add_sample(latency);
                }
            }
        }
        
        Some(hop)
    }
    
    /// Get current hop data
    pub fn get_hops(&self) -> Vec<HopSample> {
        self.hops.clone()
    }
}
