//! Interpretation engine for trace results
//! 
//! This module implements a rule-based engine that analyzes hop statistics
//! and provides human-readable interpretations of network issues.

use crate::types::*;
use std::net::IpAddr;

/// Interpretation rules engine
pub struct InterpretationEngine {
    /// Loss threshold for warning (percentage)
    loss_warning_threshold: f64,
    /// Loss threshold for critical (percentage)
    loss_critical_threshold: f64,
    /// Latency threshold for warning (ms)
    latency_warning_threshold: f64,
    /// Latency threshold for critical (ms)
    latency_critical_threshold: f64,
    /// Jitter threshold for warning (ms)
    jitter_warning_threshold: f64,
}

impl Default for InterpretationEngine {
    fn default() -> Self {
        Self {
            loss_warning_threshold: 5.0,
            loss_critical_threshold: 20.0,
            latency_warning_threshold: 100.0,
            latency_critical_threshold: 300.0,
            jitter_warning_threshold: 30.0,
        }
    }
}

impl InterpretationEngine {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Interpret a single hop based on its statistics and context
    pub fn interpret_hop(
        &self,
        hop: &HopSample,
        is_destination: bool,
        next_hops: &[&HopSample],
    ) -> HopInterpretation {
        let stats = &hop.stats;
        
        // No responses at all
        if stats.received == 0 && stats.sent > 3 {
            return self.interpret_no_response(hop, is_destination, next_hops);
        }
        
        // Check for loss
        if stats.loss_percent > self.loss_warning_threshold {
            return self.interpret_loss(hop, is_destination, next_hops);
        }
        
        // Check for high latency
        if let Some(avg) = stats.avg_ms {
            if avg > self.latency_warning_threshold {
                return self.interpret_high_latency(hop, is_destination, next_hops);
            }
        }
        
        // Check for high jitter
        if let Some(jitter) = stats.jitter_ms {
            if jitter > self.jitter_warning_threshold {
                return self.interpret_high_jitter(hop, is_destination);
            }
        }
        
        // Everything looks good
        self.interpret_ok(hop, is_destination)
    }
    
    fn interpret_no_response(
        &self,
        hop: &HopSample,
        is_destination: bool,
        next_hops: &[&HopSample],
    ) -> HopInterpretation {
        // Check if subsequent hops are responding
        let later_hops_ok = next_hops.iter().any(|h| h.stats.received > 0 && h.stats.loss_percent < 10.0);
        
        if is_destination {
            HopInterpretation {
                severity: Severity::Critical,
                headline: "Destination not responding".to_string(),
                explanation: "The target server is not responding to ICMP (ping) requests. This could indicate the server is down, a firewall is blocking ICMP, or there's a network issue at the destination.".to_string(),
                probable_causes: vec![
                    "Server is down or unreachable".to_string(),
                    "Firewall blocking ICMP traffic".to_string(),
                    "Network outage at destination".to_string(),
                ],
                confidence: 0.9,
            }
        } else if later_hops_ok {
            HopInterpretation {
                severity: Severity::Unknown,
                headline: "Hop not responding (may be normal)".to_string(),
                explanation: "This intermediate router is not responding to ICMP requests, but traffic is still reaching later hops. Many routers are configured to deprioritize or block ICMP responses while continuing to forward traffic normally.".to_string(),
                probable_causes: vec![
                    "Router configured to rate-limit or block ICMP".to_string(),
                    "Router prioritizing traffic over management responses".to_string(),
                    "ICMP filtering at this hop".to_string(),
                ],
                confidence: 0.85,
            }
        } else {
            HopInterpretation {
                severity: Severity::Critical,
                headline: "Connection lost at this hop".to_string(),
                explanation: "Network connectivity is being lost at or before this hop, and subsequent hops are also not responding. This suggests a real connectivity issue rather than ICMP filtering.".to_string(),
                probable_causes: vec![
                    "Network outage or hardware failure".to_string(),
                    "Routing misconfiguration".to_string(),
                    "Severe congestion causing packet drops".to_string(),
                ],
                confidence: 0.75,
            }
        }
    }
    
    fn interpret_loss(
        &self,
        hop: &HopSample,
        is_destination: bool,
        next_hops: &[&HopSample],
    ) -> HopInterpretation {
        let loss_percent = hop.stats.loss_percent;
        
        // Check if loss continues to subsequent hops
        let loss_continues = next_hops.iter().any(|h| h.stats.loss_percent > self.loss_warning_threshold);
        
        // Check if this is likely ICMP rate limiting
        let likely_rate_limited = !is_destination && !loss_continues && next_hops.iter().all(|h| h.stats.loss_percent < 5.0);
        
        if likely_rate_limited {
            HopInterpretation {
                severity: Severity::Warning,
                headline: format!("{:.0}% packet loss (likely rate-limiting)", loss_percent),
                explanation: "This hop shows packet loss, but subsequent hops and the destination are responding normally. This is typically caused by ICMP rate limiting, where the router deliberately slows down its responses to prevent overload.".to_string(),
                probable_causes: vec![
                    "ICMP rate limiting on router".to_string(),
                    "Router CPU/memory constraints".to_string(),
                    "Low priority for control plane traffic".to_string(),
                ],
                confidence: 0.8,
            }
        } else if is_destination {
            let severity = if loss_percent > self.loss_critical_threshold {
                Severity::Critical
            } else {
                Severity::Warning
            };
            
            HopInterpretation {
                severity,
                headline: format!("{:.0}% packet loss to destination", loss_percent),
                explanation: "The target server is experiencing significant packet loss. This indicates a real connectivity issue that will affect application performance.".to_string(),
                probable_causes: vec![
                    "Network congestion between you and the server".to_string(),
                    "Server overload or capacity issues".to_string(),
                    "Unstable network connection".to_string(),
                    "ISP routing problems".to_string(),
                ],
                confidence: 0.9,
            }
        } else if loss_continues {
            HopInterpretation {
                severity: Severity::Warning,
                headline: format!("{:.0}% packet loss starting here", loss_percent),
                explanation: "Packet loss begins at this hop and continues to subsequent hops. This suggests a genuine network issue at this point in the route.".to_string(),
                probable_causes: vec![
                    "Network congestion at this segment".to_string(),
                    "Hardware issue at this router".to_string(),
                    "Link capacity exceeded".to_string(),
                    "ISP peering issues".to_string(),
                ],
                confidence: 0.75,
            }
        } else {
            HopInterpretation {
                severity: Severity::Warning,
                headline: format!("{:.0}% packet loss at intermediate hop", loss_percent),
                explanation: "This intermediate hop shows packet loss, but subsequent hops appear normal. This could be due to ICMP deprioritization rather than actual traffic loss.".to_string(),
                probable_causes: vec![
                    "ICMP rate limiting".to_string(),
                    "Temporary congestion".to_string(),
                    "Router load balancing (asymmetric routing)".to_string(),
                ],
                confidence: 0.6,
            }
        }
    }
    
    fn interpret_high_latency(
        &self,
        hop: &HopSample,
        is_destination: bool,
        next_hops: &[&HopSample],
    ) -> HopInterpretation {
        let avg = hop.stats.avg_ms.unwrap_or(0.0);
        let is_critical = avg > self.latency_critical_threshold;
        
        // Check if latency increase started at this hop
        let latency_increase_here = if let Some(prev_hop) = next_hops.first() {
            if let Some(prev_avg) = prev_hop.stats.avg_ms {
                avg > prev_avg + 50.0 // Significant increase
            } else {
                true
            }
        } else {
            false
        };
        
        // Check if latency continues to destination
        let latency_continues = next_hops.iter().all(|h| {
            h.stats.avg_ms.map(|a| a > self.latency_warning_threshold).unwrap_or(false)
        });
        
        let severity = if is_critical {
            Severity::Critical
        } else {
            Severity::Warning
        };
        
        if is_destination {
            HopInterpretation {
                severity,
                headline: format!("High latency: {:.0}ms average", avg),
                explanation: "The destination server is responding with high latency. This will cause noticeable delays in applications and may indicate server load or network issues.".to_string(),
                probable_causes: vec![
                    "Server processing delay or overload".to_string(),
                    "Long geographic distance to server".to_string(),
                    "Network congestion on final mile".to_string(),
                ],
                confidence: 0.85,
            }
        } else if latency_increase_here && latency_continues {
            HopInterpretation {
                severity,
                headline: format!("Latency spike at this hop: {:.0}ms", avg),
                explanation: "A significant increase in latency begins at this hop and continues to the destination. This identifies the network segment where delays are being introduced.".to_string(),
                probable_causes: vec![
                    "Congested network link at this segment".to_string(),
                    "Long-distance link (crossing continents/oceans)".to_string(),
                    "Over-subscribed bandwidth at ISP peering point".to_string(),
                    "VPN or tunnel encapsulation overhead".to_string(),
                ],
                confidence: 0.8,
            }
        } else {
            HopInterpretation {
                severity: Severity::Warning,
                headline: format!("Elevated latency: {:.0}ms", avg),
                explanation: "This hop shows higher than optimal latency. If this is an intermediate hop with normal latency at the destination, it may be due to ICMP deprioritization.".to_string(),
                probable_causes: vec![
                    "Router control plane delay".to_string(),
                    "ICMP processing overhead".to_string(),
                    "Normal for this network segment".to_string(),
                ],
                confidence: 0.6,
            }
        }
    }
    
    fn interpret_high_jitter(&self, hop: &HopSample, is_destination: bool) -> HopInterpretation {
        let jitter = hop.stats.jitter_ms.unwrap_or(0.0);
        
        if is_destination {
            HopInterpretation {
                severity: Severity::Warning,
                headline: format!("High jitter: {:.0}ms variation", jitter),
                explanation: "The connection to the destination has high latency variation (jitter). This can cause problems for real-time applications like VoIP, video calls, and gaming, even if average latency is acceptable.".to_string(),
                probable_causes: vec![
                    "Network congestion causing variable queue times".to_string(),
                    "Bufferbloat on router/modem".to_string(),
                    "Intermittent interference (wireless)".to_string(),
                    "ISP traffic shaping".to_string(),
                ],
                confidence: 0.8,
            }
        } else {
            HopInterpretation {
                severity: Severity::Warning,
                headline: format!("High jitter detected: {:.0}ms", jitter),
                explanation: "This hop shows significant latency variation. Jitter at intermediate hops may indicate congestion or variable routing, but only matters if it affects the destination.".to_string(),
                probable_causes: vec![
                    "Variable ICMP processing time".to_string(),
                    "Network congestion bursts".to_string(),
                    "Load balancing across multiple paths".to_string(),
                ],
                confidence: 0.6,
            }
        }
    }
    
    fn interpret_ok(&self, hop: &HopSample, is_destination: bool) -> HopInterpretation {
        let avg = hop.stats.avg_ms.map(|a| format!("{:.0}ms", a)).unwrap_or_else(|| "N/A".to_string());
        
        if is_destination {
            HopInterpretation {
                severity: Severity::Ok,
                headline: format!("Destination responding normally ({})", avg),
                explanation: "The target server is responding with healthy latency and no packet loss. The network path appears to be functioning correctly.".to_string(),
                probable_causes: vec![],
                confidence: 0.9,
            }
        } else {
            HopInterpretation {
                severity: Severity::Ok,
                headline: format!("Healthy ({})", avg),
                explanation: "This hop is responding normally with acceptable latency and no significant packet loss.".to_string(),
                probable_causes: vec![],
                confidence: 0.85,
            }
        }
    }
    
    /// Generate an overall session summary
    pub fn generate_summary(&self, hops: &[HopSample]) -> SessionSummary {
        if hops.is_empty() {
            return SessionSummary {
                overall_status: Severity::Unknown,
                primary_finding: "No trace data available".to_string(),
                secondary_findings: vec!["The trace did not complete or no hops were discovered".to_string()],
                recommended_next_steps: vec!["Try running the trace again".to_string()],
                problem_hop_index: None,
                destination_reachable: false,
            };
        }
        
        // Find destination hop (highest index)
        let destination = hops.iter().max_by_key(|h| h.index);
        let destination_reachable = destination
            .map(|d| d.stats.received > 0 && d.stats.loss_percent < 50.0)
            .unwrap_or(false);
        
        // Find the first problematic hop
        let problem_hop = hops.iter().find(|h| {
            matches!(h.status, Severity::Warning | Severity::Critical) &&
            h.stats.received > 0 // Skip hops that just didn't respond
        });
        
        // Analyze overall patterns
        let mut findings: Vec<String> = Vec::new();
        let mut recommendations: Vec<String> = Vec::new();
        
        // Check destination status
        if let Some(dest) = destination {
            if dest.stats.received == 0 {
                findings.push("Destination is not responding to ICMP requests".to_string());
                recommendations.push("Verify the destination address is correct".to_string());
                recommendations.push("The server may be down or blocking ICMP".to_string());
            } else if dest.stats.loss_percent > self.loss_critical_threshold {
                findings.push(format!("High packet loss at destination: {:.0}%", dest.stats.loss_percent));
                recommendations.push("Contact your ISP or the destination server administrator".to_string());
            } else if dest.stats.loss_percent > self.loss_warning_threshold {
                findings.push(format!("Moderate packet loss at destination: {:.0}%", dest.stats.loss_percent));
            }
            
            if let Some(avg) = dest.stats.avg_ms {
                if avg > self.latency_critical_threshold {
                    findings.push(format!("Very high latency to destination: {:.0}ms", avg));
                } else if avg > self.latency_warning_threshold {
                    findings.push(format!("Elevated latency to destination: {:.0}ms", avg));
                }
            }
            
            if let Some(jitter) = dest.stats.jitter_ms {
                if jitter > self.jitter_warning_threshold {
                    findings.push(format!("High jitter at destination: {:.0}ms", jitter));
                    recommendations.push("For VoIP/gaming issues, check for bufferbloat on your router".to_string());
                }
            }
        }
        
        // Check for intermediate issues that propagate
        let mut loss_start: Option<u8> = None;
        let mut latency_start: Option<u8> = None;
        
        for hop in hops.iter().take(hops.len().saturating_sub(1)) { // Exclude destination
            if loss_start.is_none() && hop.stats.loss_percent > self.loss_warning_threshold {
                // Check if loss continues
                let continues = hops.iter()
                    .filter(|h| h.index > hop.index)
                    .any(|h| h.stats.loss_percent > self.loss_warning_threshold);
                if continues {
                    loss_start = Some(hop.index);
                }
            }
            
            if latency_start.is_none() {
                if let Some(avg) = hop.stats.avg_ms {
                    if avg > self.latency_warning_threshold {
                        let continues = hops.iter()
                            .filter(|h| h.index > hop.index)
                            .any(|h| h.stats.avg_ms.unwrap_or(0.0) > self.latency_warning_threshold);
                        if continues {
                            latency_start = Some(hop.index);
                        }
                    }
                }
            }
        }
        
        // Identify ISP/local network hops (typically first few)
        let first_hop = hops.iter().find(|h| h.index == 1);
        if let Some(hop) = first_hop {
            if hop.stats.loss_percent > self.loss_warning_threshold {
                findings.push("Issues detected starting at your local network or ISP".to_string());
                recommendations.push("Check your local network equipment (router, cables)".to_string());
                recommendations.push("Restart your router/modem".to_string());
            }
        }
        
        // Determine overall status
        let overall_status = if !destination_reachable {
            Severity::Critical
        } else if let Some(problem) = problem_hop {
            problem.status
        } else {
            Severity::Ok
        };
        
        // Generate primary finding
        let primary_finding = if !destination_reachable {
            "Destination unreachable".to_string()
        } else if let Some(loss_idx) = loss_start {
            format!("Packet loss begins at hop {}", loss_idx)
        } else if let Some(lat_idx) = latency_start {
            format!("Latency increase begins at hop {}", lat_idx)
        } else if let Some(problem) = problem_hop {
            problem.interpretation.as_ref()
                .map(|i| i.headline.clone())
                .unwrap_or_else(|| "Some issues detected".to_string())
        } else {
            "Connection looks stable".to_string()
        };
        
        // Add secondary findings
        if findings.is_empty() {
            findings.push("No significant issues detected along the route".to_string());
        }
        
        // Add default recommendations if none
        if recommendations.is_empty() {
            if overall_status == Severity::Ok {
                recommendations.push("No action needed - connection is healthy".to_string());
            } else {
                recommendations.push("Monitor the connection for changes".to_string());
                recommendations.push("Share this report with technical support if issues persist".to_string());
            }
        }
        
        SessionSummary {
            overall_status,
            primary_finding,
            secondary_findings: findings,
            recommended_next_steps: recommendations,
            problem_hop_index: problem_hop.map(|h| h.index),
            destination_reachable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn make_hop(index: u8, sent: u32, received: u32, avg_ms: Option<f64>, loss_percent: f64) -> HopSample {
        let mut hop = HopSample::new(index);
        hop.stats.sent = sent;
        hop.stats.received = received;
        hop.stats.avg_ms = avg_ms;
        hop.stats.loss_percent = loss_percent;
        hop
    }
    
    #[test]
    fn test_interpret_ok() {
        let engine = InterpretationEngine::new();
        let mut hop = HopSample::new(5);
        hop.stats.add_sample(20.0);
        hop.stats.add_sample(25.0);
        hop.stats.add_sample(22.0);
        hop.ip = Some("192.168.1.1".parse().unwrap());
        
        let interpretation = engine.interpret_hop(&hop, false, &[]);
        
        assert_eq!(interpretation.severity, Severity::Ok);
        assert!(interpretation.headline.contains("Healthy"));
    }
    
    #[test]
    fn test_interpret_loss_at_destination() {
        let engine = InterpretationEngine::new();
        let hop = make_hop(10, 100, 70, Some(50.0), 30.0);
        
        let interpretation = engine.interpret_hop(&hop, true, &[]);
        
        assert!(matches!(interpretation.severity, Severity::Critical | Severity::Warning));
        assert!(interpretation.headline.contains("30% packet loss"));
    }
    
    #[test]
    fn test_interpret_rate_limited() {
        let engine = InterpretationEngine::new();
        let hop = make_hop(5, 100, 80, Some(20.0), 20.0);
        let next_hop = make_hop(6, 100, 99, Some(22.0), 1.0);
        
        let interpretation = engine.interpret_hop(&hop, false, &[&next_hop]);
        
        assert_eq!(interpretation.severity, Severity::Warning);
        assert!(interpretation.headline.contains("rate-limiting"));
    }
    
    #[test]
    fn test_generate_summary_ok() {
        let engine = InterpretationEngine::new();
        
        let mut hops = vec![
            make_hop(1, 10, 10, Some(5.0), 0.0),
            make_hop(2, 10, 10, Some(15.0), 0.0),
            make_hop(3, 10, 10, Some(25.0), 0.0),
        ];
        
        // Add interpretations
        for hop in &mut hops {
            hop.interpretation = Some(engine.interpret_hop(hop, hop.index == 3, &[]));
            hop.status = hop.interpretation.as_ref().unwrap().severity;
        }
        
        let summary = engine.generate_summary(&hops);
        
        assert_eq!(summary.overall_status, Severity::Ok);
        assert!(summary.destination_reachable);
    }
}
