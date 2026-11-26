import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, Candidate, canRecycle, effectiveVerdict, errorText, Verdict } from "../api";
import { ago, baseName, bytes, count } from "../format";
import ConfirmDialog from "./ConfirmDialog";

type Filter = "all" | Verdict | "unknown";

interface Props {
  candidates: Candidate[];
  setCandidates: (c: Candidate[]) => void;
  onRecycled: (freed: number, remaining: Candidate[]) => void;
  onError: (e: string) => void;
}

const VERDICT_LABEL: Record<Verdict, string> = { safe: "Safe", caution: "Caution", keep: "Keep" };

export default function Cleanup({ candidates, setCandidates, onRecycled, onError }: Props) {
  const [filter, setFilter] = useState<Filter>("all");
  const [category, setCategory] = useState("all");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [aiBusy, setAiBusy] = useState(false);
  const [aiProgress, setAiProgress] = useState<{ done: number; total: number } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    const un = listen<{ done: number; total: number }>("ai-progress", (e) => setAiProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Drop selections that no longer exist or can no longer be recycled.
  useEffect(() => {
    setSelected((s) => {
      const ok = new Set(candidates.filter(canRecycle).map((c) => c.id));
      return new Set([...s].filter((id) => ok.has(id)));
    });
  }, [candidates]);

  const categories = useMemo(() => [...new Set(candidates.map((c) => c.category))].sort(), [candidates]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return candidates.filter((c) => {
      const v = effectiveVerdict(c);
      if (filter === "unknown" ? v !== null : filter !== "all" && v !== filter) return false;
      if (category !== "all" && c.category !== category) return false;
      if (q && !c.path.toLowerCase().includes(q) && !(c.ai?.what ?? "").toLowerCase().includes(q)) return false;
      return true;
    });
  }, [candidates, filter, category, query]);

  const counts = useMemo(() => {
    const m: Record<Filter, number> = { all: candidates.length, safe: 0, caution: 0, keep: 0, unknown: 0 };
    for (const c of candidates) m[effectiveVerdict(c) ?? "unknown"]++;
    return m;
  }, [candidates]);

  const unknownIds = candidates.filter((c) => !c.protected && c.verdict === null && !c.ai).map((c) => c.id);
  const selectedItems = candidates.filter((c) => selected.has(c.id));
  const selectedBytes = selectedItems.reduce((s, c) => s + c.size, 0);

  const toggle = (id: number) =>
    setSelected((s) => {
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });

  const selectAllSafe = () =>
    setSelected(new Set(visible.filter((c) => canRecycle(c) && effectiveVerdict(c) === "safe").map((c) => c.id)));

  const askAi = async (ids: number[]) => {
    setAiBusy(true);
    setAiProgress(null);
    try {
      setCandidates(await api.analyzeWithAi(ids));
    } catch (e) {
      onError(errorText(e));
    } finally {
      setAiBusy(false);
      setAiProgress(null);
    }
  };

  const doRecycle = async () => {
    setDeleting(true);
    try {
      const report = await api.recycle(selectedItems.map((c) => c.path));
      const remaining = await api.getCandidates();
      onRecycled(report.freed, remaining);
      setSelected(new Set());
      if (report.failed.length) {
        onError(
          `${report.failed.length} item(s) were not moved: ` +
            report.failed
              .slice(0, 3)
              .map((f) => `${baseName(f.path)} (${f.error})`)
              .join(", "),
        );
      }
    } catch (e) {
      onError(errorText(e));
    } finally {
      setDeleting(false);
      setConfirming(false);
    }
  };

  const cautionSelected = selectedItems.filter((c) => effectiveVerdict(c) !== "safe").length;

  return (
    <div className="cleanup">
      <div className="toolbar">
        <div className="chips">
          {(["all", "safe", "caution", "keep", "unknown"] as Filter[]).map((f) => (
            <button key={f} className={`chip ${f} ${filter === f ? "on" : ""}`} onClick={() => setFilter(f)}>
              {f === "all" ? "All" : f === "unknown" ? "Unknown" : VERDICT_LABEL[f]} <span>{counts[f]}</span>
            </button>
          ))}
        </div>
        <select value={category} onChange={(e) => setCategory(e.target.value)}>
          <option value="all">All categories</option>
          {categories.map((c) => (
            <option key={c}>{c}</option>
          ))}
        </select>
        <input placeholder="Search path…" value={query} onChange={(e) => setQuery(e.target.value)} />
        <div className="spacer" />
        <button className="btn" onClick={selectAllSafe}>
          Select all safe
        </button>
        <button className="btn ai" disabled={aiBusy || unknownIds.length === 0} onClick={() => askAi([])}>
          {aiBusy
            ? aiProgress
              ? `Asking Claude… ${aiProgress.done}/${aiProgress.total}`
              : "Asking Claude…"
            : `Ask Claude about ${unknownIds.length} unknown`}
        </button>
      </div>

      <div className="table">
        <div className="thead">
          <span />
          <span>Item</span>
          <span>Verdict</span>
          <span>What it is</span>
          <span className="num">Size</span>
          <span />
        </div>
        {visible.length === 0 && <div className="muted pad">Nothing matches these filters.</div>}
        {visible.map((c) => {
          const v = effectiveVerdict(c);
          const selectable = canRecycle(c);
          const fromAi = c.verdict === null && !!c.ai && !c.protected;
          return (
            <div key={c.id} className={`trow ${selected.has(c.id) ? "sel" : ""}`}>
              <input
                type="checkbox"
                checked={selected.has(c.id)}
                disabled={!selectable}
                title={selectable ? "" : c.protected ? "Protected location" : c.manual ? "Clean this up the way shown" : "Marked keep"}
                onChange={() => toggle(c.id)}
              />
              <div className="item ellipsis">
                <div className="ellipsis">
                  <strong>{baseName(c.path)}</strong>
                  <span className="muted small">
                    {" "}
                    {c.is_dir ? `${count(c.file_count)} files` : "file"} · {ago(c.modified)}
                  </span>
                </div>
                <div className="muted small ellipsis" title={c.path}>
                  {c.path}
                </div>
              </div>
              <div>
                {v ? (
                  <span className={`badge ${v}`}>
                    {VERDICT_LABEL[v]}
                    {fromAi && <em> · AI</em>}
                  </span>
                ) : (
                  <span className="badge unknown">Unknown</span>
                )}
              </div>
              <div className="why">
                <div>
                  {c.ai?.what && <strong>{c.ai.what}. </strong>}
                  {c.reason || c.ai?.reason || (!c.protected && <span className="muted">{c.category}</span>)}
                </div>
                {c.protected && <div className="muted small">Protected location: DiskSage never deletes it.</div>}
                {c.ai && c.reason && c.ai.reason && <div className="muted small">Claude: {c.ai.reason}</div>}
                {(c.manual || c.ai?.tip) && <div className="tip small">→ {c.manual || c.ai?.tip}</div>}
              </div>
              <span className="num strong">{bytes(c.size)}</span>
              <div className="row-actions">
                <button className="btn ghost tiny" onClick={() => api.reveal(c.path)}>
                  Show
                </button>
                {!c.protected && (
                  <button className="btn ghost tiny" disabled={aiBusy} onClick={() => askAi([c.id])}>
                    Ask AI
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>

      {selected.size > 0 && (
        <div className="actionbar">
          <span>
            <strong>{selected.size}</strong> selected · <strong>{bytes(selectedBytes)}</strong>
          </span>
          <button className="btn ghost" onClick={() => setSelected(new Set())}>
            Clear
          </button>
          <button className="btn danger" onClick={() => setConfirming(true)}>
            Move to Recycle Bin
          </button>
        </div>
      )}

      {confirming && (
        <ConfirmDialog
          title={`Move ${selected.size} item(s) to the Recycle Bin?`}
          confirmLabel={`Move ${bytes(selectedBytes)}`}
          busy={deleting}
          onConfirm={doRecycle}
          onCancel={() => setConfirming(false)}
        >
          {cautionSelected > 0 && (
            <p className="warn">
              {cautionSelected} of these are not marked safe. Make sure you don't need them.
            </p>
          )}
          <ul className="confirm-list">
            {selectedItems.slice(0, 12).map((c) => (
              <li key={c.id}>
                <span className="ellipsis">{c.path}</span>
                <span className="num">{bytes(c.size)}</span>
              </li>
            ))}
            {selectedItems.length > 12 && <li className="muted">…and {selectedItems.length - 12} more</li>}
          </ul>
          <p className="muted small">
            You can restore anything from the Recycle Bin. Space is freed when you empty it.
          </p>
        </ConfirmDialog>
      )}
    </div>
  );
}
