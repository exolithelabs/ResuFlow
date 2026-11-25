import { invoke } from "@tauri-apps/api/core";

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
export interface TrashEntry {
  id: string;
  slug: string;
  title: string;
  deleted_ms: number;
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
  preview_base: string;
}
/** "profile" or "resume:<slug>" */
export type FileId = "profile" | `resume:${string}`;
export type FieldValue = string | string[] | null;
export type Agent = "claude" | "agents";

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
  deleteResume: (slug: string) => invoke<TrashEntry>("delete_resume", { slug }),
  listTrash: () => invoke<TrashEntry[]>("list_trash"),
  /** Resolves to the restored slug. */
  restoreResume: (id: string) => invoke<string>("restore_resume", { id }),
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
  installSkills: (agent: Agent) => invoke<string>("install_skills", { agent }),
};

export const isConflict = (e: unknown) => String(e).startsWith("conflict");

export const str = (v: string | string[] | undefined) => (Array.isArray(v) ? v.join(", ") : (v ?? ""));
export const list = (v: string | string[] | undefined) =>
  Array.isArray(v) ? v : v ? v.split(",").map((s) => s.trim()).filter(Boolean) : [];
