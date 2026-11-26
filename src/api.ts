import { invoke } from "@tauri-apps/api/core";

export type Verdict = "safe" | "caution" | "keep";
export type Source = "rule" | "large-file" | "large-folder";

export interface DriveInfo {
  mount: string;
  name: string;
  total: number;
  free: number;
}

export interface ProgressEvent {
  files: number;
  dirs: number;
  bytes: number;
  errors: number;
  elapsed_ms: number;
}

export interface ExtStat {
  ext: string;
  bytes: number;
  files: number;
}

export interface ScanSummary {
  root: string;
  total_bytes: number;
  file_count: number;
  dir_count: number;
  errors: number;
  elapsed_ms: number;
  cancelled: boolean;
  ext_stats: ExtStat[];
  candidate_count: number;
  reclaimable_safe: number;
  duplicate_sizes: number;
}

export interface AiNote {
  what: string;
  verdict: Verdict;
  reason: string;
  tip: string | null;
}

export interface Candidate {
  id: number;
  path: string;
  is_dir: boolean;
  size: number;
  file_count: number;
  modified: number;
  source: Source;
  category: string;
  verdict: Verdict | null;
  reason: string;
  manual: string | null;
  protected: boolean;
  ai: AiNote | null;
}

export interface BrowseEntry {
  name: string;
  path: string;
  size: number;
  file_count: number;
  is_dir: boolean;
  modified: number;
}

export interface BrowseResult {
  path: string;
  parent: string | null;
  size: number;
  entries: BrowseEntry[];
}

export interface DupFile {
  path: string;
  modified: number;
  protected: boolean;
}

export interface DupGroup {
  size: number;
  hash: string;
  wasted: number;
  files: DupFile[];
}

export interface RecycleReport {
  moved: string[];
  failed: { path: string; error: string }[];
  freed: number;
}

export interface SettingsView {
  has_key: boolean;
  key_hint: string;
  model: string;
  redact_username: boolean;
  settings_path: string;
}

/** Same rules as `Candidate::effective_verdict` in Rust. */
export function effectiveVerdict(c: Candidate): Verdict | null {
  if (c.protected) return "keep";
  return c.verdict ?? c.ai?.verdict ?? null;
}

/** Same rules as `Candidate::can_recycle` in Rust (the backend re-checks anyway). */
export function canRecycle(c: Candidate): boolean {
  return !c.protected && !c.manual && effectiveVerdict(c) !== "keep";
}

export const api = {
  listDrives: () => invoke<DriveInfo[]>("list_drives"),
  startScan: (root: string) => invoke<ScanSummary>("start_scan", { root }),
  cancel: () => invoke<void>("cancel"),
  browse: (path: string | null) => invoke<BrowseResult>("browse", { path }),
  getCandidates: () => invoke<Candidate[]>("get_candidates"),
  analyzeWithAi: (ids: number[]) => invoke<Candidate[]>("analyze_with_ai", { ids }),
  findDuplicates: () => invoke<DupGroup[]>("find_duplicates"),
  recycle: (paths: string[]) => invoke<RecycleReport>("recycle", { paths }),
  reveal: (path: string) => invoke<void>("reveal", { path }),
  getSettings: () => invoke<SettingsView>("get_settings"),
  saveSettings: (apiKey: string | null, model: string, redactUsername: boolean) =>
    invoke<SettingsView>("save_settings", { apiKey, model, redactUsername }),
  testAi: () => invoke<string>("test_ai"),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
