//! Tauri commands for frontend communication
//!
//! This module provides the command interface between the Rust backend
//! and the TypeScript frontend.

use crate::interpretation::InterpretationEngine;
use crate::traceroute::TraceRunner;
use crate::types::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{error, info};

/// Application state
pub struct AppState {
    /// Active trace sessions
    pub sessions: RwLock<HashMap<String, ActiveSession>>,
    /// Interpretation engine
    pub engine: InterpretationEngine,
}

/// Active trace session with runner and cancellation flag
pub struct ActiveSession {
    pub runner: Arc<Mutex<Option<TraceRunner>>>,
    pub cancel_flag: Arc<AtomicBool>,
    pub hops: Arc<StdMutex<Vec<HopSample>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            engine: InterpretationEngine::new(),
        }
    }
}

/// Resolve a hostname to an IP address
#[tauri::command]
pub async fn resolve_host(target: String) -> Result<String, String> {
    crate::traceroute::resolve_target(&target)
        .map(|ip| ip.to_string())
        .map_err(|e| e.to_string())
}

/// Start a new trace session
#[tauri::command]
pub async fn start_trace(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    config: TraceConfig,
) -> Result<TraceSession, String> {
    info!("Starting trace to: {}", config.target);

    let mut session = TraceSession::new(config.clone());

    let runner = match TraceRunner::new(&session) {
        Ok(runner) => runner,
        Err(err) => {
            session.state = SessionState::Error;
            session.error = Some(err.to_string());
            return Err(err.to_string());
        }
    };

    session.target_ip = Some(runner.target_ip());
    session.state = SessionState::Running;
    session.started_at = Some(chrono::Utc::now());

    let session_id = session.id.clone();
    let cancel_flag = runner.cancel_flag();
    let hops = runner.hops_handle();

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(
            session_id.clone(),
            ActiveSession {
                runner: Arc::new(Mutex::new(Some(runner))),
                cancel_flag: cancel_flag.clone(),
                hops,
            },
        );
    }

    app.emit(
        "trace-event",
        TraceEvent::SessionStarted {
            session: session.clone(),
        },
    )
    .map_err(|e| e.to_string())?;

    let app_clone = app.clone();
    let state_clone = state.inner().clone();
    let session_id_clone = session_id.clone();

    tokio::spawn(async move {
        run_trace_task(app_clone, state_clone, session_id_clone, cancel_flag).await;
    });

    Ok(session)
}

/// Background task for running a trace
async fn run_trace_task(
    app: AppHandle,
    state: Arc<AppState>,
    session_id: String,
    _cancel_flag: Arc<AtomicBool>,
) {
    let runner = {
        let sessions = state.sessions.read().await;
        sessions
            .get(&session_id)
            .map(|session| session.runner.clone())
    };

    let Some(runner) = runner else {
        error!("Session not found: {}", session_id);
        return;
    };

    let Some(mut runner_guard) = runner.lock().await.take() else {
        error!("Runner already taken for session: {}", session_id);
        return;
    };

    let (tx, mut rx) = mpsc::channel::<TraceEvent>(100);
    let session_id_clone = session_id.clone();

    let app_clone = app.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Err(err) = app_clone.emit("trace-event", &event) {
                error!("Failed to emit event: {}", err);
            }
        }
    });

    let result = runner_guard.run(tx).await;
    let hops = runner_guard.get_hops();

    let engine = &state.engine;
    let hops_with_interpretation: Vec<HopSample> = hops
        .iter()
        .enumerate()
        .map(|(index, hop)| {
            let is_destination = index == hops.len().saturating_sub(1);
            let next_hops: Vec<&HopSample> = hops.iter().skip(index + 1).collect();
            let mut hop = hop.clone();
            hop.interpretation = Some(engine.interpret_hop(&hop, is_destination, &next_hops));
            hop.status = hop.interpretation.as_ref().unwrap().severity;
            hop
        })
        .collect();

    let summary = engine.generate_summary(&hops_with_interpretation);

    {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&session_id);
    }

    match result {
        Ok(()) => {
            info!("Trace completed: {}", session_id);
            if let Err(err) = app.emit(
                "trace-event",
                TraceEvent::SessionCompleted {
                    session_id: session_id_clone,
                    summary,
                    hops: hops_with_interpretation,
                },
            ) {
                error!("Failed to emit completion event: {}", err);
            }
        }
        Err(err) => {
            error!("Trace error: {}", err);
            if let Err(emit_err) = app.emit(
                "trace-event",
                TraceEvent::SessionError {
                    session_id: session_id_clone,
                    error: err.to_string(),
                },
            ) {
                error!("Failed to emit error event: {}", emit_err);
            }
        }
    }
}

/// Stop a running trace session
#[tauri::command]
pub async fn stop_trace(state: State<'_, Arc<AppState>>, session_id: String) -> Result<(), String> {
    info!("Stopping trace: {}", session_id);

    let sessions = state.sessions.read().await;

    if let Some(session) = sessions.get(&session_id) {
        session.cancel_flag.store(false, Ordering::Relaxed);
    }

    Ok(())
}

/// Get current hop data for a session
#[tauri::command]
pub async fn get_session_hops(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<Vec<HopSample>, String> {
    let sessions = state.sessions.read().await;

    if let Some(session) = sessions.get(&session_id) {
        return Ok(session.hops.lock().unwrap().clone());
    }

    Err("Session not found or not running".to_string())
}

/// Generate interpretation for a set of hops
#[tauri::command]
pub fn interpret_hops(state: State<'_, Arc<AppState>>, hops: Vec<HopSample>) -> SessionSummary {
    state.engine.generate_summary(&hops)
}

/// Export session data as JSON
#[tauri::command]
pub async fn export_json(
    summary: SessionSummary,
    hops: Vec<HopSample>,
    config: TraceConfig,
) -> Result<String, String> {
    #[derive(serde::Serialize)]
    struct ExportData {
        summary: SessionSummary,
        hops: Vec<HopSample>,
        config: TraceConfig,
        exported_at: chrono::DateTime<chrono::Utc>,
    }

    let data = ExportData {
        summary,
        hops,
        config,
        exported_at: chrono::Utc::now(),
    };

    serde_json::to_string_pretty(&data).map_err(|e| e.to_string())
}

/// Generate HTML report
#[tauri::command]
pub async fn export_html(
    summary: SessionSummary,
    hops: Vec<HopSample>,
    config: TraceConfig,
) -> Result<String, String> {
    let target = &config.target;
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");

    let status_color = match summary.overall_status {
        Severity::Ok => "#22c55e",
        Severity::Warning => "#eab308",
        Severity::Critical => "#ef4444",
        Severity::Unknown => "#6b7280",
    };

    let status_text = match summary.overall_status {
        Severity::Ok => "OK",
        Severity::Warning => "Warning",
        Severity::Critical => "Critical",
        Severity::Unknown => "Unknown",
    };

    let hop_rows = hops
        .iter()
        .map(|hop| {
            let hop_status_color = match hop.status {
                Severity::Ok => "#22c55e",
                Severity::Warning => "#eab308",
                Severity::Critical => "#ef4444",
                Severity::Unknown => "#6b7280",
            };

            let ip = hop
                .ip
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "*".to_string());
            let hostname = hop.hostname.as_deref().unwrap_or(&ip);

            let loss = format!("{:.1}%", hop.stats.loss_percent);
            let sent = hop.stats.sent.to_string();
            let recv = hop.stats.received.to_string();
            let best = hop
                .stats
                .best_ms
                .map(|v| format!("{:.1}", v))
                .unwrap_or("-".to_string());
            let avg = hop
                .stats
                .avg_ms
                .map(|v| format!("{:.1}", v))
                .unwrap_or("-".to_string());
            let worst = hop
                .stats
                .worst_ms
                .map(|v| format!("{:.1}", v))
                .unwrap_or("-".to_string());
            let last = hop
                .stats
                .last_ms
                .map(|v| format!("{:.1}", v))
                .unwrap_or("-".to_string());
            let jitter = hop
                .stats
                .jitter_ms
                .map(|v| format!("{:.1}", v))
                .unwrap_or("-".to_string());

            let interpretation = hop.interpretation.as_ref();
            let headline = interpretation
                .map(|item| item.headline.clone())
                .unwrap_or_default();
            let explanation = interpretation
                .map(|item| item.explanation.clone())
                .unwrap_or_default();

            format!(
                r#"<tr>
                <td style="text-align: center; color: {hop_status_color};">&#9679;</td>
                <td>{hop_index}</td>
                <td title="{ip}">{hostname}</td>
                <td>{loss}</td>
                <td>{sent}</td>
                <td>{recv}</td>
                <td>{best}</td>
                <td>{avg}</td>
                <td>{worst}</td>
                <td>{last}</td>
                <td>{jitter}</td>
                <td><strong>{headline}</strong><br/><small style="color: #666;">{explanation}</small></td>
            </tr>"#,
                hop_status_color = hop_status_color,
                hop_index = hop.index,
                ip = ip,
                hostname = hostname,
                loss = loss,
                sent = sent,
                recv = recv,
                best = best,
                avg = avg,
                worst = worst,
                last = last,
                jitter = jitter,
                headline = headline,
                explanation = explanation,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let findings = summary
        .secondary_findings
        .iter()
        .map(|finding| format!("<li>{}</li>", finding))
        .collect::<Vec<_>>()
        .join("\n");

    let recommendations = summary
        .recommended_next_steps
        .iter()
        .map(|recommendation| format!("<li>{}</li>", recommendation))
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>WLTP Report - {}</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; line-height: 1.6; color: #333; background: #f5f5f5; padding: 20px; }}
        .container {{ max-width: 1200px; margin: 0 auto; background: white; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .header {{ padding: 24px; border-bottom: 1px solid #eee; }}
        .header h1 {{ font-size: 24px; margin-bottom: 8px; }}
        .header .meta {{ color: #666; font-size: 14px; }}
        .status {{ display: inline-block; padding: 4px 12px; border-radius: 4px; color: white; font-weight: 600; font-size: 14px; margin-left: 12px; }}
        .summary {{ padding: 24px; background: #fafafa; border-bottom: 1px solid #eee; }}
        .summary h2 {{ font-size: 18px; margin-bottom: 16px; }}
        .summary-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 24px; }}
        .summary-card {{ background: white; padding: 16px; border-radius: 6px; border: 1px solid #e0e0e0; }}
        .summary-card h3 {{ font-size: 14px; color: #666; margin-bottom: 8px; }}
        .summary-card ul {{ margin-left: 20px; }}
        .summary-card li {{ margin-bottom: 4px; }}
        .primary-finding {{ font-size: 18px; font-weight: 600; margin-bottom: 16px; padding: 12px; background: white; border-radius: 6px; border-left: 4px solid {}; }}
        .hops {{ padding: 24px; }}
        .hops h2 {{ font-size: 18px; margin-bottom: 16px; }}
        table {{ width: 100%; border-collapse: collapse; font-size: 14px; }}
        th {{ background: #f5f5f5; padding: 12px 8px; text-align: left; font-weight: 600; border-bottom: 2px solid #e0e0e0; }}
        td {{ padding: 10px 8px; border-bottom: 1px solid #eee; }}
        tr:hover {{ background: #fafafa; }}
        .footer {{ padding: 16px 24px; border-top: 1px solid #eee; font-size: 12px; color: #666; text-align: center; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>WLTP Network Diagnostic Report<span class="status" style="background: {};">{}</span></h1>
            <div class="meta">
                Target: <strong>{}</strong> | Generated: {}
            </div>
        </div>

        <div class="summary">
            <h2>Summary</h2>
            <div class="primary-finding">{}</div>
            <div class="summary-grid">
                <div class="summary-card">
                    <h3>Findings</h3>
                    <ul>
                        {}
                    </ul>
                </div>
                <div class="summary-card">
                    <h3>Recommendations</h3>
                    <ul>
                        {}
                    </ul>
                </div>
            </div>
        </div>

        <div class="hops">
            <h2>Route Details</h2>
            <table>
                <thead>
                    <tr>
                        <th style="width: 40px;"></th>
                        <th style="width: 50px;">Hop</th>
                        <th>Host</th>
                        <th>Loss%</th>
                        <th>Sent</th>
                        <th>Recv</th>
                        <th>Best</th>
                        <th>Avg</th>
                        <th>Worst</th>
                        <th>Last</th>
                        <th>Jitter</th>
                        <th>Interpretation</th>
                    </tr>
                </thead>
                <tbody>
                    {}
                </tbody>
            </table>
        </div>

        <div class="footer">
            Generated by WLTP - Modern WinMTR for Windows/macOS
        </div>
    </div>
</body>
</html>"#,
        target,
        status_color,
        status_color,
        status_text,
        target,
        timestamp,
        summary.primary_finding,
        findings,
        recommendations,
        hop_rows
    );

    Ok(html)
}

/// Save content to a file
#[tauri::command]
pub async fn save_file(
    _app: AppHandle,
    _content: String,
    _default_name: String,
    _filter_name: String,
    _filter_extensions: Vec<String>,
) -> Result<String, String> {
    Err("save_file is not supported in this build".to_string())
}

/// Read app settings
#[tauri::command]
pub async fn get_settings() -> Settings {
    Settings::default()
}

/// Update app settings
#[tauri::command]
pub async fn update_settings(_settings: Settings) -> Result<(), String> {
    // TODO: Persist settings to disk
    Ok(())
}

/// Application settings
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub theme: Theme,
    pub explanation_level: ExplanationLevel,
    pub default_interval_ms: u64,
    pub default_max_hops: u8,
    pub default_timeout_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExplanationLevel {
    Simple,
    Detailed,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            explanation_level: ExplanationLevel::Simple,
            default_interval_ms: 1000,
            default_max_hops: 30,
            default_timeout_ms: 1000,
        }
    }
}
