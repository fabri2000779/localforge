/**
 * Metrics tab — local CPU/RAM/network history for a server.
 *
 * Reads the host's local time-series (`query_metrics`); the cloud is never
 * involved. Hand-rolled SVG charts (no chart dependency). Network is shown as a
 * rate derived from the delta between cumulative samples.
 */
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Activity, Loader2, RotateCcw, Cpu, MemoryStick, Wifi } from 'lucide-react';

interface MetricPoint {
  ts: number;
  cpuPercent: number;
  memoryMb: number;
  netRxBytes: number;
  netTxBytes: number;
}

const RANGES: Array<{ label: string; ms: number }> = [
  { label: '1h', ms: 3600_000 },
  { label: '6h', ms: 6 * 3600_000 },
  { label: '24h', ms: 24 * 3600_000 },
];

function fmtBytesRate(bps: number): string {
  if (bps < 1024) return `${Math.round(bps)} B/s`;
  if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
  return `${(bps / 1024 / 1024).toFixed(2)} MB/s`;
}

/** A compact sparkline-style area+line chart. `series` may hold 1-2 lines. */
function Chart({
  points, series, height = 90,
}: {
  points: number[]; // x positions (timestamps, ms)
  series: Array<{ values: number[]; color: string }>;
  height?: number;
}) {
  const W = 600;
  const H = height;
  const pad = 4;
  if (points.length < 2) {
    return <div className="text-xs text-zinc-500 py-6 text-center">Not enough data yet — samples every minute.</div>;
  }
  const xMin = points[0]!;
  const xMax = points[points.length - 1]!;
  const xSpan = Math.max(1, xMax - xMin);
  let yMax = 0;
  for (const s of series) for (const v of s.values) if (v > yMax) yMax = v;
  yMax = yMax <= 0 ? 1 : yMax * 1.1;
  const x = (t: number) => pad + ((t - xMin) / xSpan) * (W - pad * 2);
  const y = (v: number) => H - pad - (v / yMax) * (H - pad * 2);

  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" style={{ width: '100%', height }}>
      {series.map((s, si) => {
        const line = s.values.map((v, i) => `${i === 0 ? 'M' : 'L'} ${x(points[i]!).toFixed(1)} ${y(v).toFixed(1)}`).join(' ');
        const area = `${line} L ${x(xMax).toFixed(1)} ${H - pad} L ${x(xMin).toFixed(1)} ${H - pad} Z`;
        return (
          <g key={si}>
            <path d={area} fill={s.color} opacity={0.12} />
            <path d={line} fill="none" stroke={s.color} strokeWidth={1.5} />
          </g>
        );
      })}
    </svg>
  );
}

export function MetricsPanel({ serverId, nodeId }: { serverId: string; nodeId: string }) {
  const [range, setRange] = useState(RANGES[2]!); // 24h
  const [points, setPoints] = useState<MetricPoint[]>([]);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true); setErr(null);
    try {
      const since = Date.now() - range.ms;
      setPoints(await invoke<MetricPoint[]>('query_metrics', { serverId, sinceMs: since, nodeId }));
    } catch (e) { setErr(String(e)); }
    finally { setLoading(false); }
  }, [serverId, nodeId, range]);

  useEffect(() => { void refresh(); }, [refresh]);

  const xs = points.map((p) => p.ts);
  const cpu = points.map((p) => p.cpuPercent);
  const mem = points.map((p) => p.memoryMb);
  // Network rate from cumulative-counter deltas.
  const rxRate: number[] = [];
  const txRate: number[] = [];
  const netXs: number[] = [];
  for (let i = 1; i < points.length; i++) {
    const dt = (points[i]!.ts - points[i - 1]!.ts) / 1000;
    if (dt <= 0) continue;
    rxRate.push(Math.max(0, (points[i]!.netRxBytes - points[i - 1]!.netRxBytes) / dt));
    txRate.push(Math.max(0, (points[i]!.netTxBytes - points[i - 1]!.netTxBytes) / dt));
    netXs.push(points[i]!.ts);
  }

  const last = points[points.length - 1];

  return (
    <div className="space-y-4">
      <div className="card">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2 font-medium text-zinc-200">
            <Activity size={16} className="text-emerald-400" /> Metrics history
          </div>
          <div className="flex items-center gap-1">
            {RANGES.map((r) => (
              <button key={r.label} className={`px-2.5 py-1 rounded-md text-xs ${range.label === r.label ? 'bg-zinc-700 text-white' : 'bg-zinc-800/60 text-zinc-400 hover:text-white'}`}
                onClick={() => setRange(r)}>{r.label}</button>
            ))}
            <button className="icon-btn text-zinc-400 hover:text-white ml-1" title="Refresh" onClick={refresh} disabled={loading}>
              <RotateCcw size={14} className={loading ? 'animate-spin' : ''} />
            </button>
          </div>
        </div>
        <p className="text-xs text-zinc-500 mt-1">Sampled every minute on the host (kept ~14 days). Not sent to the cloud.</p>
      </div>

      {err && <div className="text-sm text-red-300 px-1 break-all">{err}</div>}

      {loading && points.length === 0 ? (
        <div className="card flex items-center gap-2 text-zinc-400 py-8 justify-center"><Loader2 size={16} className="animate-spin" /> Loading…</div>
      ) : (
        <>
          <div className="card">
            <div className="flex items-center justify-between mb-2">
              <span className="flex items-center gap-2 text-sm text-zinc-300"><Cpu size={14} className="text-sky-400" /> CPU</span>
              <span className="font-mono text-sm text-zinc-200">{last ? `${last.cpuPercent.toFixed(1)}%` : '—'}</span>
            </div>
            <Chart points={xs} series={[{ values: cpu, color: '#38bdf8' }]} />
          </div>
          <div className="card">
            <div className="flex items-center justify-between mb-2">
              <span className="flex items-center gap-2 text-sm text-zinc-300"><MemoryStick size={14} className="text-sky-400" /> Memory</span>
              <span className="font-mono text-sm text-zinc-200">{last ? `${last.memoryMb.toFixed(0)} MB` : '—'}</span>
            </div>
            <Chart points={xs} series={[{ values: mem, color: '#7dd3fc' }]} />
          </div>
          <div className="card">
            <div className="flex items-center justify-between mb-2">
              <span className="flex items-center gap-2 text-sm text-zinc-300"><Wifi size={14} className="text-emerald-400" /> Network</span>
              <span className="font-mono text-xs text-zinc-400">
                {rxRate.length ? `↓ ${fmtBytesRate(rxRate[rxRate.length - 1]!)}  ↑ ${fmtBytesRate(txRate[txRate.length - 1]!)}` : '—'}
              </span>
            </div>
            <Chart points={netXs} series={[{ values: rxRate, color: '#34d399' }, { values: txRate, color: '#f59e0b' }]} />
            <div className="flex gap-4 mt-1 text-[11px] text-zinc-500">
              <span className="flex items-center gap-1"><span className="inline-block w-2 h-2 rounded-sm" style={{ background: '#34d399' }} /> Download</span>
              <span className="flex items-center gap-1"><span className="inline-block w-2 h-2 rounded-sm" style={{ background: '#f59e0b' }} /> Upload</span>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
