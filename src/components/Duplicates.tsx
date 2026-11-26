import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, DupGroup, errorText } from "../api";
import { ago, bytes, count } from "../format";
import ConfirmDialog from "./ConfirmDialog";

interface Props {
  onRecycled: (freed: number) => void;
  onError: (e: string) => void;
}

export default function Duplicates({ onRecycled, onError }: Props) {
  const [groups, setGroups] = useState<DupGroup[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<{ checked: number; total: number } | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirming, setConfirming] = useState(false);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    const un = listen<{ checked: number; total: number }>("dup-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const run = async () => {
    setBusy(true);
    setSelected(new Set());
    try {
      setGroups(await api.findDuplicates());
    } catch (e) {
      onError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  /** Never allow every copy in a group to be selected. */
  const toggle = (g: DupGroup, path: string) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(path)) {
        n.delete(path);
      } else {
        const others = g.files.filter((f) => f.path !== path && !n.has(f.path));
        if (others.length === 0) return s;
        n.add(path);
      }
      return n;
    });

  /** Keep the oldest copy (files are sorted oldest first) and select the rest. */
  const selectExtras = () => {
    const n = new Set<string>();
    for (const g of groups ?? []) {
      const deletable = g.files.filter((f) => !f.protected);
      // Keep a protected copy if there is one, else the oldest.
      const keep = g.files.find((f) => f.protected) ?? g.files[0];
      for (const f of deletable) if (f !== keep) n.add(f.path);
    }
    setSelected(n);
  };

  const sizeOf = new Map<string, number>();
  for (const g of groups ?? []) for (const f of g.files) sizeOf.set(f.path, g.size);
  const selectedBytes = [...selected].reduce((s, p) => s + (sizeOf.get(p) ?? 0), 0);

  const doRecycle = async () => {
    setDeleting(true);
    try {
      const report = await api.recycle([...selected]);
      onRecycled(report.freed);
      const moved = new Set(report.moved);
      setGroups((gs) =>
        (gs ?? [])
          .map((g) => ({ ...g, files: g.files.filter((f) => !moved.has(f.path)) }))
          .filter((g) => g.files.length > 1)
          .map((g) => ({ ...g, wasted: g.size * (g.files.length - 1) })),
      );
      setSelected(new Set());
      if (report.failed.length) onError(`${report.failed.length} file(s) could not be moved.`);
    } catch (e) {
      onError(errorText(e));
    } finally {
      setDeleting(false);
      setConfirming(false);
    }
  };

  if (!groups) {
    return (
      <div className="empty">
        <h2>Duplicate files</h2>
        <p className="muted">
          Compares files of 1 MB and up: same size first, then a quick hash of the start and end, then a full hash.
          Only exact copies are shown.
        </p>
        <button className="btn primary" onClick={run} disabled={busy}>
          {busy ? (progress ? `Checking ${count(progress.checked)} / ${count(progress.total)}…` : "Starting…") : "Find duplicates"}
        </button>
        {busy && (
          <button className="btn ghost" onClick={() => api.cancel()}>
            Stop
          </button>
        )}
      </div>
    );
  }

  const wasted = groups.reduce((s, g) => s + g.wasted, 0);

  return (
    <div className="cleanup">
      <div className="toolbar">
        <span>
          <strong>{groups.length}</strong> groups · <strong>{bytes(wasted)}</strong> in extra copies
        </span>
        <div className="spacer" />
        <button className="btn" onClick={run} disabled={busy}>
          Re-check
        </button>
        <button className="btn" onClick={selectExtras} disabled={groups.length === 0}>
          Select extra copies (keep oldest)
        </button>
      </div>

      <div className="dup-list">
        {groups.map((g) => (
          <div className="panel dup" key={g.hash + g.size}>
            <div className="panel-head">
              <span>
                {g.files.length} copies × {bytes(g.size)}
              </span>
              <span className="num strong">{bytes(g.wasted)} extra</span>
            </div>
            {g.files.map((f, i) => (
              <label className="dup-row" key={f.path}>
                <input
                  type="checkbox"
                  disabled={f.protected}
                  checked={selected.has(f.path)}
                  onChange={() => toggle(g, f.path)}
                />
                <span className="ellipsis" title={f.path}>
                  {f.path}
                </span>
                <span className="muted small">
                  {i === 0 ? "oldest · " : ""}
                  {ago(f.modified)}
                  {f.protected ? " · protected" : ""}
                </span>
                <button
                  className="btn ghost tiny"
                  onClick={(e) => {
                    e.preventDefault();
                    api.reveal(f.path);
                  }}
                >
                  Show
                </button>
              </label>
            ))}
          </div>
        ))}
      </div>

      {selected.size > 0 && (
        <div className="actionbar">
          <span>
            <strong>{selected.size}</strong> files · <strong>{bytes(selectedBytes)}</strong>
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
          title={`Move ${selected.size} duplicate file(s) to the Recycle Bin?`}
          confirmLabel={`Move ${bytes(selectedBytes)}`}
          busy={deleting}
          onConfirm={doRecycle}
          onCancel={() => setConfirming(false)}
        >
          <p>At least one copy of every file is kept. You can restore anything from the Recycle Bin.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
