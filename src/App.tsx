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
import appIcon from './assets/app-icon.svg';

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
    language: 'en',
    theme: 'system',
    explanationLevel: 'simple',
    defaultIntervalMs: 1000,
    defaultMaxHops: 30,
    defaultTimeoutMs: 1000,
  });
  const copy = UI_TEXT[settings.language];

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
      setError(copy.errors.emptyTarget);
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
  }, [copy.errors.emptyTarget, settings, target]);

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
    <div className="flex h-screen overflow-hidden bg-[radial-gradient(circle_at_top,#fde7d7_0%,#f6d4c5_28%,#efe5dd_58%,#e8ddd5_100%)] text-stone-900 dark:bg-[radial-gradient(circle_at_top,#4b2a2a_0%,#2b1d24_35%,#171318_72%,#0f0d12_100%)] dark:text-stone-100">
      <div className="mx-auto flex h-full w-full max-w-[1080px] flex-col">
        <header className="shrink-0 border-b border-orange-200/70 bg-white/75 backdrop-blur-md dark:border-orange-950/70 dark:bg-stone-950/60">
          <div className="flex h-11 items-center justify-between px-2.5 sm:px-3">
            <div className="flex items-center gap-2">
              <img
                src={appIcon}
                alt="WLTP"
                className="h-6 w-6 rounded-[7px] shadow-sm shadow-stone-900/10"
              />
              <div>
                <h1 className="text-[13px] font-semibold tracking-[0.08em]">WLTP</h1>
                <p className="text-[10px] text-stone-500 dark:text-stone-400">
                  {copy.appSubtitle}
                </p>
              </div>
            </div>

            <nav className="flex items-center gap-1.5">
              <NavButton active={view === 'main'} onClick={() => setView('main')}>
                {copy.navDiagnose}
              </NavButton>
              <NavButton active={view === 'settings'} onClick={() => setView('settings')}>
                {copy.navSettings}
              </NavButton>
            </nav>
          </div>
        </header>

        <main className="flex min-h-0 flex-1 flex-col p-2 sm:p-2.5">
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
              copy={copy}
            />
          ) : (
            <SettingsView settings={settings} onChange={handleSettingsChange} copy={copy} />
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
      className={`rounded-md px-2 py-1 text-[11px] font-semibold transition-colors ${
        active
          ? 'bg-gradient-to-r from-amber-100 via-orange-100 to-rose-100 text-orange-900 shadow-sm dark:from-amber-950 dark:via-orange-950 dark:to-rose-950 dark:text-orange-200'
          : 'text-stone-600 hover:bg-white/60 hover:text-stone-900 dark:text-stone-400 dark:hover:bg-stone-900/50 dark:hover:text-stone-100'
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
  copy: UICopy;
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
  copy,
}: MainViewProps) {
  return (
    <div className="flex h-full min-h-0 flex-col gap-2">
      <section className="shrink-0 rounded-md border border-orange-200/70 bg-white/82 shadow-sm shadow-orange-200/30 backdrop-blur-sm dark:border-orange-950/70 dark:bg-stone-950/72 dark:shadow-black/20">
        <div className="flex flex-col gap-2 p-2.5">
          <div className="flex flex-col gap-2 lg:flex-row lg:items-end">
            <div className="flex-1">
              <label
                htmlFor="target"
                className="mb-1 block text-[10px] font-semibold uppercase tracking-[0.14em] text-stone-500 dark:text-stone-400"
              >
                {copy.targetLabel}
              </label>
              <input
                id="target"
                type="text"
                value={target}
                onChange={(e) => setTarget(e.target.value)}
                placeholder={copy.targetPlaceholder}
                disabled={isRunning}
                className="w-full rounded-md border border-orange-200 bg-orange-50/70 px-3 py-1.5 text-[13px] shadow-sm outline-none transition focus:border-orange-400 focus:ring-2 focus:ring-orange-400/20 disabled:opacity-50 dark:border-stone-700 dark:bg-stone-900 dark:text-white"
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !isRunning) {
                    onStart();
                  }
                }}
              />
            </div>

            <div className="flex items-end gap-1">
              {!isRunning ? (
                <button
                  onClick={onStart}
                  className="rounded-md bg-gradient-to-r from-amber-500 via-orange-500 to-rose-500 px-3 py-1.5 text-[12px] font-semibold text-white shadow-sm shadow-orange-500/25 transition hover:from-amber-600 hover:via-orange-600 hover:to-rose-600"
                >
                  {copy.startTrace}
                </button>
              ) : (
                <button
                  onClick={onStop}
                  className="rounded-md bg-gradient-to-r from-rose-500 to-red-500 px-3 py-1.5 text-[12px] font-semibold text-white shadow-sm shadow-rose-500/25 transition hover:from-rose-600 hover:to-red-600"
                >
                  {copy.stopTrace}
                </button>
              )}
            </div>
          </div>

          {error && (
            <div className="rounded-md border border-rose-300 bg-rose-100/90 px-2 py-1 dark:border-rose-950 dark:bg-rose-950/20">
              <p className="text-[12px] text-rose-800 dark:text-rose-200">{error}</p>
            </div>
          )}

          {session && (
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-stone-600 dark:text-stone-400">
              {session.targetIp && (
                <span>
                  {copy.resolvedLabel}:{' '}
                  <code className="rounded bg-orange-100/80 px-1.5 py-0.5 dark:bg-stone-800">
                    {session.targetIp}
                  </code>
                </span>
              )}
              {session.startedAt && (
                <span>
                  {copy.startedLabel}: {new Date(session.startedAt).toLocaleTimeString()}
                </span>
              )}
            </div>
          )}
        </div>
      </section>

      {!isRunning && summary && <SummaryCard summary={summary} copy={copy} />}

      {isRunning && (
        <section className="shrink-0 rounded-md border border-orange-200/80 bg-gradient-to-r from-amber-50 via-orange-50 to-rose-50 px-2 py-1.5 text-orange-950 shadow-sm shadow-orange-200/30 dark:border-orange-950/80 dark:bg-gradient-to-r dark:from-amber-950/30 dark:via-orange-950/25 dark:to-rose-950/30 dark:text-orange-200">
          <div className="flex items-center gap-2">
            <h2 className="text-[12px] font-semibold">{copy.traceInProgressTitle}</h2>
            <p className="text-[11px] opacity-80">
              {copy.traceInProgressHint}
            </p>
          </div>
        </section>
      )}

      {hops.length > 0 && (
        <section className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-md border border-orange-200/70 bg-white/82 shadow-sm shadow-orange-200/25 backdrop-blur-sm dark:border-orange-950/70 dark:bg-stone-950/72">
          <div className="flex shrink-0 items-center justify-between gap-2 border-b border-orange-200/70 px-2 py-1 dark:border-orange-950/70">
            <div>
              <h2 className="text-[12px] font-semibold">{copy.networkRoute}</h2>
            </div>

            <div className="flex gap-1">
              <button
                onClick={onExportHtml}
                className="rounded-md bg-orange-100/80 px-2 py-1 text-[10px] font-semibold text-orange-900 transition hover:bg-orange-200 dark:bg-stone-800 dark:text-orange-200 dark:hover:bg-stone-700"
              >
                {copy.exportHtml}
              </button>
              <button
                onClick={onExportJson}
                className="rounded-md bg-orange-100/80 px-2 py-1 text-[10px] font-semibold text-orange-900 transition hover:bg-orange-200 dark:bg-stone-800 dark:text-orange-200 dark:hover:bg-stone-700"
                >
                {copy.exportJson}
              </button>
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden [scrollbar-width:none] [-ms-overflow-style:none] [&::-webkit-scrollbar]:hidden">
            <table className="w-full table-auto divide-y divide-orange-100 dark:divide-stone-800">
              <colgroup>
                <col className="w-7" />
                <col className="w-9" />
                <col />
                <col className="w-11" />
                <col className="w-10" />
                <col className="w-10" />
                <col className="w-11" />
                <col className="w-11" />
                <col className="w-11" />
                <col className="w-11" />
                <col className="w-12" />
                <col className="w-[32%]" />
              </colgroup>
              <thead className="sticky top-0 z-10 bg-gradient-to-r from-orange-50 to-rose-50 dark:from-stone-950 dark:to-stone-900">
                <tr>
                  <HeaderCell className="pr-2">{copy.table.status}</HeaderCell>
                  <HeaderCell className="pl-2">{copy.table.hop}</HeaderCell>
                  <HeaderCell className="pr-1">{copy.table.host}</HeaderCell>
                  <HeaderCell align="right" className="pl-1">{copy.table.loss}</HeaderCell>
                  <HeaderCell align="right">{copy.table.sent}</HeaderCell>
                  <HeaderCell align="right">{copy.table.received}</HeaderCell>
                  <HeaderCell align="right">{copy.table.best}</HeaderCell>
                  <HeaderCell align="right">{copy.table.avg}</HeaderCell>
                  <HeaderCell align="right">{copy.table.worst}</HeaderCell>
                  <HeaderCell align="right">{copy.table.last}</HeaderCell>
                  <HeaderCell align="right" className="pr-2">{copy.table.jitter}</HeaderCell>
                  <HeaderCell className="pl-3">{copy.table.interpretation}</HeaderCell>
                </tr>
              </thead>

              <tbody className="divide-y divide-orange-100 bg-white/90 dark:divide-stone-800 dark:bg-stone-950/80">
                {hops.map((hop, index) => (
                  <HopRow key={hop.index} hop={hop} isDestination={index === hops.length - 1} />
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}

      {isRunning && hops.length === 0 && !error && (
        <section className="rounded-md border border-orange-200/70 bg-white/82 p-2.5 shadow-sm shadow-orange-200/20 dark:border-orange-950/70 dark:bg-stone-950/72">
          <h2 className="mb-1 text-[12px] font-semibold">{copy.traceRunningTitle}</h2>
          <p className="text-[12px] text-stone-600 dark:text-stone-400">
            {copy.traceRunningHint}
          </p>
        </section>
      )}

      {!isRunning && hops.length === 0 && !error && (
        <section className="flex flex-1 items-center justify-center rounded-md border border-dashed border-orange-300/80 bg-white/60 p-4 text-center shadow-inner shadow-orange-100/30 dark:border-orange-900/70 dark:bg-stone-950/45">
          <div>
            <div className="mb-3 text-orange-300 dark:text-stone-600">
              <svg className="mx-auto h-10 w-10" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={1.5}
                  d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z"
                />
              </svg>
            </div>
            <h3 className="mb-1.5 text-[13px] font-semibold text-stone-900 dark:text-stone-100">
              {copy.emptyStateTitle}
            </h3>
            <p className="mx-auto max-w-md text-[12px] text-stone-600 dark:text-stone-400">
              {copy.emptyStateText}
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
  className = '',
}: {
  children: ReactNode;
  align?: 'left' | 'right';
  className?: string;
}) {
  return (
    <th
      className={`px-1.5 py-1.5 text-[9px] font-semibold uppercase tracking-[0.08em] text-stone-500 dark:text-stone-400 ${className} ${
        align === 'right' ? 'text-right' : 'text-left'
      }`}
    >
      {children}
    </th>
  );
}

function SummaryCard({ summary, copy }: { summary: SessionSummary; copy: UICopy }) {
  const statusColors: Record<string, string> = {
    ok: 'border-emerald-300 bg-emerald-100/90 text-emerald-950 dark:border-emerald-900 dark:bg-emerald-950/35 dark:text-emerald-300',
    warning:
      'border-amber-200 bg-gradient-to-r from-amber-50 to-orange-50 text-amber-900 dark:border-amber-900 dark:bg-amber-950/40 dark:text-amber-300',
    critical: 'border-rose-300 bg-gradient-to-r from-rose-100 to-orange-100 text-rose-950 dark:border-rose-950 dark:bg-rose-950/20 dark:text-rose-200',
    unknown: 'border-orange-200 bg-orange-50/80 text-stone-800 dark:border-stone-800 dark:bg-stone-900 dark:text-stone-300',
  };

  const statusIcon: Record<string, string> = {
    ok: 'OK',
    warning: '!',
    critical: 'X',
    unknown: '?',
  };

  return (
    <section className={`shrink-0 rounded-md border p-2 ${statusColors[summary.overallStatus]}`}>
      <div className="flex items-start gap-2">
        <div className="flex h-6 min-w-6 items-center justify-center rounded-md border border-current/20 bg-white/50 text-[10px] font-bold dark:bg-transparent">
          {statusIcon[summary.overallStatus]}
        </div>

        <div className="flex-1">
          <h2 className="mb-1 text-[12px] font-semibold">{summary.primaryFinding}</h2>

          {summary.secondaryFindings.length > 0 && (
            <div className="mb-3">
              <h3 className="mb-1 text-[10px] font-semibold uppercase tracking-wide opacity-80">
                {copy.summaryObservations}
              </h3>
              <ul className="list-disc list-inside space-y-0.5 text-[12px]">
                {summary.secondaryFindings.map((finding, index) => (
                  <li key={index}>{finding}</li>
                ))}
              </ul>
            </div>
          )}

          {summary.recommendedNextSteps.length > 0 && (
            <div>
              <h3 className="mb-1 text-[10px] font-semibold uppercase tracking-wide opacity-80">
                {copy.summaryActions}
              </h3>
              <ul className="list-disc list-inside space-y-0.5 text-[12px]">
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

function HopRow({ hop, isDestination }: { hop: HopSample; isDestination: boolean }) {
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
  const destinationHealthy = isDestination && hop.stats.lossPercent === 0;
  const destinationProblem = isDestination && hop.stats.lossPercent > 0;
  const destinationAccentClass = destinationHealthy
    ? 'text-emerald-700 dark:text-emerald-400'
    : destinationProblem
      ? 'font-bold text-rose-700 dark:text-rose-400'
      : '';

  return (
    <tr className="transition-colors hover:bg-orange-50/70 dark:hover:bg-stone-900/80">
      <td className="whitespace-nowrap px-1.5 py-1 pr-2">
        <span className={`inline-block h-1.5 w-1.5 rounded-full ${statusColors[hop.status]}`} />
      </td>
      <td
        className={`whitespace-nowrap px-1 py-1.5 pl-2 text-[12px] font-semibold text-stone-900 dark:text-stone-100 ${destinationAccentClass}`}
      >
        {hop.index}
      </td>
      <td className="px-1 pr-0.5 py-1 whitespace-nowrap">
        <div className={`max-w-[14rem] text-[12px] ${destinationAccentClass}`}>
          <div className="truncate font-medium">
            {hostDisplay}
          </div>
          {ipDisplay && (
            <div
              className={`truncate text-[10px] ${
                destinationHealthy
                  ? 'text-emerald-600/90 dark:text-emerald-400/90'
                  : destinationProblem
                    ? 'font-semibold text-rose-600/90 dark:text-rose-400/90'
                    : 'text-stone-500 dark:text-stone-400'
              }`}
            >
              {ipDisplay}
            </div>
          )}
        </div>
      </td>
      <td className="whitespace-nowrap px-1 py-1.5 pl-0.5 text-right text-[11px] tabular-nums">
        <span
          className={
            destinationHealthy
              ? 'font-semibold text-emerald-700 dark:text-emerald-400'
              : destinationProblem
                ? 'font-bold text-rose-700 dark:text-rose-400'
                : hop.stats.lossPercent > 5
                  ? 'font-semibold text-rose-600 dark:text-rose-400'
                  : ''
          }
        >
          {hop.stats.lossPercent.toFixed(1)}%
        </span>
      </td>
      <td className="whitespace-nowrap px-1 py-1.5 text-right text-[11px] tabular-nums text-stone-600 dark:text-stone-400">
        {hop.stats.sent}
      </td>
      <td className="whitespace-nowrap px-1 py-1.5 text-right text-[11px] tabular-nums text-stone-600 dark:text-stone-400">
        {hop.stats.received}
      </td>
      <td className="whitespace-nowrap px-1 py-1.5 text-right text-[11px] tabular-nums text-stone-600 dark:text-stone-400">
        {formatMs(hop.stats.bestMs)}
      </td>
      <td className="whitespace-nowrap px-1 py-1.5 text-right text-[11px] tabular-nums">
        <span className={hop.stats.avgMs && hop.stats.avgMs > 100 ? 'text-orange-600 dark:text-orange-400' : ''}>
          {formatMs(hop.stats.avgMs)}
        </span>
      </td>
      <td className="whitespace-nowrap px-1 py-1.5 text-right text-[11px] tabular-nums text-stone-600 dark:text-stone-400">
        {formatMs(hop.stats.worstMs)}
      </td>
      <td className="whitespace-nowrap px-1 py-1.5 text-right text-[11px] tabular-nums text-stone-600 dark:text-stone-400">
        {formatMs(hop.stats.lastMs)}
      </td>
      <td className="whitespace-nowrap px-1 py-1.5 pr-2 text-right text-[11px] tabular-nums">
        <span className={hop.stats.jitterMs && hop.stats.jitterMs > 30 ? 'text-orange-600 dark:text-orange-400' : ''}>
          {formatMs(hop.stats.jitterMs)}
        </span>
      </td>
      <td className="px-1.5 py-1.5 pl-3 text-[12px]">
        {hop.interpretation && (
          <div className="space-y-0.5">
            <div className="font-medium text-stone-900 dark:text-stone-100">
              {hop.interpretation.headline}
            </div>
            <div className="line-clamp-2 text-[10px] text-stone-500 dark:text-stone-400">
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
  copy,
}: {
  settings: Settings;
  onChange: (settings: Settings) => void;
  copy: UICopy;
}) {
  return (
    <div className="mx-auto h-full w-full max-w-3xl overflow-auto">
      <div className="rounded-md border border-orange-200/70 bg-white/82 p-3 shadow-sm shadow-orange-200/25 dark:border-transparent dark:bg-stone-950/72">
        <h2 className="mb-2 text-[13px] font-semibold">{copy.settingsTitle}</h2>

        <div className="space-y-2.5">
          <Field label={copy.settings.languageLabel}>
            <div className="relative">
              <select
                value={settings.language}
                onChange={(e) =>
                  onChange({ ...settings, language: e.target.value as Settings['language'] })
                }
                className={selectClassName}
              >
                <option value="en">{copy.settings.languageEnglish}</option>
                <option value="ru">{copy.settings.languageRussian}</option>
              </select>
              <SelectChevron />
            </div>
          </Field>

          <Field label={copy.settings.themeLabel}>
            <div className="relative">
              <select
                value={settings.theme}
                onChange={(e) =>
                  onChange({ ...settings, theme: e.target.value as Settings['theme'] })
                }
                className={selectClassName}
              >
                <option value="system">{copy.settings.themeSystem}</option>
                <option value="light">{copy.settings.themeLight}</option>
                <option value="dark">{copy.settings.themeDark}</option>
              </select>
              <SelectChevron />
            </div>
          </Field>

          <Field label={copy.settings.explanationLevelLabel}>
            <div className="relative">
              <select
                value={settings.explanationLevel}
                onChange={(e) =>
                  onChange({
                    ...settings,
                    explanationLevel: e.target.value as Settings['explanationLevel'],
                  })
                }
                className={selectClassName}
              >
                <option value="simple">{copy.settings.explanationSimple}</option>
                <option value="detailed">{copy.settings.explanationDetailed}</option>
              </select>
              <SelectChevron />
            </div>
          </Field>

          <Field label={copy.settings.probeIntervalLabel} hint={copy.settings.probeIntervalHint}>
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

          <Field label={copy.settings.maximumHopsLabel} hint={copy.settings.maximumHopsHint}>
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

          <Field label={copy.settings.probeTimeoutLabel} hint={copy.settings.probeTimeoutHint}>
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
      <label className="mb-1 block text-[12px] font-medium text-stone-700 dark:text-stone-300">
        {label}
      </label>
      {children}
      {hint && <p className="mt-1 text-[10px] text-stone-500 dark:text-stone-400">{hint}</p>}
    </div>
  );
}

function SelectChevron() {
  return (
    <span className="pointer-events-none absolute inset-y-0 right-3 flex items-center text-stone-500 dark:text-stone-400">
      <svg className="h-3.5 w-3.5" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
        <path
          fillRule="evenodd"
          d="M5.23 7.21a.75.75 0 011.06.02L10 11.17l3.71-3.94a.75.75 0 111.08 1.04l-4.25 4.5a.75.75 0 01-1.08 0l-4.25-4.5a.75.75 0 01.02-1.06z"
          clipRule="evenodd"
        />
      </svg>
    </span>
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

const UI_TEXT = {
  en: {
    appSubtitle: 'WinMTR-style route diagnostics',
    navDiagnose: 'Diagnose',
    navSettings: 'Settings',
    targetLabel: 'Target Host or IP',
    targetPlaceholder: 'e.g., google.com or 8.8.8.8',
    startTrace: 'Start Trace',
    stopTrace: 'Stop',
    resolvedLabel: 'Resolved',
    startedLabel: 'Started',
    traceInProgressTitle: 'Trace in progress',
    traceInProgressHint: 'Final diagnosis appears after the route settles or when you stop the trace.',
    networkRoute: 'Network Route',
    exportHtml: 'Export HTML',
    exportJson: 'Export JSON',
    traceRunningTitle: 'Trace is running',
    traceRunningHint: 'Discovering route and waiting for the first hop responses.',
    emptyStateTitle: 'Ready to Diagnose',
    emptyStateText: 'Enter a hostname or IP address above and click Start Trace to begin network diagnostics.',
    summaryObservations: 'Observations',
    summaryActions: 'Recommended Actions',
    settingsTitle: 'Settings',
    errors: {
      emptyTarget: 'Please enter a target host or IP address',
    },
    table: {
      status: 'Status',
      hop: 'Hop',
      host: 'Host',
      loss: 'Loss%',
      sent: 'Sent',
      received: 'Recv',
      best: 'Best',
      avg: 'Avg',
      worst: 'Worst',
      last: 'Last',
      jitter: 'Jitter',
      interpretation: 'Interpretation',
    },
    settings: {
      languageLabel: 'Language',
      languageEnglish: 'English',
      languageRussian: 'Russian',
      themeLabel: 'Theme',
      themeSystem: 'System',
      themeLight: 'Light',
      themeDark: 'Dark',
      explanationLevelLabel: 'Explanation Level',
      explanationSimple: 'Simple (for beginners)',
      explanationDetailed: 'Detailed (for advanced users)',
      probeIntervalLabel: 'Probe Interval (ms)',
      probeIntervalHint: 'Time between probes (100-10000 ms)',
      maximumHopsLabel: 'Maximum Hops',
      maximumHopsHint: 'Maximum number of hops to trace (1-64)',
      probeTimeoutLabel: 'Probe Timeout (ms)',
      probeTimeoutHint: 'Timeout for each probe (100-10000 ms)',
    },
  },
  ru: {
    appSubtitle: 'Диагностика маршрута в стиле WinMTR',
    navDiagnose: 'Диагностика',
    navSettings: 'Настройки',
    targetLabel: 'Хост или IP цели',
    targetPlaceholder: 'например, google.com или 8.8.8.8',
    startTrace: 'Старт',
    stopTrace: 'Стоп',
    resolvedLabel: 'Разрешён',
    startedLabel: 'Запущено',
    traceInProgressTitle: 'Трассировка выполняется',
    traceInProgressHint: 'Итоговый диагноз появится после стабилизации маршрута или после остановки трассировки.',
    networkRoute: 'Маршрут сети',
    exportHtml: 'Экспорт HTML',
    exportJson: 'Экспорт JSON',
    traceRunningTitle: 'Трассировка выполняется',
    traceRunningHint: 'Определяем маршрут и ждём первые ответы от хопов.',
    emptyStateTitle: 'Готово к диагностике',
    emptyStateText: 'Введите hostname или IP-адрес выше и нажмите «Старт», чтобы начать сетевую диагностику.',
    summaryObservations: 'Наблюдения',
    summaryActions: 'Рекомендуемые действия',
    settingsTitle: 'Настройки',
    errors: {
      emptyTarget: 'Введите хост или IP-адрес цели',
    },
    table: {
      status: 'Статус',
      hop: 'Хоп',
      host: 'Хост',
      loss: 'Потери%',
      sent: 'Отпр',
      received: 'Получ',
      best: 'Луч',
      avg: 'Сред',
      worst: 'Худш',
      last: 'Послед',
      jitter: 'Джитт',
      interpretation: 'Интерпретация',
    },
    settings: {
      languageLabel: 'Язык',
      languageEnglish: 'Английский',
      languageRussian: 'Русский',
      themeLabel: 'Тема',
      themeSystem: 'Системная',
      themeLight: 'Светлая',
      themeDark: 'Тёмная',
      explanationLevelLabel: 'Уровень объяснений',
      explanationSimple: 'Простой (для новичков)',
      explanationDetailed: 'Подробный (для опытных)',
      probeIntervalLabel: 'Интервал проб (мс)',
      probeIntervalHint: 'Время между пробами (100-10000 мс)',
      maximumHopsLabel: 'Максимум хопов',
      maximumHopsHint: 'Максимальное количество хопов (1-64)',
      probeTimeoutLabel: 'Таймаут пробы (мс)',
      probeTimeoutHint: 'Таймаут для каждой пробы (100-10000 мс)',
    },
  },
} as const;

type UICopy = (typeof UI_TEXT)[keyof typeof UI_TEXT];

const inputClassName =
  'w-full rounded-md border border-orange-200 bg-orange-50/70 px-3 py-1.5 text-[12px] shadow-sm focus:border-orange-400 focus:ring-2 focus:ring-orange-400/20 dark:border-stone-700 dark:bg-stone-900 dark:text-white';
const selectClassName = `${inputClassName} appearance-none pr-9`;

export default App;
