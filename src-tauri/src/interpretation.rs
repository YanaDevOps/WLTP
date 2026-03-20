//! Interpretation engine for trace results
//!
//! This module implements a rule-based engine that analyzes hop statistics
//! and provides human-readable interpretations of network issues.

use crate::commands::{ExplanationLevel, Language};
use crate::types::*;

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

    pub fn annotate_hops(
        &self,
        hops: &[HopSample],
        level: ExplanationLevel,
        language: Language,
    ) -> Vec<HopSample> {
        hops.iter()
            .enumerate()
            .map(|(index, hop)| {
                let is_destination = index == hops.len().saturating_sub(1);
                let next_hops: Vec<&HopSample> = hops.iter().skip(index + 1).collect();
                let mut hop = hop.clone();
                let interpretation =
                    self.interpret_hop_with_level(&hop, is_destination, &next_hops, level.clone());
                hop.interpretation =
                    Some(self.localize_hop_interpretation(interpretation, language.clone()));
                if let Some(interpretation) = &hop.interpretation {
                    hop.status = interpretation.severity;
                }
                hop
            })
            .collect()
    }

    /// Interpret a single hop based on its statistics and context
    pub fn interpret_hop(
        &self,
        hop: &HopSample,
        is_destination: bool,
        next_hops: &[&HopSample],
    ) -> HopInterpretation {
        self.interpret_hop_with_level(hop, is_destination, next_hops, ExplanationLevel::Detailed)
    }

    pub fn interpret_hop_with_level(
        &self,
        hop: &HopSample,
        is_destination: bool,
        next_hops: &[&HopSample],
        level: ExplanationLevel,
    ) -> HopInterpretation {
        let stats = &hop.stats;

        // No responses at all
        if stats.received == 0 && stats.sent >= 3 {
            return self.interpret_no_response(hop, is_destination, next_hops, level);
        }

        // Check for loss
        if stats.loss_percent > self.loss_warning_threshold {
            return self.interpret_loss(hop, is_destination, next_hops, level);
        }

        // Check for high latency
        if let Some(avg) = stats.avg_ms {
            if avg > self.latency_warning_threshold {
                return self.interpret_high_latency(hop, is_destination, next_hops, level);
            }
        }

        // Check for high jitter
        if let Some(jitter) = stats.jitter_ms {
            if jitter > self.jitter_warning_threshold {
                return self.interpret_high_jitter(hop, is_destination, level);
            }
        }

        // Everything looks good
        self.interpret_ok(hop, is_destination, level)
    }

    fn interpret_no_response(
        &self,
        _hop: &HopSample,
        is_destination: bool,
        next_hops: &[&HopSample],
        level: ExplanationLevel,
    ) -> HopInterpretation {
        // Check if subsequent hops are responding
        let later_hops_ok = next_hops
            .iter()
            .any(|h| h.stats.received > 0 && h.stats.loss_percent < 10.0);

        if is_destination {
            self.message(
                level,
                Severity::Critical,
                "Target is not replying",
                "This address did not answer ping at all. Usually that means the host is down, blocks ping, or the route cannot reach it.",
                "Destination not responding",
                "The target server is not responding to ICMP (ping) requests. This could indicate the server is down, a firewall is blocking ICMP, or there's a network issue at the destination.",
                vec![
                    "The server may be offline".to_string(),
                    "A firewall may be blocking ping replies".to_string(),
                    "The route may not be reaching the destination".to_string(),
                ],
                0.9,
            )
        } else if later_hops_ok {
            self.message(
                level,
                Severity::Unknown,
                "This hop ignores ping replies",
                "This router is not answering, but later hops still reply. That usually means the router hides ping responses and is not the real problem.",
                "Hop not responding (may be normal)",
                "This intermediate router is not responding to ICMP requests, but traffic is still reaching later hops. Many routers are configured to deprioritize or block ICMP responses while continuing to forward traffic normally.",
                vec![
                    "The router may be rate-limiting ping".to_string(),
                    "This device may deprioritize control traffic".to_string(),
                    "ICMP could be filtered at this hop".to_string(),
                ],
                0.85,
            )
        } else {
            self.message(
                level,
                Severity::Critical,
                "Traffic likely stops here",
                "This hop and the ones after it are not replying. That usually means the route breaks at this point or just before it.",
                "Connection lost at this hop",
                "Network connectivity is being lost at or before this hop, and subsequent hops are also not responding. This suggests a real connectivity issue rather than ICMP filtering.",
                vec![
                    "A router or link may be down".to_string(),
                    "Routing may be broken at this point".to_string(),
                    "Heavy congestion may be dropping packets".to_string(),
                ],
                0.75,
            )
        }
    }

    fn interpret_loss(
        &self,
        hop: &HopSample,
        is_destination: bool,
        next_hops: &[&HopSample],
        level: ExplanationLevel,
    ) -> HopInterpretation {
        let loss_percent = hop.stats.loss_percent;

        // Check if loss continues to subsequent hops
        let loss_continues = next_hops
            .iter()
            .any(|h| h.stats.loss_percent > self.loss_warning_threshold);

        // Check if this is likely ICMP rate limiting
        let likely_rate_limited = !is_destination
            && !loss_continues
            && next_hops.iter().all(|h| h.stats.loss_percent < 5.0);

        if likely_rate_limited {
            self.message(
                level,
                Severity::Warning,
                &format!("{:.0}% loss here is probably harmless", loss_percent),
                "This hop drops ping replies, but later hops are healthy. Usually the router is limiting ping responses rather than dropping real traffic.",
                &format!("{:.0}% packet loss (likely rate-limiting)", loss_percent),
                "This hop shows packet loss, but subsequent hops and the destination are responding normally. This is typically caused by ICMP rate limiting, where the router deliberately slows down its responses to prevent overload.",
                vec![
                    "The router may be rate-limiting ICMP".to_string(),
                    "This device may give low priority to ping replies".to_string(),
                    "The control plane could be busy even while forwarding stays normal".to_string(),
                ],
                0.8,
            )
        } else if is_destination {
            let severity = if loss_percent > self.loss_critical_threshold {
                Severity::Critical
            } else {
                Severity::Warning
            };

            self.message(
                level,
                severity,
                &format!("{:.0}% packet loss to the target", loss_percent),
                "Some packets reach the target and some do not. Apps may feel slow, disconnect, or retry.",
                &format!("{:.0}% packet loss to destination", loss_percent),
                "The target server is experiencing significant packet loss. This indicates a real connectivity issue that will affect application performance.",
                vec![
                    "There may be congestion between you and the target".to_string(),
                    "The target may be overloaded".to_string(),
                    "Your connection may be unstable".to_string(),
                    "An ISP or routing issue may be involved".to_string(),
                ],
                0.9,
            )
        } else if loss_continues {
            self.message(
                level,
                Severity::Warning,
                &format!("{:.0}% loss starts at this hop", loss_percent),
                "Packet loss begins here and keeps showing up later. This usually points to a real problem on this link or router.",
                &format!("{:.0}% packet loss starting here", loss_percent),
                "Packet loss begins at this hop and continues to subsequent hops. This suggests a genuine network issue at this point in the route.",
                vec![
                    "There may be congestion on this segment".to_string(),
                    "This router or link may have a fault".to_string(),
                    "Link capacity may be saturated".to_string(),
                    "ISP peering could be unstable".to_string(),
                ],
                0.75,
            )
        } else {
            self.message(
                level,
                Severity::Warning,
                &format!("{:.0}% loss only on this router", loss_percent),
                "Later hops look normal, so this is often ping-reply behavior rather than a real end-to-end loss problem.",
                &format!("{:.0}% packet loss at intermediate hop", loss_percent),
                "This intermediate hop shows packet loss, but subsequent hops appear normal. This could be due to ICMP deprioritization rather than actual traffic loss.",
                vec![
                    "ICMP may be rate-limited here".to_string(),
                    "There may be temporary congestion".to_string(),
                    "Load balancing can make traceroute look uneven".to_string(),
                ],
                0.6,
            )
        }
    }

    fn interpret_high_latency(
        &self,
        hop: &HopSample,
        is_destination: bool,
        next_hops: &[&HopSample],
        level: ExplanationLevel,
    ) -> HopInterpretation {
        let avg = hop.stats.avg_ms.unwrap_or(0.0);
        let is_critical = avg > self.latency_critical_threshold;

        // Check if latency increase started at this hop
        let latency_increase_here = !next_hops.is_empty();

        // Check if latency continues to destination
        let latency_continues = next_hops.iter().all(|h| {
            h.stats
                .avg_ms
                .map(|a| a > self.latency_warning_threshold)
                .unwrap_or(false)
        });

        let severity = if is_critical {
            Severity::Critical
        } else {
            Severity::Warning
        };

        if is_destination {
            self.message(
                level,
                severity,
                &format!("The target is slow to answer ({:.0}ms)", avg),
                "Replies take longer than normal. Browsing, downloads, calls, or games may feel delayed.",
                &format!("High latency: {:.0}ms average", avg),
                "The destination server is responding with high latency. This will cause noticeable delays in applications and may indicate server load or network issues.",
                vec![
                    "The target may be overloaded".to_string(),
                    "The route may cover a long geographic distance".to_string(),
                    "There may be congestion near the destination".to_string(),
                ],
                0.85,
            )
        } else if latency_increase_here && latency_continues {
            self.message(
                level,
                severity,
                &format!("Delay starts around this hop ({:.0}ms)", avg),
                "Latency jumps here and stays high later. This is a good suspect link for congestion or distance.",
                &format!("Latency spike at this hop: {:.0}ms", avg),
                "A significant increase in latency begins at this hop and continues to the destination. This identifies the network segment where delays are being introduced.",
                vec![
                    "This network segment may be congested".to_string(),
                    "The route may cross a long-distance link".to_string(),
                    "There may be an oversubscribed peering point".to_string(),
                    "A tunnel or VPN can add delay here".to_string(),
                ],
                0.8,
            )
        } else {
            self.message(
                level,
                Severity::Warning,
                &format!("This hop replies slowly ({:.0}ms)", avg),
                "This router is slower to answer ping than the rest of the path. That matters only if the delay continues to the target.",
                &format!("Elevated latency: {:.0}ms", avg),
                "This hop shows higher than optimal latency. If this is an intermediate hop with normal latency at the destination, it may be due to ICMP deprioritization.",
                vec![
                    "The router control plane may answer slowly".to_string(),
                    "ICMP processing overhead can inflate this number".to_string(),
                    "This may be normal for this network segment".to_string(),
                ],
                0.6,
            )
        }
    }

    fn interpret_high_jitter(
        &self,
        hop: &HopSample,
        is_destination: bool,
        level: ExplanationLevel,
    ) -> HopInterpretation {
        let jitter = hop.stats.jitter_ms.unwrap_or(0.0);

        if is_destination {
            self.message(
                level,
                Severity::Warning,
                &format!("Latency is unstable ({:.0}ms jitter)", jitter),
                "Reply time changes a lot from packet to packet. Calls, streams, and games may stutter even if average ping looks okay.",
                &format!("High jitter: {:.0}ms variation", jitter),
                "The connection to the destination has high latency variation (jitter). This can cause problems for real-time applications like VoIP, video calls, and gaming, even if average latency is acceptable.",
                vec![
                    "There may be congestion causing queue swings".to_string(),
                    "Bufferbloat on a router or modem is possible".to_string(),
                    "Wireless interference can cause jitter".to_string(),
                    "Traffic shaping may be inconsistent".to_string(),
                ],
                0.8,
            )
        } else {
            self.message(
                level,
                Severity::Warning,
                &format!("This hop has unstable reply times ({:.0}ms)", jitter),
                "If later hops stay stable, this is often just how this router answers ping.",
                &format!("High jitter detected: {:.0}ms", jitter),
                "This hop shows significant latency variation. Jitter at intermediate hops may indicate congestion or variable routing, but only matters if it affects the destination.",
                vec![
                    "ICMP processing time may vary here".to_string(),
                    "There may be short congestion bursts".to_string(),
                    "Load balancing across paths can change timings".to_string(),
                ],
                0.6,
            )
        }
    }

    fn interpret_ok(
        &self,
        hop: &HopSample,
        is_destination: bool,
        level: ExplanationLevel,
    ) -> HopInterpretation {
        let avg = hop
            .stats
            .avg_ms
            .map(|a| format!("{:.0}ms", a))
            .unwrap_or_else(|| "N/A".to_string());

        if is_destination {
            self.message(
                level,
                Severity::Ok,
                &format!("Target looks healthy ({})", avg),
                "The destination is replying with little or no loss and normal delay.",
                &format!("Destination responding normally ({})", avg),
                "The target server is responding with healthy latency and no packet loss. The network path appears to be functioning correctly.",
                vec![],
                0.9,
            )
        } else {
            self.message(
                level,
                Severity::Ok,
                &format!("This hop looks normal ({})", avg),
                "This router is replying normally.",
                &format!("Healthy ({})", avg),
                "This hop is responding normally with acceptable latency and no significant packet loss.",
                vec![],
                0.85,
            )
        }
    }

    fn message(
        &self,
        level: ExplanationLevel,
        severity: Severity,
        simple_headline: &str,
        simple_explanation: &str,
        detailed_headline: &str,
        detailed_explanation: &str,
        probable_causes: Vec<String>,
        confidence: f64,
    ) -> HopInterpretation {
        let (headline, explanation) = match level {
            ExplanationLevel::Simple => {
                (simple_headline.to_string(), simple_explanation.to_string())
            }
            ExplanationLevel::Detailed => (
                detailed_headline.to_string(),
                detailed_explanation.to_string(),
            ),
        };

        HopInterpretation {
            severity,
            headline,
            explanation,
            probable_causes,
            confidence,
        }
    }

    /// Generate an overall session summary
    pub fn generate_summary(&self, hops: &[HopSample], language: Language) -> SessionSummary {
        let summary = if hops.is_empty() {
            SessionSummary {
                overall_status: Severity::Unknown,
                primary_finding: "No trace data available".to_string(),
                secondary_findings: vec![
                    "The trace did not complete or no hops were discovered".to_string()
                ],
                recommended_next_steps: vec!["Try running the trace again".to_string()],
                problem_hop_index: None,
                destination_reachable: false,
            }
        } else {
            // Find destination hop (highest index)
            let destination = hops.iter().max_by_key(|h| h.index);
            let destination_reachable = destination
                .map(|d| d.stats.received > 0 && d.stats.loss_percent < 50.0)
                .unwrap_or(false);

            // Find the first problematic hop
            let problem_hop = hops.iter().find(|h| {
                matches!(h.status, Severity::Warning | Severity::Critical) && h.stats.received > 0
                // Skip hops that just didn't respond
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
                    findings.push(format!(
                        "High packet loss at destination: {:.0}%",
                        dest.stats.loss_percent
                    ));
                    recommendations.push(
                        "Contact your ISP or the destination server administrator".to_string(),
                    );
                } else if dest.stats.loss_percent > self.loss_warning_threshold {
                    findings.push(format!(
                        "Moderate packet loss at destination: {:.0}%",
                        dest.stats.loss_percent
                    ));
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
                        recommendations.push(
                            "For VoIP/gaming issues, check for bufferbloat on your router"
                                .to_string(),
                        );
                    }
                }
            }

            // Check for intermediate issues that propagate
            let mut loss_start: Option<u8> = None;
            let mut latency_start: Option<u8> = None;

            for hop in hops.iter().take(hops.len().saturating_sub(1)) {
                // Exclude destination
                if loss_start.is_none() && hop.stats.loss_percent > self.loss_warning_threshold {
                    // Check if loss continues
                    let continues = hops
                        .iter()
                        .filter(|h| h.index > hop.index)
                        .any(|h| h.stats.loss_percent > self.loss_warning_threshold);
                    if continues {
                        loss_start = Some(hop.index);
                    }
                }

                if latency_start.is_none() {
                    if let Some(avg) = hop.stats.avg_ms {
                        if avg > self.latency_warning_threshold {
                            let continues = hops.iter().filter(|h| h.index > hop.index).any(|h| {
                                h.stats.avg_ms.unwrap_or(0.0) > self.latency_warning_threshold
                            });
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
                    findings
                        .push("Issues detected starting at your local network or ISP".to_string());
                    recommendations
                        .push("Check your local network equipment (router, cables)".to_string());
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
                if let Some(first_dead_hop) = hops
                    .iter()
                    .find(|h| h.stats.received == 0 && h.stats.sent >= 3)
                {
                    format!("The route likely breaks near hop {}", first_dead_hop.index)
                } else {
                    "The target is not replying".to_string()
                }
            } else if let Some(loss_idx) = loss_start {
                format!("Packet loss begins at hop {}", loss_idx)
            } else if let Some(lat_idx) = latency_start {
                format!("Latency increase begins at hop {}", lat_idx)
            } else if let Some(problem) = problem_hop {
                problem
                    .interpretation
                    .as_ref()
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
                if overall_status != Severity::Ok {
                    recommendations.push("Monitor the connection for changes".to_string());
                    recommendations.push(
                        "Share this report with technical support if issues persist".to_string(),
                    );
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
        };

        self.localize_summary(summary, language)
    }

    fn localize_hop_interpretation(
        &self,
        mut interpretation: HopInterpretation,
        language: Language,
    ) -> HopInterpretation {
        if language == Language::Ru {
            interpretation.headline = self.translate_text(&interpretation.headline);
            interpretation.explanation = self.translate_text(&interpretation.explanation);
            interpretation.probable_causes = interpretation
                .probable_causes
                .into_iter()
                .map(|item| self.translate_text(&item))
                .collect();
        }

        interpretation
    }

    fn localize_summary(&self, mut summary: SessionSummary, language: Language) -> SessionSummary {
        if language == Language::Ru {
            summary.primary_finding = self.translate_text(&summary.primary_finding);
            summary.secondary_findings = summary
                .secondary_findings
                .into_iter()
                .map(|item| self.translate_text(&item))
                .collect();
            summary.recommended_next_steps = summary
                .recommended_next_steps
                .into_iter()
                .map(|item| self.translate_text(&item))
                .collect();
        }

        summary
    }

    fn translate_text(&self, text: &str) -> String {
        match text {
            "Target is not replying" => "Цель не отвечает".to_string(),
            "This address did not answer ping at all. Usually that means the host is down, blocks ping, or the route cannot reach it." => "Этот адрес совсем не отвечает на ping. Обычно это значит, что хост недоступен, блокирует ping или маршрут до него не доходит.".to_string(),
            "Destination not responding" => "Цель не отвечает".to_string(),
            "The target server is not responding to ICMP (ping) requests. This could indicate the server is down, a firewall is blocking ICMP, or there's a network issue at the destination." => "Целевой сервер не отвечает на ICMP-запросы (ping). Это может означать, что сервер выключен, firewall блокирует ICMP или проблема находится на стороне назначения.".to_string(),
            "The server may be offline" => "Сервер может быть недоступен".to_string(),
            "A firewall may be blocking ping replies" => "Firewall может блокировать ответы на ping".to_string(),
            "The route may not be reaching the destination" => "Маршрут может не доходить до назначения".to_string(),
            "This hop ignores ping replies" => "Этот хоп игнорирует ответы на ping".to_string(),
            "This router is not answering, but later hops still reply. That usually means the router hides ping responses and is not the real problem." => "Этот роутер не отвечает, но более поздние хопы отвечают. Обычно это значит, что роутер скрывает ответы на ping и не является реальной проблемой.".to_string(),
            "Hop not responding (may be normal)" => "Хоп не отвечает (это может быть нормой)".to_string(),
            "This intermediate router is not responding to ICMP requests, but traffic is still reaching later hops. Many routers are configured to deprioritize or block ICMP responses while continuing to forward traffic normally." => "Этот промежуточный роутер не отвечает на ICMP-запросы, но трафик всё равно доходит до следующих хопов. Многие роутеры намеренно занижают приоритет ICMP или блокируют ответы, продолжая нормально пересылать трафик.".to_string(),
            "The router may be rate-limiting ping" => "Роутер может ограничивать ответы на ping".to_string(),
            "This device may deprioritize control traffic" => "Устройство может занижать приоритет служебного трафика".to_string(),
            "ICMP could be filtered at this hop" => "ICMP может фильтроваться на этом хопе".to_string(),
            "Traffic likely stops here" => "Трафик, вероятно, обрывается здесь".to_string(),
            "This hop and the ones after it are not replying. That usually means the route breaks at this point or just before it." => "Этот хоп и все после него не отвечают. Обычно это значит, что маршрут ломается в этой точке или прямо перед ней.".to_string(),
            "Connection lost at this hop" => "Соединение теряется на этом хопе".to_string(),
            "Network connectivity is being lost at or before this hop, and subsequent hops are also not responding. This suggests a real connectivity issue rather than ICMP filtering." => "Связность сети теряется на этом хопе или перед ним, и следующие хопы тоже не отвечают. Это больше похоже на реальную сетевую проблему, чем на фильтрацию ICMP.".to_string(),
            "A router or link may be down" => "Роутер или линк могут быть недоступны".to_string(),
            "Routing may be broken at this point" => "Маршрутизация может ломаться в этой точке".to_string(),
            "Heavy congestion may be dropping packets" => "Сильная перегрузка может приводить к потере пакетов".to_string(),
            "This hop drops ping replies, but later hops are healthy. Usually the router is limiting ping responses rather than dropping real traffic." => "Этот хоп теряет ответы на ping, но более поздние хопы в порядке. Обычно это значит, что роутер ограничивает ответы на ping, а не теряет реальный трафик.".to_string(),
            "This hop shows packet loss, but subsequent hops and the destination are responding normally. This is typically caused by ICMP rate limiting, where the router deliberately slows down its responses to prevent overload." => "Этот хоп показывает потери, но последующие хопы и цель отвечают нормально. Обычно это вызвано ограничением ICMP, когда роутер специально замедляет ответы, чтобы не перегружаться.".to_string(),
            "The router may be rate-limiting ICMP" => "Роутер может ограничивать ICMP".to_string(),
            "This device may give low priority to ping replies" => "Устройство может давать низкий приоритет ответам на ping".to_string(),
            "The control plane could be busy even while forwarding stays normal" => "Служебная плоскость может быть загружена, даже если пересылка трафика работает нормально".to_string(),
            "Some packets reach the target and some do not. Apps may feel slow, disconnect, or retry." => "Часть пакетов доходит до цели, а часть нет. Приложения могут тормозить, отключаться или повторять запросы.".to_string(),
            "The target server is experiencing significant packet loss. This indicates a real connectivity issue that will affect application performance." => "Целевой сервер испытывает заметную потерю пакетов. Это указывает на реальную проблему со связностью, которая будет влиять на работу приложений.".to_string(),
            "There may be congestion between you and the target" => "Между вами и целью может быть перегрузка".to_string(),
            "The target may be overloaded" => "Цель может быть перегружена".to_string(),
            "Your connection may be unstable" => "Ваше соединение может быть нестабильным".to_string(),
            "An ISP or routing issue may be involved" => "Проблема может быть у провайдера или в маршрутизации".to_string(),
            "Packet loss begins here and keeps showing up later. This usually points to a real problem on this link or router." => "Потери пакетов начинаются здесь и продолжаются дальше. Обычно это указывает на реальную проблему на этом линке или роутере.".to_string(),
            "This network segment may be congested" => "Этот сегмент сети может быть перегружен".to_string(),
            "This router or link may have a fault" => "У этого роутера или линка может быть неисправность".to_string(),
            "Link capacity may be saturated" => "Канал может быть перегружен".to_string(),
            "ISP peering could be unstable" => "Стыковка с провайдером может быть нестабильной".to_string(),
            "Later hops look normal, so this is often ping-reply behavior rather than a real end-to-end loss problem." => "Более поздние хопы выглядят нормально, поэтому это часто связано с поведением ping-ответов, а не с реальной потерей трафика по пути.".to_string(),
            "This intermediate hop shows packet loss, but subsequent hops appear normal. This could be due to ICMP deprioritization rather than actual traffic loss." => "Этот промежуточный хоп показывает потери, но следующие хопы выглядят нормально. Это может быть связано с занижением приоритета ICMP, а не с реальной потерей трафика.".to_string(),
            "ICMP may be rate-limited here" => "ICMP может ограничиваться на этом хопе".to_string(),
            "There may be temporary congestion" => "Здесь может быть временная перегрузка".to_string(),
            "Load balancing can make traceroute look uneven" => "Балансировка нагрузки может делать traceroute неровным".to_string(),
            "Replies take longer than normal. Browsing, downloads, calls, or games may feel delayed." => "Ответы занимают больше времени, чем обычно. Браузер, загрузки, звонки или игры могут ощущаться медленными.".to_string(),
            "The destination server is responding with high latency. This will cause noticeable delays in applications and may indicate server load or network issues." => "Целевой сервер отвечает с высокой задержкой. Это вызовет заметные задержки в приложениях и может указывать на нагрузку на сервер или проблемы в сети.".to_string(),
            "The route may cover a long geographic distance" => "Маршрут может проходить через большое расстояние".to_string(),
            "There may be congestion near the destination" => "Возле назначения может быть перегрузка".to_string(),
            "Latency jumps here and stays high later. This is a good suspect link for congestion or distance." => "Задержка резко растёт здесь и остаётся высокой дальше. Это хороший кандидат на проблемный линк из-за перегрузки или расстояния.".to_string(),
            "A significant increase in latency begins at this hop and continues to the destination. This identifies the network segment where delays are being introduced." => "Заметный рост задержки начинается на этом хопе и продолжается до цели. Это указывает на сегмент сети, где появляется задержка.".to_string(),
            "The route may cross a long-distance link" => "Маршрут может проходить через дальний линк".to_string(),
            "There may be an oversubscribed peering point" => "Точка стыка сетей может быть перегружена".to_string(),
            "A tunnel or VPN can add delay here" => "Туннель или VPN могут добавлять задержку здесь".to_string(),
            "This router is slower to answer ping than the rest of the path. That matters only if the delay continues to the target." => "Этот роутер отвечает на ping медленнее остального маршрута. Это важно только если задержка сохраняется до цели.".to_string(),
            "This hop shows higher than optimal latency. If this is an intermediate hop with normal latency at the destination, it may be due to ICMP deprioritization." => "Этот хоп показывает задержку выше оптимальной. Если это промежуточный хоп, а у цели задержка нормальная, причина может быть в низком приоритете ICMP.".to_string(),
            "The router control plane may answer slowly" => "Служебная плоскость роутера может отвечать медленно".to_string(),
            "ICMP processing overhead can inflate this number" => "Нагрузка на обработку ICMP может завышать это значение".to_string(),
            "This may be normal for this network segment" => "Для этого сегмента сети это может быть нормой".to_string(),
            "Reply time changes a lot from packet to packet. Calls, streams, and games may stutter even if average ping looks okay." => "Время ответа сильно меняется от пакета к пакету. Звонки, стримы и игры могут подлагивать, даже если средний ping выглядит нормально.".to_string(),
            "The connection to the destination has high latency variation (jitter). This can cause problems for real-time applications like VoIP, video calls, and gaming, even if average latency is acceptable." => "Соединение до цели имеет сильный разброс задержки (джиттер). Это может вызывать проблемы для приложений реального времени, например VoIP, видеозвонков и игр, даже если средняя задержка приемлемая.".to_string(),
            "There may be congestion causing queue swings" => "Перегрузка может вызывать скачки очередей".to_string(),
            "Bufferbloat on a router or modem is possible" => "Возможен bufferbloat на роутере или модеме".to_string(),
            "Wireless interference can cause jitter" => "Помехи в Wi‑Fi могут вызывать джиттер".to_string(),
            "Traffic shaping may be inconsistent" => "Шейпинг трафика может работать нестабильно".to_string(),
            "If later hops stay stable, this is often just how this router answers ping." => "Если более поздние хопы остаются стабильными, это часто просто особенность ответа этого роутера на ping.".to_string(),
            "This hop shows significant latency variation. Jitter at intermediate hops may indicate congestion or variable routing, but only matters if it affects the destination." => "Этот хоп показывает заметный разброс задержки. Джиттер на промежуточных хопах может означать перегрузку или меняющуюся маршрутизацию, но важен только если влияет на цель.".to_string(),
            "ICMP processing time may vary here" => "Время обработки ICMP здесь может плавать".to_string(),
            "There may be short congestion bursts" => "Могут быть короткие всплески перегрузки".to_string(),
            "Load balancing across paths can change timings" => "Балансировка по разным путям может менять тайминги".to_string(),
            "The destination is replying with little or no loss and normal delay." => "Цель отвечает с минимальными или нулевыми потерями и нормальной задержкой.".to_string(),
            "The target server is responding with healthy latency and no packet loss. The network path appears to be functioning correctly." => "Целевой сервер отвечает с нормальной задержкой и без потери пакетов. Маршрут, похоже, работает корректно.".to_string(),
            "This router is replying normally." => "Этот роутер отвечает нормально.".to_string(),
            "This hop is responding normally with acceptable latency and no significant packet loss." => "Этот хоп отвечает нормально, с приемлемой задержкой и без заметной потери пакетов.".to_string(),
            "No trace data available" => "Нет данных трассировки".to_string(),
            "The trace did not complete or no hops were discovered" => "Трассировка не завершилась или хопы не были обнаружены".to_string(),
            "Try running the trace again" => "Попробуйте запустить трассировку ещё раз".to_string(),
            "Destination is not responding to ICMP requests" => "Назначение не отвечает на ICMP-запросы".to_string(),
            "Verify the destination address is correct" => "Проверьте, что адрес назначения указан верно".to_string(),
            "The server may be down or blocking ICMP" => "Сервер может быть недоступен или блокировать ICMP".to_string(),
            "Contact your ISP or the destination server administrator" => "Свяжитесь с провайдером или администратором целевого сервера".to_string(),
            "For VoIP/gaming issues, check for bufferbloat on your router" => "Для проблем с VoIP или играми проверьте bufferbloat на роутере".to_string(),
            "Issues detected starting at your local network or ISP" => "Проблемы начинаются в вашей локальной сети или у провайдера".to_string(),
            "Check your local network equipment (router, cables)" => "Проверьте локальное оборудование сети (роутер, кабели)".to_string(),
            "Restart your router/modem" => "Перезагрузите роутер или модем".to_string(),
            "No significant issues detected along the route" => "Существенных проблем по маршруту не обнаружено".to_string(),
            "Monitor the connection for changes" => "Наблюдайте за соединением и проверяйте изменения".to_string(),
            "Share this report with technical support if issues persist" => "Передайте этот отчёт в техподдержку, если проблема сохранится".to_string(),
            "Some issues detected" => "Обнаружены некоторые проблемы".to_string(),
            "Connection looks stable" => "Соединение выглядит стабильным".to_string(),
            _ => self.translate_pattern_text(text),
        }
    }

    fn translate_pattern_text(&self, text: &str) -> String {
        let patterns = [
            (
                "% loss here is probably harmless",
                "% потерь здесь, вероятно, безвредны",
            ),
            (
                "% packet loss (likely rate-limiting)",
                "% потерь пакетов (похоже на rate-limit)",
            ),
            ("% packet loss to the target", "% потерь пакетов до цели"),
            (
                "% packet loss to destination",
                "% потерь пакетов до назначения",
            ),
            (
                "% loss starts at this hop",
                "% потерь начинаются на этом хопе",
            ),
            (
                "% packet loss starting here",
                "% потерь пакетов начинаются здесь",
            ),
            (
                "% loss only on this router",
                "% потерь только на этом роутере",
            ),
            (
                "% packet loss at intermediate hop",
                "% потерь на промежуточном хопе",
            ),
            ("The target is slow to answer (", "Цель отвечает медленно ("),
            ("High latency: ", "Высокая задержка: "),
            (
                "Delay starts around this hop (",
                "Задержка начинается примерно на этом хопе (",
            ),
            (
                "Latency spike at this hop: ",
                "Скачок задержки на этом хопе: ",
            ),
            ("This hop replies slowly (", "Этот хоп отвечает медленно ("),
            ("Elevated latency: ", "Повышенная задержка: "),
            ("Latency is unstable (", "Задержка нестабильна ("),
            ("High jitter: ", "Высокий джиттер: "),
            (
                "This hop has unstable reply times (",
                "У этого хопа нестабильное время ответа (",
            ),
            ("High jitter detected: ", "Обнаружен высокий джиттер: "),
            ("Target looks healthy (", "Цель выглядит здоровой ("),
            (
                "Destination responding normally (",
                "Цель отвечает нормально (",
            ),
            ("This hop looks normal (", "Этот хоп выглядит нормально ("),
            ("Healthy (", "Норма ("),
            (
                "High packet loss at destination: ",
                "Высокая потеря пакетов на назначении: ",
            ),
            (
                "Moderate packet loss at destination: ",
                "Умеренная потеря пакетов на назначении: ",
            ),
            (
                "Very high latency to destination: ",
                "Очень высокая задержка до назначения: ",
            ),
            (
                "Elevated latency to destination: ",
                "Повышенная задержка до назначения: ",
            ),
            (
                "High jitter at destination: ",
                "Высокий джиттер на назначении: ",
            ),
            (
                "The route likely breaks near hop ",
                "Маршрут, вероятно, обрывается около хопа ",
            ),
            (
                "Packet loss begins at hop ",
                "Потери пакетов начинаются на хопе ",
            ),
            (
                "Latency increase begins at hop ",
                "Рост задержки начинается на хопе ",
            ),
        ];

        for (prefix, translated_prefix) in patterns {
            if let Some(rest) = text.strip_prefix(prefix) {
                return format!("{translated_prefix}{rest}");
            }
        }

        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hop(
        index: u8,
        sent: u32,
        received: u32,
        avg_ms: Option<f64>,
        loss_percent: f64,
    ) -> HopSample {
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

        assert!(matches!(
            interpretation.severity,
            Severity::Critical | Severity::Warning
        ));
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
    fn test_simple_no_response_message_for_beginners() {
        let engine = InterpretationEngine::new();
        let hop = make_hop(4, 5, 0, None, 100.0);
        let next_hop = make_hop(5, 5, 5, Some(18.0), 0.0);

        let interpretation =
            engine.interpret_hop_with_level(&hop, false, &[&next_hop], ExplanationLevel::Simple);

        assert_eq!(interpretation.severity, Severity::Unknown);
        assert!(interpretation.headline.contains("ignores ping replies"));
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

        let summary = engine.generate_summary(&hops, Language::En);

        assert_eq!(summary.overall_status, Severity::Ok);
        assert!(summary.destination_reachable);
    }
}
