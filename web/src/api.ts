import { Channel, invoke } from "@tauri-apps/api/core";

export type Status = "draft" | "ready" | "sent" | "archived";
export const STATUSES: Status[] = ["draft", "ready", "sent", "archived"];

export interface ResumeSummary {
  slug: string;
  title: string;
  status: Status;
  tags: string[];
  updated_ms: number;
  words: number;
  has_posting: boolean;
}
/** A Markdown file split into front matter fields and body. */
export interface Parts {
  fields: Record<string, string | string[]>;
  body: string;
  body_offset: number;
  hash: string;
}
export interface TextFile {
  content: string;
  hash: string;
}
export interface InstructionFile {
  items: string[];
  hash: string;
}
export interface Skill {
  name: string;
  description: string;
  content: string;
}
export interface Finding {
  level: "error" | "warn" | "info";
  message: string;
  line?: number;
}
export interface Keywords {
  coverage: number;
  found: string[];
  missing: string[];
}
export interface Report {
  words: number;
  bullets: number;
  bullets_with_numbers: number;
  est_pages: number;
  findings: Finding[];
  keywords?: Keywords;
}
export interface Info {
  root: string;
  exe: string;
  mcp_config: unknown;
  version: string;
  pdf_export: boolean;
  /** The data folder's format, and the upgrade opening it just did, if any. */
  format: number;
  upgraded: { from: number; to: number; backup: string } | null;
  /** Whether this copy can update itself (a release AppImage, Windows install or macOS app). */
  updates: boolean;
  license: string;
}
export interface Licenses {
  id: string;
  license: string;
  notice: string;
  third_party: ThirdParty;
}
/** THIRD_PARTY_LICENSES.md, taken apart. Prose is Markdown; license texts are verbatim. */
export interface ThirdParty {
  prose: { title: string; paragraphs: string[] }[];
  summary: [string, number][];
  groups: { title: string; note: string; packages: ThirdPartyPackage[] }[];
  /** Package `texts` index into this. */
  texts: { kind: string; users: number; text: string }[];
}
export interface ThirdPartyPackage {
  name: string;
  note: string;
  /** Empty when used on every platform. */
  platforms: string;
  version: string;
  declared: string;
  used: string;
  source: string;
  texts: number[];
}
export interface UpdateInfo {
  version: string;
  notes: string | null;
  date: string | null;
}
export interface Progress {
  done: number;
  total: number | null;
}
/** "profile" or "resume:<slug>" */
export type FileId = "profile" | `resume:${string}`;
export type FieldValue = string | string[] | null;

export const api = {
  info: () => invoke<Info>("info"),
  revision: () => invoke<string>("revision"),
  listResumes: () => invoke<ResumeSummary[]>("list_resumes"),
  /** Resolves to the new slug. */
  createResume: (title: string, from?: string) => invoke<string>("create_resume", { title, from }),
  readParts: (file: FileId) => invoke<Parts>("read_parts", { file }),
  writeBody: (file: FileId, body: string, expectedHash?: string) =>
    invoke<Parts>("write_body", { file, body, expectedHash }),
  setFields: (file: FileId, fields: Record<string, FieldValue>, expectedHash?: string) =>
    invoke<Parts>("set_fields", { file, fields, expectedHash }),
  setMeta: (slug: string, meta: { title?: string; status?: Status; tags?: string[] }, expectedHash?: string) =>
    invoke<Parts>("set_meta", { slug, ...meta, expectedHash }),
  archiveResume: (slug: string) => invoke<ResumeSummary>("archive_resume", { slug }),
  /** Back from the archive as a draft. */
  restoreResume: (slug: string) => invoke<ResumeSummary>("restore_resume", { slug }),
  /** Erases the resume for good. */
  deleteResume: (slug: string) => invoke<void>("delete_resume", { slug }),
  readPosting: (slug: string) => invoke<TextFile>("read_posting", { slug }),
  writePosting: (slug: string, content: string, expectedHash?: string) =>
    invoke<TextFile>("write_posting", { slug, content, expectedHash }),
  readInstructions: () => invoke<InstructionFile>("read_instructions"),
  writeInstructions: (items: string[], expectedHash?: string) =>
    invoke<InstructionFile>("write_instructions", { items, expectedHash }),
  checkResume: (slug: string) => invoke<Report>("check_resume", { slug }),
  /** Asks where to save; resolves to the saved path, or null if cancelled. */
  savePdf: (slug: string) => invoke<string | null>("save_pdf", { slug }),
  printResume: (slug: string) => invoke<void>("print_resume", { slug }),
  listSkills: () => invoke<Skill[]>("list_skills"),
  licenses: () => invoke<Licenses>("licenses"),
  /** Resolves to the newer release, or null when this is the latest. */
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  /** Installs what the last check found, then restarts into it (on Windows the installer does). */
  installUpdate: (onProgress: (p: Progress) => void) => {
    const channel = new Channel<Progress>();
    channel.onmessage = onProgress;
    return invoke<void>("install_update", { onProgress: channel });
  },
};

export const isConflict = (e: unknown) => String(e).startsWith("conflict");

export const str = (v: string | string[] | undefined) => (Array.isArray(v) ? v.join(", ") : (v ?? ""));
export const list = (v: string | string[] | undefined) =>
  Array.isArray(v) ? v : v ? v.split(",").map((s) => s.trim()).filter(Boolean) : [];
