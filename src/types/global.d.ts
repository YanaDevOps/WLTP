// Global type declarations for WLTP
export interface TraceConfig {
  target: string;
  protocol: 'icmp' | 'udp' | 'tcp';
  intervalMs: number;
  maxHops: number;
  timeoutMs: number;
  count: number;
}

export type SessionState = 'initializing' | 'running' | 'paused' | 'completed' | 'error';

export interface TraceSession {
  id: string;
  config: TraceConfig;
  targetIp: string | null;
  state: SessionState;
  startedAt: string | null;
  endedAt: string | null;
  error: string | null;
}

export type Severity = 'ok' | 'warning' | 'critical' | 'unknown';

export interface HopStats {
  sent: number;
  received: number;
  lossPercent: number;
  bestMs: number | null;
  worstMs: number | null;
  avgMs: number | null;
  lastMs: number | null;
  jitterMs: number | null;
}

export interface HopInterpretation {
  severity: Severity;
  headline: string;
  explanation: string;
  probableCauses: string[];
  confidence: number;
}

export interface HopSample {
  index: number;
  hostname: string | null;
  ip: string | null;
  stats: HopStats;
  status: Severity;
  interpretation: HopInterpretation | null;
}

export interface SessionSummary {
  overallStatus: Severity;
  primaryFinding: string;
  secondaryFindings: string[];
  recommendedNextSteps: string[];
  problemHopIndex: number | null;
  destinationReachable: boolean;
}

export interface TraceEvent {
  type: 'session_started' | 'hop_discovered' | 'hop_response' | 'hop_timeout' | 'hop_stats_update' | 'session_completed' | 'session_error' | 'dns_resolved';
  session: TraceSession;
  hop: HopSample;
  sessionId: string;
  hopIndex: number;
  latencyMs: number;
  stats: HopStats;
  summary: SessionSummary;
  hops: HopSample[];
  error: string;
  hostname: string;
  ip: string;
}

export interface Settings {
  language: 'en' | 'ru';
  theme: 'system' | 'light' | 'dark';
  explanationLevel: 'simple' | 'detailed';
  defaultIntervalMs: number;
  defaultMaxHops: number;
  defaultTimeoutMs: number;
}
