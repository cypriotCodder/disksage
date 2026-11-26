import { Candidate, canRecycle, effectiveVerdict, ScanSummary } from "../api";
import { baseName, bytes, count, duration } from "../format";

interface Props {
  summary: ScanSummary | null;
  candidates: Candidate[];
  scanning: boolean;
  goCleanup: () => void;
}

export default function Overview({ summary, candidates, scanning, goCleanup }: Props) {
  if (!summary) {
    return (
      <div className="empty">
        <h1>Find what's filling your disk</h1>
        <p className="muted">
          Pick a drive above and scan. DiskSage finds caches, build output, leftovers and large unknown folders, then
          Claude explains the ones the rules can't recognise. Nothing is deleted without you checking it first, and
          everything goes to the Recycle Bin.
        </p>
        {scanning && <p>Scanning…</p>}
      </div>
    );
  }

  const byVerdict = { safe: 0, caution: 0, keep: 0, unknown: 0 };
  let unknownCount = 0;
  for (const c of candidates) {
    const v = effectiveVerdict(c);
    if (v === null) {
      byVerdict.unknown += c.size;
      unknownCount++;
    } else if (v === "safe" && canRecycle(c)) byVerdict.safe += c.size;
    else if (v === "caution") byVerdict.caution += c.size;
    else byVerdict.keep += c.size;
  }
  const maxExt = summary.ext_stats[0]?.bytes ?? 1;
  const top = [...candidates].filter(canRecycle).sort((a, b) => b.size - a.size).slice(0, 6);

  return (
    <div className="overview">
      <div className="cards">
        <div className="card">
          <div className="muted small">Scanned</div>
          <div className="big-num">{bytes(summary.total_bytes)}</div>
          <div className="muted small">
            {count(summary.file_count)} files · {count(summary.dir_count)} folders · {duration(summary.elapsed_ms)}
          </div>
        </div>
        <div className="card accent">
          <div className="muted small">Safe to remove</div>
          <div className="big-num">{bytes(byVerdict.safe)}</div>
          <div className="muted small">Caches, temp files and build output</div>
        </div>
        <div className="card">
          <div className="muted small">Worth a look</div>
          <div className="big-num">{bytes(byVerdict.caution)}</div>
          <div className="muted small">Marked caution</div>
        </div>
        <div className="card">
          <div className="muted small">Not identified yet</div>
          <div className="big-num">{bytes(byVerdict.unknown)}</div>
          <div className="muted small">{unknownCount} {unknownCount === 1 ? "item" : "items"} for Claude</div>
        </div>
      </div>

      {summary.errors > 0 && (
        <p className="muted small">
          {count(summary.errors)} folders could not be read (usually system folders). Run DiskSage as administrator to
          include them.
        </p>
      )}

      <div className="two-col">
        <div className="panel">
          <div className="panel-head">
            <h2>Biggest wins</h2>
            <button className="btn" onClick={goCleanup}>
              Open Cleanup
            </button>
          </div>
          {top.length === 0 && <p className="muted">Nothing large found.</p>}
          {top.map((c) => (
            <div className="row-line" key={c.id}>
              <div className="ellipsis">
                <strong>{c.ai?.what || baseName(c.path)}</strong>
                <div className="muted small ellipsis">{c.path}</div>
              </div>
              <span className="num">{bytes(c.size)}</span>
            </div>
          ))}
        </div>

        <div className="panel">
          <h2>Space by file type</h2>
          {summary.ext_stats.map((e) => (
            <div className="bar-row" key={e.ext}>
              <span className="bar-label">.{e.ext}</span>
              <div className="bar">
                <div className="bar-fill" style={{ width: `${(e.bytes / maxExt) * 100}%` }} />
              </div>
              <span className="num">{bytes(e.bytes)}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
