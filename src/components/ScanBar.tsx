import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, DriveInfo, ProgressEvent } from "../api";
import { bytes, count, duration } from "../format";

interface Props {
  scanning: boolean;
  progress: ProgressEvent | null;
  onScan: (root: string) => void;
  onCancel: () => void;
}

export default function ScanBar({ scanning, progress, onScan, onCancel }: Props) {
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [root, setRoot] = useState("");

  useEffect(() => {
    api.listDrives().then((d) => {
      setDrives(d);
      if (d.length) setRoot((r) => r || d[0].mount);
    });
  }, []);

  const pickFolder = async () => {
    const dir = await open({ directory: true, multiple: false, title: "Choose a folder to scan" });
    if (typeof dir === "string") setRoot(dir);
  };

  return (
    <header className="scanbar">
      <div className="drives">
        {drives.map((d) => {
          const used = d.total - d.free;
          const pct = d.total ? (used / d.total) * 100 : 0;
          return (
            <button
              key={d.mount}
              className={`drive ${root === d.mount ? "selected" : ""}`}
              onClick={() => setRoot(d.mount)}
              disabled={scanning}
              title={d.name}
            >
              <div className="drive-top">
                <strong>{d.mount}</strong>
                <span className="muted small">{bytes(d.free)} free</span>
              </div>
              <div className="meter">
                <div className={`meter-fill ${pct > 90 ? "hot" : ""}`} style={{ width: `${pct}%` }} />
              </div>
            </button>
          );
        })}
        <button className="drive folder" onClick={pickFolder} disabled={scanning}>
          <div className="drive-top">
            <strong>Folder…</strong>
          </div>
          <div className="muted small ellipsis">
            {root && !drives.some((d) => d.mount === root) ? root : "Scan one folder"}
          </div>
        </button>
      </div>

      <div className="scan-actions">
        {scanning ? (
          <>
            <div className="live">
              <span className="spinner" aria-hidden />
              <div>
                <div>
                  {count(progress?.files ?? 0)} files · {bytes(progress?.bytes ?? 0)}
                </div>
                <div className="muted small">
                  {count(progress?.dirs ?? 0)} folders · {duration(progress?.elapsed_ms ?? 0)}
                  {progress && progress.errors > 0 && ` · ${count(progress.errors)} not readable`}
                </div>
              </div>
            </div>
            <button className="btn ghost" onClick={onCancel}>
              Cancel
            </button>
          </>
        ) : (
          <button className="btn primary big" disabled={!root} onClick={() => onScan(root)}>
            Scan {root.length <= 4 ? root : "folder"}
          </button>
        )}
      </div>
    </header>
  );
}
