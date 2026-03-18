import { useState, useEffect, useCallback } from 'react';
import {
  startTrace,
  stopTrace,
  onTraceEvent,
  exportHtml,
  exportJson,
  getSettings,
  updateSettings,
  type TraceConfig,
  type TraceSession,
  type HopSample,
  type SessionSummary,
  type TraceEvent,
  type Settings,
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

  // Load settings on mount
  useEffect(() => {
    getSettings().then(setSettings).catch(console.error);
  }, []);

  // Subscribe to trace events
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
            setHops((prev) => {
              const existing = prev.findIndex((h) => h.index === event.hop!.index);
              if (existing >= 0) {
                const updated = [...prev];
                updated[existing] = event.hop!;
                return updated;
              }
              return [...prev, event.hop!].sort((a, b) => a.index - b.index);
            });
          }
          break;

        case 'hop_stats_update':
          if (event.hopIndex !== undefined && event.stats) {
            setHops((prev) => {
              const idx = prev.findIndex((h) => h.index === event.hopIndex);
              if (idx >= 0) {
                const updated = [...prev];
                updated[idx] = { ...updated[idx], stats: event.stats! };
                return updated;
              }
              return prev;
            });
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
      if (unlisten) unlisten();
    };
  }, []);

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
      count: 0, // Continuous until stopped
    };

    try {
      await startTrace(config);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [target, settings]);

  const handleStopTrace = useCallback(async () => {
    if (session) {
      try {
        await stopTrace(session.id);
        setIsRunning(false);
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      }
    }
  }, [session]);

  const handleExportHtml = useCallback(async () => {
    if (!session || !summary) return;
    try {
      const html = await exportHtml(summary, hops, session.config);
      const blob = new Blob([html], { type: 'text/html' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `wltp-report-${target}-${new Date().toISOString().slice(0, 10)}.html`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [hops, session, summary, target]);

  const handleExportJson = useCallback(async () => {
    if (!session || !summary) return;
    try {
      const json = await exportJson(summary, hops, session.config);
      const blob = new Blob([json], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `wltp-report-${target}-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
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

  // Apply theme
  useEffect(() => {
    const root = document.documentElement;
    if (settings.theme === 'dark') {
      root.classList.add('dark');
    } else if (settings.theme === 'light') {
      root.classList.remove('dark');
    } else {
      // System preference
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      if (prefersDark) {
        root.classList.add('dark');
      } else {
        root.classList.remove('dark');
      }
    }
  }, [settings.theme]);

  return (
    <div className="min-h-screen bg-gray-50 dark:bg-gray-900 text-gray-900 dark:text-gray-100">
      <header className="bg-white dark:bg-gray-800 shadow-sm border-b border-gray-200 dark:border-gray-700">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex items-center justify-between h-16">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 bg-blue-600 rounded-lg flex items-center justify-center">
                <span className="text-white font-bold text-sm">W</span>
              </div>
              <h1 className="text-xl font-semibold">WLTP</h1>
            </div>

            <nav className="flex items-center gap-4">
              <button
                onClick={() => setView('main')}
                className={`px-3 py-2 text-sm font-medium rounded-md transition-colors ${
                  view === 'main'
                    ? 'bg-blue-100 dark:bg-blue-900 text-blue-700 dark:text-blue-300'
                    : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100'
                }`}
              >
                Diagnose
              </button>
              <button
                onClick={() => setView('settings')}
                className={`px-3 py-2 text-sm font-medium rounded-md transition-colors ${
                  view === 'settings'
                    ? 'bg-blue-100 dark:bg-blue-900 text-blue-700 dark:text-blue-300'
                    : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100'
                }`}
              >
                Settings
              </button>
            </nav>
          </div>
        </div>
      </header>

      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
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
          <SettingsView
            settings={settings}
            onChange={handleSettingsChange}
          />
        )}
      </main>
    </div>
  );
}

// Main diagnostic view
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
    <div className="space-y-6">
      {/* Input Section */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        <div className="flex flex-col sm:flex-row gap-4">
          <div className="flex-1">
            <label htmlFor="target" className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              Target Host or IP
            </label>
            <input
              type="text"
              id="target"
              value={target}
              onChange={(e) => setTarget(e.target.value)}
              placeholder="e.g., google.com or 8.8.8.8"
              disabled={isRunning}
              className="w-full px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-md shadow-sm focus:ring-blue-500 focus:border-blue-500 dark:bg-gray-700 dark:text-white disabled:opacity-50"
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
                className="px-6 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 transition-colors font-medium"
              >
                Start Trace
              </button>
            ) : (
              <button
                onClick={onStop}
                className="px-6 py-2 bg-red-600 text-white rounded-md hover:bg-red-700 transition-colors font-medium"
              >
                Stop
              </button>
            )}
          </div>
        </div>

        {error && (
          <div className="mt-4 p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-md">
            <p className="text-sm text-red-700 dark:text-red-300">{error}</p>
          </div>
        )}

        {session && (
          <div className="mt-4 flex items-center gap-4 text-sm text-gray-600 dark:text-gray-400">
            {session.targetIp && (
              <span>Resolved: <code className="bg-gray-100 dark:bg-gray-700 px-1 rounded">{session.targetIp}</code></span>
            )}
            {session.startedAt && (
              <span>Started: {new Date(session.startedAt).toLocaleTimeString()}</span>
            )}
          </div>
        )}
      </div>

      {/* Summary Section */}
      {summary && (
        <SummaryCard summary={summary} />
      )}

      {/* Hops Table */}
      {hops.length > 0 && (
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow overflow-hidden">
          <div className="p-4 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between">
            <h2 className="text-lg font-semibold">Network Route</h2>
            <div className="flex gap-2">
              <button
                onClick={onExportHtml}
                className="px-3 py-1.5 text-sm bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-md transition-colors"
              >
                Export HTML
              </button>
              <button
                onClick={onExportJson}
                className="px-3 py-1.5 text-sm bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-md transition-colors"
              >
                Export JSON
              </button>
            </div>
          </div>

          <div className="overflow-x-auto">
            <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-700">
              <thead className="bg-gray-50 dark:bg-gray-900">
                <tr>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Status</th>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Hop</th>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Host</th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Loss%</th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Sent</th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Recv</th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Best</th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Avg</th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Worst</th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Last</th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Jitter</th>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Interpretation</th>
                </tr>
              </thead>
              <tbody className="bg-white dark:bg-gray-800 divide-y divide-gray-200 dark:divide-gray-700">
                {hops.map((hop) => (
                  <HopRow key={hop.index} hop={hop} />
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Empty State */}
      {!isRunning && hops.length === 0 && !error && (
        <div className="text-center py-12">
          <div className="text-gray-400 dark:text-gray-600 mb-4">
            <svg className="w-16 h-16 mx-auto" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z" />
            </svg>
          </div>
          <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">Ready to Diagnose</h3>
          <p className="text-gray-600 dark:text-gray-400 max-w-md mx-auto">
            Enter a hostname or IP address above and click "Start Trace" to begin network diagnostics.
          </p>
        </div>
      )}
    </div>
  );
}

// Summary card component
function SummaryCard({ summary }: { summary: SessionSummary }) {
  const statusColors = {
    ok: 'bg-green-100 dark:bg-green-900/30 text-green-800 dark:text-green-300 border-green-200 dark:border-green-800',
    warning: 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-800 dark:text-yellow-300 border-yellow-200 dark:border-yellow-800',
    critical: 'bg-red-100 dark:bg-red-900/30 text-red-800 dark:text-red-300 border-red-200 dark:border-red-800',
    unknown: 'bg-gray-100 dark:bg-gray-900/30 text-gray-800 dark:text-gray-300 border-gray-200 dark:border-gray-800',
  };

  const statusIcon = {
    ok: '✓',
    warning: '⚠',
    critical: '✗',
    unknown: '?',
  };

  return (
    <div className={`rounded-lg border p-6 ${statusColors[summary.overallStatus]}`}>
      <div className="flex items-start gap-4">
        <div className="text-2xl">{statusIcon[summary.overallStatus]}</div>
        <div className="flex-1">
          <h2 className="text-lg font-semibold mb-2">{summary.primaryFinding}</h2>

          {summary.secondaryFindings.length > 0 && (
            <div className="mb-4">
              <h3 className="text-sm font-medium mb-1 opacity-80">Observations:</h3>
              <ul className="list-disc list-inside text-sm space-y-1">
                {summary.secondaryFindings.map((finding, i) => (
                  <li key={i}>{finding}</li>
                ))}
              </ul>
            </div>
          )}

          {summary.recommendedNextSteps.length > 0 && (
            <div>
              <h3 className="text-sm font-medium mb-1 opacity-80">Recommended Actions:</h3>
              <ul className="list-disc list-inside text-sm space-y-1">
                {summary.recommendedNextSteps.map((step, i) => (
                  <li key={i}>{step}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// Hop row component
function HopRow({ hop }: { hop: HopSample }) {
  const statusColors = {
    ok: 'text-green-500',
    warning: 'text-yellow-500',
    critical: 'text-red-500',
    unknown: 'text-gray-400',
  };

  const formatMs = (value: number | null | undefined): string => {
    if (value === null || value === undefined) return '-';
    return `${value.toFixed(1)}`;
  };

  const hostDisplay = hop.hostname || hop.ip || '*';
  const ipDisplay = hop.ip && hop.hostname ? hop.ip : '';

  return (
    <tr className="hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors">
      <td className="px-4 py-3 whitespace-nowrap">
        <span className={`text-lg ${statusColors[hop.status]}`}>●</span>
      </td>
      <td className="px-4 py-3 whitespace-nowrap text-sm font-medium text-gray-900 dark:text-gray-100">
        {hop.index}
      </td>
      <td className="px-4 py-3 whitespace-nowrap">
        <div className="text-sm">
          <div className="font-medium text-gray-900 dark:text-gray-100">{hostDisplay}</div>
          {ipDisplay && <div className="text-gray-500 dark:text-gray-400 text-xs">{ipDisplay}</div>}
        </div>
      </td>
      <td className="px-4 py-3 whitespace-nowrap text-sm text-right tabular-nums">
        <span className={hop.stats.lossPercent > 5 ? 'text-red-600 dark:text-red-400 font-medium' : ''}>
          {hop.stats.lossPercent.toFixed(1)}%
        </span>
      </td>
      <td className="px-4 py-3 whitespace-nowrap text-sm text-right tabular-nums text-gray-600 dark:text-gray-400">
        {hop.stats.sent}
      </td>
      <td className="px-4 py-3 whitespace-nowrap text-sm text-right tabular-nums text-gray-600 dark:text-gray-400">
        {hop.stats.received}
      </td>
      <td className="px-4 py-3 whitespace-nowrap text-sm text-right tabular-nums text-gray-600 dark:text-gray-400">
        {formatMs(hop.stats.bestMs)}
      </td>
      <td className="px-4 py-3 whitespace-nowrap text-sm text-right tabular-nums">
        <span className={hop.stats.avgMs && hop.stats.avgMs > 100 ? 'text-yellow-600 dark:text-yellow-400' : ''}>
          {formatMs(hop.stats.avgMs)}
        </span>
      </td>
      <td className="px-4 py-3 whitespace-nowrap text-sm text-right tabular-nums text-gray-600 dark:text-gray-400">
        {formatMs(hop.stats.worstMs)}
      </td>
      <td className="px-4 py-3 whitespace-nowrap text-sm text-right tabular-nums text-gray-600 dark:text-gray-400">
        {formatMs(hop.stats.lastMs)}
      </td>
      <td className="px-4 py-3 whitespace-nowrap text-sm text-right tabular-nums">
        <span className={hop.stats.jitterMs && hop.stats.jitterMs > 30 ? 'text-yellow-600 dark:text-yellow-400' : ''}>
          {formatMs(hop.stats.jitterMs)}
        </span>
      </td>
      <td className="px-4 py-3 text-sm max-w-xs">
        {hop.interpretation && (
          <div>
            <div className="font-medium text-gray-900 dark:text-gray-100">{hop.interpretation.headline}</div>
            <div className="text-xs text-gray-500 dark:text-gray-400 mt-0.5 line-clamp-2">
              {hop.interpretation.explanation}
            </div>
          </div>
        )}
      </td>
    </tr>
  );
}

// Settings view
function SettingsView({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange: (settings: Settings) => void;
}) {
  return (
    <div className="max-w-2xl mx-auto">
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        <h2 className="text-lg font-semibold mb-6">Settings</h2>

        <div className="space-y-6">
          {/* Theme */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Theme
            </label>
            <select
              value={settings.theme}
              onChange={(e) => onChange({ ...settings, theme: e.target.value as Settings['theme'] })}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md shadow-sm focus:ring-blue-500 focus:border-blue-500 dark:bg-gray-700 dark:text-white"
            >
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </div>

          {/* Explanation Level */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Explanation Level
            </label>
            <select
              value={settings.explanationLevel}
              onChange={(e) => onChange({ ...settings, explanationLevel: e.target.value as Settings['explanationLevel'] })}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md shadow-sm focus:ring-blue-500 focus:border-blue-500 dark:bg-gray-700 dark:text-white"
            >
              <option value="simple">Simple (for beginners)</option>
              <option value="detailed">Detailed (for advanced users)</option>
            </select>
          </div>

          {/* Default Interval */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Probe Interval (ms)
            </label>
            <input
              type="number"
              value={settings.defaultIntervalMs}
              onChange={(e) => onChange({ ...settings, defaultIntervalMs: parseInt(e.target.value) || 1000 })}
              min={100}
              max={10000}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md shadow-sm focus:ring-blue-500 focus:border-blue-500 dark:bg-gray-700 dark:text-white"
            />
            <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
              Time between probes (100-10000 ms)
            </p>
          </div>

          {/* Max Hops */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Maximum Hops
            </label>
            <input
              type="number"
              value={settings.defaultMaxHops}
              onChange={(e) => onChange({ ...settings, defaultMaxHops: parseInt(e.target.value) || 30 })}
              min={1}
              max={64}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md shadow-sm focus:ring-blue-500 focus:border-blue-500 dark:bg-gray-700 dark:text-white"
            />
            <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
              Maximum number of hops to trace (1-64)
            </p>
          </div>

          {/* Timeout */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Probe Timeout (ms)
            </label>
            <input
              type="number"
              value={settings.defaultTimeoutMs}
              onChange={(e) => onChange({ ...settings, defaultTimeoutMs: parseInt(e.target.value) || 1000 })}
              min={100}
              max={10000}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md shadow-sm focus:ring-blue-500 focus:border-blue-500 dark:bg-gray-700 dark:text-white"
            />
            <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
              Timeout for each probe (100-10000 ms)
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}

export default App;
