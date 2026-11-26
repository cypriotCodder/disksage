import { useEffect, useState } from "react";
import { api, errorText, SettingsView } from "../api";

export default function Settings({ onError }: { onError: (e: string) => void }) {
  const [view, setView] = useState<SettingsView | null>(null);
  const [key, setKey] = useState("");
  const [model, setModel] = useState("");
  const [redact, setRedact] = useState(true);
  const [status, setStatus] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    api.getSettings().then((v) => {
      setView(v);
      setModel(v.model);
      setRedact(v.redact_username);
    });
  }, []);

  const save = async () => {
    try {
      const v = await api.saveSettings(key.trim() ? key.trim() : null, model, redact);
      setView(v);
      setKey("");
      setStatus("Saved.");
    } catch (e) {
      onError(errorText(e));
    }
  };

  const test = async () => {
    setTesting(true);
    setStatus(null);
    try {
      setStatus(await api.testAi());
    } catch (e) {
      setStatus(errorText(e));
    } finally {
      setTesting(false);
    }
  };

  if (!view) return null;

  return (
    <div className="settings panel">
      <h2>Claude</h2>
      <label className="field">
        <span>API key</span>
        <input
          type="password"
          placeholder={view.has_key ? `Saved (${view.key_hint}). Paste a new one to replace it.` : "sk-ant-…"}
          value={key}
          onChange={(e) => setKey(e.target.value)}
          autoComplete="off"
          spellCheck={false}
        />
        <small className="muted">
          Create one at console.anthropic.com. Stored on this PC in {view.settings_path}. The ANTHROPIC_API_KEY
          environment variable overrides it.
        </small>
      </label>

      <label className="field">
        <span>Model</span>
        <input value={model} onChange={(e) => setModel(e.target.value)} spellCheck={false} />
        <small className="muted">claude-haiku-5-5 is fast and cheap; claude-sonnet-5-5 gives sharper answers.</small>
      </label>

      <label className="check">
        <input type="checkbox" checked={redact} onChange={(e) => setRedact(e.target.checked)} />
        Hide my Windows user name in paths sent to Claude
      </label>

      <div className="settings-actions">
        <button className="btn primary" onClick={save}>
          Save
        </button>
        <button className="btn" onClick={test} disabled={testing || !view.has_key}>
          {testing ? "Testing…" : "Test connection"}
        </button>
        {status && <span className="muted">{status}</span>}
      </div>

      <h2>What gets sent</h2>
      <p className="muted">
        Only metadata for the items you ask about: path, size, file count and last-modified date. File contents never
        leave your PC. Deleting always goes to the Recycle Bin, and protected locations (Windows, Program Files, your
        Documents, Desktop, Pictures, Videos, Music, OneDrive, and any .git folder) can't be deleted from DiskSage at
        all.
      </p>
    </div>
  );
}
