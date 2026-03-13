import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import type { TraceConfig, TraceSession, HopSample, SessionSummary, TraceEvent, Settings } from '../types/global';

// Re-export types
export type { TraceConfig, TraceSession, HopSample, SessionSummary, TraceEvent, Settings };

// Tauri command wrappers
export async function resolveHost(target: string): Promise<string> {
  return invoke<string>('resolve_host', { target });
}

export async function startTrace(config: TraceConfig): Promise<TraceSession> {
  return invoke<TraceSession>('start_trace', { config });
}

export async function stopTrace(sessionId: string): Promise<void> {
  return invoke<void>('stop_trace', { sessionId });
}

export async function getSessionHops(sessionId: string): Promise<HopSample[]> {
  return invoke<HopSample[]>('get_session_hops', { sessionId });
}

export async function interpretHops(hops: HopSample[]): Promise<HopSample[]> {
  return invoke<HopSample[]>('interpret_hops', { hops });
}

export async function exportJson(sessionId: string): Promise<string> {
  return invoke<string>('export_json', { sessionId });
}

export async function exportHtml(sessionId: string): Promise<string> {
  return invoke<string>('export_html', { sessionId });
}

export async function getSettings(): Promise<Settings> {
  return invoke<Settings>('get_settings');
}

export async function updateSettings(settings: Settings): Promise<void> {
  return invoke<void>('update_settings', { settings });
}

// Event listener helper
export function onTraceEvent(callback: (event: TraceEvent) => void): Promise<UnlistenFn> {
  return listen<TraceEvent>('trace-event', (event) => {
    callback(event.payload);
  });
}
