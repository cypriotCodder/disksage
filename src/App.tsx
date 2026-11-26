import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, Candidate, errorText, ProgressEvent, ScanSummary } from "./api";
import ScanBar from "./components/ScanBar";
import Overview from "./components/Overview";
import Explore from "./components/Explore";
import Cleanup from "./components/Cleanup";
import Duplicates from "./components/Duplicates";
import Settings from "./components/Settings";
import { bytes } from "./format";

type Tab = "overview" | "explore" | "cleanup" | "duplicates" | "settings";

const TABS: { id: Tab; label: string; needsScan: boolean }[] = [
  { id: "overview", label: "Overview", needsScan: false },
  { id: "cleanup", label: "Cleanup", needsScan: true },
  { id: "explore", label: "Explore", needsScan: true },
  { id: "duplicates", label: "Duplicates", needsScan: true },
  { id: "settings", label: "Settings", needsScan: false },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("overview");
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [freedTotal, setFreedTotal] = useState(0);
  const [scanId, setScanId] = useState(0);

  useEffect(() => {
    const un = listen<ProgressEvent>("scan-progress", (e) => setProgress(e.payload));
    const un2 = listen<string>("ai-warning", (e) => setNotice(`Claude stopped early: ${e.payload}`));
    return () => {
      un.then((f) => f());
      un2.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 6000);
    return () => clearTimeout(t);
  }, [notice]);

  const runScan = useCallback(async (root: string) => {
    setError(null);
    setScanning(true);
    setProgress(null);
    try {
      const s = await api.startScan(root);
      setSummary(s);
      setCandidates(await api.getCandidates());
      setScanId((n) => n + 1);
      if (s.cancelled) setNotice("Scan cancelled. Results are partial.");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setScanning(false);
    }
  }, []);

  const onRecycled = useCallback((freed: number, remaining?: Candidate[]) => {
    setFreedTotal((f) => f + freed);
    if (remaining) setCandidates(remaining);
    setNotice(`Moved ${bytes(freed)} to the Recycle Bin. Empty it to actually free the space.`);
  }, []);

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark" aria-hidden />
          <span>DiskSage</span>
        </div>
        <nav>
          {TABS.map((t) => (
            <button
              key={t.id}
              className={`nav-item ${tab === t.id ? "active" : ""}`}
              disabled={t.needsScan && !summary}
              onClick={() => setTab(t.id)}
            >
              {t.label}
              {t.id === "cleanup" && candidates.length > 0 && <span className="pill">{candidates.length}</span>}
            </button>
          ))}
        </nav>
        {freedTotal > 0 && (
          <div className="freed">
            <div className="muted small">Moved to Recycle Bin</div>
            <div className="freed-num">{bytes(freedTotal)}</div>
          </div>
        )}
      </aside>

      <main className="main">
        <ScanBar scanning={scanning} progress={progress} onScan={runScan} onCancel={() => api.cancel()} />
        {error && (
          <div className="banner error" onClick={() => setError(null)}>
            {error}
          </div>
        )}
        {notice && <div className="banner info">{notice}</div>}

        <section className="content">
          {tab === "overview" && (
            <Overview summary={summary} candidates={candidates} scanning={scanning} goCleanup={() => setTab("cleanup")} />
          )}
          {tab === "cleanup" && summary && (
            <Cleanup candidates={candidates} setCandidates={setCandidates} onRecycled={onRecycled} onError={setError} />
          )}
          {tab === "explore" && summary && <Explore key={scanId} onError={setError} />}
          {tab === "duplicates" && summary && (
            <Duplicates key={scanId} onRecycled={(f) => onRecycled(f)} onError={setError} />
          )}
          {tab === "settings" && <Settings onError={setError} />}
        </section>
      </main>
    </div>
  );
}
