import { useEffect, useState } from "react";
import { api, BrowseResult, errorText } from "../api";
import { ago, bytes, count } from "../format";

export default function Explore({ onError }: { onError: (e: string) => void }) {
  const [data, setData] = useState<BrowseResult | null>(null);

  const go = (path: string | null) =>
    api
      .browse(path)
      .then(setData)
      .catch((e) => onError(errorText(e)));

  useEffect(() => {
    go(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!data) return null;
  const crumbs = data.path.split(/[\\/]/).filter(Boolean);

  return (
    <div className="panel">
      <div className="panel-head">
        <div className="crumbs">
          {data.parent && (
            <button className="btn ghost" onClick={() => go(data.parent)}>
              ← Up
            </button>
          )}
          <span className="ellipsis" title={data.path}>
            {crumbs.join("  ›  ")}
          </span>
        </div>
        <span className="num">{bytes(data.size)}</span>
      </div>
      <div className="explore-list">
        {data.entries.map((e, i) => {
          const pct = data.size ? (e.size / data.size) * 100 : 0;
          return (
            <div
              key={e.path || `files-${i}`}
              className={`explore-row ${e.is_dir ? "clickable" : ""}`}
              onClick={() => e.is_dir && go(e.path)}
            >
              <span className="icon" aria-hidden>
                {e.is_dir ? "▸" : "·"}
              </span>
              <div className="ellipsis">
                <span>{e.name}</span>
                <span className="muted small">
                  {e.is_dir && ` ${count(e.file_count)} files`}
                  {e.modified > 0 && ` · ${ago(e.modified)}`}
                </span>
              </div>
              <div className="bar">
                <div className="bar-fill" style={{ width: `${pct}%` }} />
              </div>
              <span className="num">{bytes(e.size)}</span>
              <span className="muted small pct">{pct.toFixed(1)}%</span>
              {e.path ? (
                <button
                  className="btn ghost tiny"
                  onClick={(ev) => {
                    ev.stopPropagation();
                    api.reveal(e.path);
                  }}
                >
                  Show
                </button>
              ) : (
                <span />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
