import { useCallback, useEffect, useState, type ReactNode } from 'react';
import {
  exportHtml,
  exportJson,
  getSessionHops,
  getSettings,
  interpretHops,
  onTraceEvent,
  startTrace,
  stopTrace,
  updateSettings,
  type HopSample,
  type SessionSummary,
  type Settings,
  type TraceConfig,
  type TraceEvent,
  type TraceSession,
} from './lib/tauri';

type View = 'main' | 'settings';

function App() {
  const [view, setView] = useState<View>('main');
  const [target, setTarget] = useState('');
  const [session, setSession] = useState<TraceSession | null>(null);
  const [hops, setHops] = useState<HopSample[]>([]);
  const [summary, setSummary] = useState<SessionSummary | null>(null);
  const [isRunning, setIsRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState<Settings>({
    theme: 'system',
    explanationLevel: 'simple',
    defaultIntervalMs: 1000,
    defaultMaxHops: 30,
    defaultTimeoutMs: 1000,
  });

  useEffect(() => {
    getSettings().then(setSettings).catch(console.error);
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    onTraceEvent((event: TraceEvent) => {
      switch (event.type) {
        case 'session_started':
          if (event.session) {
            setSession(event.session);
            setIsRunning(true);
            setError(null);
          }
          break;

        case 'hop_discovered':
          if (event.hop) {
            setHops((prev) => upsertHop(prev, event.hop));
          }
          break;

        case 'hop_stats_update':
          if (event.hopIndex !== undefined && event.stats) {
            setHops((prev) =>
              prev.map((hop) =>
                hop.index === event.hopIndex ? { ...hop, stats: event.stats! } : hop,
              ),
            );
          }
          break;

        case 'session_completed':
          setIsRunning(false);
          if (event.summary) {
            setSummary(event.summary);
          }
          if (event.hops) {
            setHops(event.hops);
          }
          break;

        case 'session_error':
          setIsRunning(false);
          if (event.error) {
            setError(event.error);
          }
          break;
      }
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  useEffect(() => {
    if (!session || !isRunning) {
      return;
    }

    let disposed = false;

    const pollHops = async () => {
      try {
        const currentHops = await getSessionHops(session.id);
        if (disposed) {
          return;
        }

        setHops(currentHops);

        if (currentHops.length > 0) {
          const currentSummary = await interpretHops(currentHops);
          if (!disposed) {
            setSummary(currentSummary);
          }
        } else {
          setSummary(null);
        }
      } catch {
        if (!disposed) {
          setIsRunning(false);
        }
      }
    };

    pollHops();
    const intervalId = window.setInterval(pollHops, 1000);

    return () => {
      disposed = true;
      window.clearInterval(intervalId);
    };
  }, [isRunning, session]);

  const handleStartTrace = useCallback(async () => {
    if (!target.trim()) {
      setError('Please enter a target host or IP address');
      return;
    }

    setError(null);
    setHops([]);
    setSummary(null);

    const config: TraceConfig = {
      target: target.trim(),
      protocol: 'icmp',
      intervalMs: settings.defaultIntervalMs,
      maxHops: settings.defaultMaxHops,
      timeoutMs: settings.defaultTimeoutMs,
      count: 0,
    };

    try {
      const startedSession = await startTrace(config);
      setSession(startedSession);
      setIsRunning(true);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [settings, target]);

  const handleStopTrace = useCallback(async () => {
    if (!session) {
      return;
    }

    try {
      await stopTrace(session.id);
      setIsRunning(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [session]);

  const handleExportHtml = useCallback(async () => {
    if (!session || !summary) {
      return;
    }

    try {
      const html = await exportHtml(summary, hops, session.config);
      const blob = new Blob([html], { type: 'text/html' });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `wltp-report-${target}-${new Date().toISOString().slice(0, 10)}.html`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [hops, session, summary, target]);

  const handleExportJson = useCallback(async () => {
    if (!session || !summary) {
      return;
    }

    try {
      const json = await exportJson(summary, hops, session.config);
      const blob = new Blob([json], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `wltp-report-${target}-${new Date().toISOString().slice(0, 10)}.json`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [hops, session, summary, target]);

  const handleSettingsChange = useCallback(async (newSettings: Settings) => {
    setSettings(newSettings);
    try {
      await updateSettings(newSettings);
    } catch (err) {
      console.error('Failed to save settings:', err);
    }
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    if (settings.theme === 'dark') {
      root.classList.add('dark');
      return;
    }

    if (settings.theme === 'light') {
      root.classList.remove('dark');
      return;
    }

    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    root.classList.toggle('dark', prefersDark);
  }, [settings.theme]);

  return (
    <div className="flex h-screen overflow-hidden bg-slate-100 text-slate-900 dark:bg-slate-950 dark:text-slate-100">
      <div className="mx-auto flex h-full w-full max-w-[1600px] flex-col">
        <header className="shrink-0 border-b border-slate-200 bg-white/95 dark:border-slate-800 dark:bg-slate-900/95">
          <div className="flex h-12 items-center justify-between px-3 sm:px-4">
            <div className="flex items-center gap-3">
              <div className="flex h-7 w-7 items-center justify-center rounded-md bg-blue-600">
                <span className="text-xs font-bold text-white">W</span>
              </div>
              <div>
                <h1 className="text-sm font-semibold tracking-wide">WLTP</h1>
                <p className="text-[11px] text-slate-500 dark:text-slate-400">
                  WinMTR-style route diagnostics
                </p>
              </div>
            </div>

            <nav className="flex items-center gap-2">
              <NavButton active={view === 'main'} onClick={() => setView('main')}>
                Diagnose
              </NavButton>
              <NavButton active={view === 'settings'} onClick={() => setView('settings')}>
                Settings
              </NavButton>
            </nav>
          </div>
        </header>

        <main className="flex min-h-0 flex-1 flex-col p-3 sm:p-4">
          {view === 'main' ? (
            <MainView
              target={target}
              setTarget={setTarget}
              isRunning={isRunning}
              error={error}
              hops={hops}
              summary={summary}
              onStart={handleStartTrace}
              onStop={handleStopTrace}
              onExportHtml={handleExportHtml}
              onExportJson={handleExportJson}
              session={session}
            />
          ) : (
            <SettingsView settings={settings} onChange={handleSettingsChange} />
          )}
        </main>
      </div>
    </div>
  );
}

function NavButton({
  active,
  children,
  onClick,
}: {
  active: boolean;
  children: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`rounded-md px-3 py-1.5 text-xs font-medium transition-colors ${
        active
          ? 'bg-blue-100 text-blue-700 dark:bg-blue-950 dark:text-blue-300'
          : 'text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100'
      }`}
    >
      {children}
    </button>
  );
}

interface MainViewProps {
  target: string;
  setTarget: (value: string) => void;
  isRunning: boolean;
  error: string | null;
  hops: HopSample[];
  summary: SessionSummary | null;
  onStart: () => void;
  onStop: () => void;
  onExportHtml: () => void;
  onExportJson: () => void;
  session: TraceSession | null;
}

function MainView({
  target,
  setTarget,
  isRunning,
  error,
  hops,
  summary,
  onStart,
  onStop,
  onExportHtml,
  onExportJson,
  session,
}: MainViewProps) {
  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <section className="shrink-0 rounded-lg border border-slate-200 bg-white shadow-sm dark:border-slate-800 dark:bg-slate-900">
        <div className="flex flex-col gap-3 p-4">
          <div className="flex flex-col gap-3 lg:flex-row lg:items-end">
            <div className="flex-1">
              <label
                htmlFor="target"
                className="mb-1 block text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400"
              >
                Target Host or IP
              </label>
              <input
                id="target"
                type="text"
                value={target}
                onChange={(e) => setTarget(e.target.value)}
                placeholder="e.g., google.com or 8.8.8.8"
                disabled={isRunning}
                className="w-full rounded-md border border-slate-300 bg-slate-50 px-3 py-2 text-sm shadow-sm outline-none transition focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-white"
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !isRunning) {
                    onStart();
                  }
                }}
              />
            </div>

            <div className="flex items-end gap-2">
              {!isRunning ? (
                <button
                  onClick={onStart}
                  className="rounded-md bg-blue-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-blue-700"
                >
                  Start Trace
                </button>
              ) : (
                <button
                  onClick={onStop}
                  className="rounded-md bg-red-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-red-700"
                >
                  Stop
                </button>
              )}
            </div>
          </div>

          {error && (
            <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 dark:border-red-900 dark:bg-red-950/40">
              <p className="text-sm text-red-700 dark:text-red-300">{error}</p>
            </div>
          )}

          {session && (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-slate-600 dark:text-slate-400">
              {session.targetIp && (
                <span>
                  Resolved:{' '}
                  <code className="rounded bg-slate-100 px-1.5 py-0.5 dark:bg-slate-800">
                    {session.targetIp}
                  </code>
                </span>
              )}
              {session.startedAt && (
                <span>Started: {new Date(session.startedAt).toLocaleTimeString()}</span>
              )}
            </div>
          )}
        </div>
      </section>

      {!isRunning && summary && <SummaryCard summary={summary} />}

      {isRunning && (
        <section className="shrink-0 rounded-lg border border-blue-200 bg-blue-50 px-4 py-3 text-blue-900 shadow-sm dark:border-blue-900 dark:bg-blue-950/30 dark:text-blue-200">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-sm font-semibold">Trace in progress</h2>
              <p className="text-xs opacity-80">
                Final diagnosis appears after the route settles or when you stop the trace.
              </p>
            </div>
            <div className="text-xs font-medium opacity-80">
              {hops.length > 0 ? `${hops.length} hops discovered` : 'Discovering hops'}
            </div>
          </div>
        </section>
      )}

      {hops.length > 0 && (
        <section className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-slate-200 bg-white shadow-sm dark:border-slate-800 dark:bg-slate-900">
          <div className="flex shrink-0 items-center justify-between gap-3 border-b border-slate-200 px-4 py-2.5 dark:border-slate-800">
            <div>
              <h2 className="text-sm font-semibold">Network Route</h2>
              <p className="text-xs text-slate-500 dark:text-slate-400">
                Scroll stays inside the hops panel.
              </p>
            </div>

            <div className="flex gap-2">
              <button
                onClick={onExportHtml}
                className="rounded-md bg-slate-100 px-3 py-1.5 text-xs font-medium transition-colors hover:bg-slate-200 dark:bg-slate-800 dark:hover:bg-slate-700"
              >
                Export HTML
              </button>
              <button
                onClick={onExportJson}
                className="rounded-md bg-slate-100 px-3 py-1.5 text-xs font-medium transition-colors hover:bg-slate-200 dark:bg-slate-800 dark:hover:bg-slate-700"
              >
                Export JSON
              </button>
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-auto">
            <table className="min-w-full divide-y divide-slate-200 dark:divide-slate-800">
              <thead className="sticky top-0 z-10 bg-slate-50 dark:bg-slate-950">
                <tr>
                  <HeaderCell>Status</HeaderCell>
                  <HeaderCell>Hop</HeaderCell>
                  <HeaderCell>Host</HeaderCell>
                  <HeaderCell align="right">Loss%</HeaderCell>
                  <HeaderCell align="right">Sent</HeaderCell>
                  <HeaderCell align="right">Recv</HeaderCell>
                  <HeaderCell align="right">Best</HeaderCell>
                  <HeaderCell align="right">Avg</HeaderCell>
                  <HeaderCell align="right">Worst</HeaderCell>
                  <HeaderCell align="right">Last</HeaderCell>
                  <HeaderCell align="right">Jitter</HeaderCell>
                  <HeaderCell>Interpretation</HeaderCell>
                </tr>
              </thead>

              <tbody className="divide-y divide-slate-200 bg-white dark:divide-slate-800 dark:bg-slate-900">
                {hops.map((hop) => (
                  <HopRow key={hop.index} hop={hop} />
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}

      {isRunning && hops.length === 0 && !error && (
        <section className="rounded-lg border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-800 dark:bg-slate-900">
          <h2 className="mb-1 text-sm font-semibold">Trace is running</h2>
          <p className="text-sm text-slate-600 dark:text-slate-400">
            Discovering route and waiting for the first hop responses.
          </p>
        </section>
      )}

      {!isRunning && hops.length === 0 && !error && (
        <section className="flex flex-1 items-center justify-center rounded-lg border border-dashed border-slate-300 bg-white/60 p-8 text-center dark:border-slate-700 dark:bg-slate-900/40">
          <div>
            <div className="mb-4 text-slate-400 dark:text-slate-600">
              <svg className="mx-auto h-16 w-16" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={1.5}
                  d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z"
                />
              </svg>
            </div>
            <h3 className="mb-2 text-base font-semibold text-slate-900 dark:text-slate-100">
              Ready to Diagnose
            </h3>
            <p className="mx-auto max-w-md text-sm text-slate-600 dark:text-slate-400">
              Enter a hostname or IP address above and click Start Trace to begin network diagnostics.
            </p>
          </div>
        </section>
      )}
    </div>
  );
}

function HeaderCell({
  children,
  align = 'left',
}: {
  children: ReactNode;
  align?: 'left' | 'right';
}) {
  return (
    <th
      className={`px-3 py-2 text-[11px] font-semibold uppercase tracking-wider text-slate-500 dark:text-slate-400 ${
        align === 'right' ? 'text-right' : 'text-left'
      }`}
    >
      {children}
    </th>
  );
}

function SummaryCard({ summary }: { summary: SessionSummary }) {
  const statusColors: Record<string, string> = {
    ok: 'border-green-200 bg-green-50 text-green-800 dark:border-green-900 dark:bg-green-950/40 dark:text-green-300',
    warning:
      'border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-900 dark:bg-amber-950/40 dark:text-amber-300',
    critical: 'border-red-200 bg-red-50 text-red-800 dark:border-red-900 dark:bg-red-950/40 dark:text-red-300',
    unknown: 'border-slate-200 bg-slate-100 text-slate-800 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-300',
  };

  const statusIcon: Record<string, string> = {
    ok: 'OK',
    warning: '!',
    critical: 'X',
    unknown: '?',
  };

  return (
    <section className={`shrink-0 rounded-lg border p-4 ${statusColors[summary.overallStatus]}`}>
      <div className="flex items-start gap-3">
        <div className="flex h-7 min-w-7 items-center justify-center rounded-md border border-current/20 bg-white/50 text-xs font-bold dark:bg-transparent">
          {statusIcon[summary.overallStatus]}
        </div>

        <div className="flex-1">
          <h2 className="mb-2 text-sm font-semibold">{summary.primaryFinding}</h2>

          {summary.secondaryFindings.length > 0 && (
            <div className="mb-3">
              <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide opacity-80">
                Observations
              </h3>
              <ul className="list-disc list-inside space-y-1 text-sm">
                {summary.secondaryFindings.map((finding, index) => (
                  <li key={index}>{finding}</li>
                ))}
              </ul>
            </div>
          )}

          {summary.recommendedNextSteps.length > 0 && (
            <div>
              <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide opacity-80">
                Recommended Actions
              </h3>
              <ul className="list-disc list-inside space-y-1 text-sm">
                {summary.recommendedNextSteps.map((step, index) => (
                  <li key={index}>{step}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

function HopRow({ hop }: { hop: HopSample }) {
  const statusColors: Record<string, string> = {
    ok: 'bg-green-500',
    warning: 'bg-amber-500',
    critical: 'bg-red-500',
    unknown: 'bg-slate-400',
  };

  const formatMs = (value: number | null | undefined): string => {
    if (value === null || value === undefined) {
      return '-';
    }
    return value.toFixed(1);
  };

  const hostDisplay = hop.hostname || hop.ip || '*';
  const ipDisplay = hop.ip && hop.hostname ? hop.ip : '';

  return (
    <tr className="transition-colors hover:bg-slate-50 dark:hover:bg-slate-800/70">
      <td className="whitespace-nowrap px-3 py-2.5">
        <span className={`inline-block h-2.5 w-2.5 rounded-full ${statusColors[hop.status]}`} />
      </td>
      <td className="whitespace-nowrap px-3 py-2.5 text-sm font-semibold text-slate-900 dark:text-slate-100">
        {hop.index}
      </td>
      <td className="px-3 py-2.5">
        <div className="min-w-[220px] text-sm">
          <div className="truncate font-medium text-slate-900 dark:text-slate-100">
            {hostDisplay}
          </div>
          {ipDisplay && (
            <div className="truncate text-[11px] text-slate-500 dark:text-slate-400">
              {ipDisplay}
            </div>
          )}
        </div>
      </td>
      <td className="whitespace-nowrap px-3 py-2.5 text-right text-sm tabular-nums">
        <span className={hop.stats.lossPercent > 5 ? 'font-semibold text-red-600 dark:text-red-400' : ''}>
          {hop.stats.lossPercent.toFixed(1)}%
        </span>
      </td>
      <td className="whitespace-nowrap px-3 py-2.5 text-right text-sm tabular-nums text-slate-600 dark:text-slate-400">
        {hop.stats.sent}
      </td>
      <td className="whitespace-nowrap px-3 py-2.5 text-right text-sm tabular-nums text-slate-600 dark:text-slate-400">
        {hop.stats.received}
      </td>
      <td className="whitespace-nowrap px-3 py-2.5 text-right text-sm tabular-nums text-slate-600 dark:text-slate-400">
        {formatMs(hop.stats.bestMs)}
      </td>
      <td className="whitespace-nowrap px-3 py-2.5 text-right text-sm tabular-nums">
        <span className={hop.stats.avgMs && hop.stats.avgMs > 100 ? 'text-amber-600 dark:text-amber-400' : ''}>
          {formatMs(hop.stats.avgMs)}
        </span>
      </td>
      <td className="whitespace-nowrap px-3 py-2.5 text-right text-sm tabular-nums text-slate-600 dark:text-slate-400">
        {formatMs(hop.stats.worstMs)}
      </td>
      <td className="whitespace-nowrap px-3 py-2.5 text-right text-sm tabular-nums text-slate-600 dark:text-slate-400">
        {formatMs(hop.stats.lastMs)}
      </td>
      <td className="whitespace-nowrap px-3 py-2.5 text-right text-sm tabular-nums">
        <span className={hop.stats.jitterMs && hop.stats.jitterMs > 30 ? 'text-amber-600 dark:text-amber-400' : ''}>
          {formatMs(hop.stats.jitterMs)}
        </span>
      </td>
      <td className="max-w-sm px-3 py-2.5 text-sm">
        {hop.interpretation && (
          <div className="space-y-0.5">
            <div className="font-medium text-slate-900 dark:text-slate-100">
              {hop.interpretation.headline}
            </div>
            <div className="line-clamp-2 text-xs text-slate-500 dark:text-slate-400">
              {hop.interpretation.explanation}
            </div>
          </div>
        )}
      </td>
    </tr>
  );
}

function SettingsView({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange: (settings: Settings) => void;
}) {
  return (
    <div className="mx-auto h-full w-full max-w-3xl overflow-auto">
      <div className="rounded-lg border border-slate-200 bg-white p-5 shadow-sm dark:border-slate-800 dark:bg-slate-900">
        <h2 className="mb-5 text-base font-semibold">Settings</h2>

        <div className="space-y-5">
          <Field label="Theme">
            <select
              value={settings.theme}
              onChange={(e) => onChange({ ...settings, theme: e.target.value as Settings['theme'] })}
              className={inputClassName}
            >
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </Field>

          <Field label="Explanation Level">
            <select
              value={settings.explanationLevel}
              onChange={(e) =>
                onChange({
                  ...settings,
                  explanationLevel: e.target.value as Settings['explanationLevel'],
                })
              }
              className={inputClassName}
            >
              <option value="simple">Simple (for beginners)</option>
              <option value="detailed">Detailed (for advanced users)</option>
            </select>
          </Field>

          <Field label="Probe Interval (ms)" hint="Time between probes (100-10000 ms)">
            <input
              type="number"
              min={100}
              max={10000}
              value={settings.defaultIntervalMs}
              onChange={(e) =>
                onChange({
                  ...settings,
                  defaultIntervalMs: parseInt(e.target.value, 10) || 1000,
                })
              }
              className={inputClassName}
            />
          </Field>

          <Field label="Maximum Hops" hint="Maximum number of hops to trace (1-64)">
            <input
              type="number"
              min={1}
              max={64}
              value={settings.defaultMaxHops}
              onChange={(e) =>
                onChange({
                  ...settings,
                  defaultMaxHops: parseInt(e.target.value, 10) || 30,
                })
              }
              className={inputClassName}
            />
          </Field>

          <Field label="Probe Timeout (ms)" hint="Timeout for each probe (100-10000 ms)">
            <input
              type="number"
              min={100}
              max={10000}
              value={settings.defaultTimeoutMs}
              onChange={(e) =>
                onChange({
                  ...settings,
                  defaultTimeoutMs: parseInt(e.target.value, 10) || 1000,
                })
              }
              className={inputClassName}
            />
          </Field>
        </div>
      </div>
    </div>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div>
      <label className="mb-2 block text-sm font-medium text-slate-700 dark:text-slate-300">
        {label}
      </label>
      {children}
      {hint && <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">{hint}</p>}
    </div>
  );
}

function upsertHop(current: HopSample[], nextHop: HopSample): HopSample[] {
  const existingIndex = current.findIndex((hop) => hop.index === nextHop.index);
  if (existingIndex === -1) {
    return [...current, nextHop].sort((left, right) => left.index - right.index);
  }

  const updated = [...current];
  updated[existingIndex] = nextHop;
  return updated;
}

const inputClassName =
  'w-full rounded-md border border-slate-300 bg-slate-50 px-3 py-2 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-white';

export default App;
