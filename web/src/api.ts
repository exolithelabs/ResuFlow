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
}
/** A Markdown file split into front matter fields and body. */
export interface Parts {
  fields: Record<string, string | string[]>;
  body: string;
  body_offset: number;
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
export interface Report {
  words: number;
  bullets: number;
  bullets_with_numbers: number;
  est_pages: number;
  findings: Finding[];
}
export interface Info {
  root: string;
  mcp_config: unknown;
  mcp_command: string;
  version: string;
  pdf_export: boolean;
}
/** "profile", "instructions" or "resume:<slug>" */
export type FileId = string;
export interface InstructionFile {
  items: string[];
  hash: string;
}
export type FieldValue = string | string[] | null;

export const api = {
  info: () => invoke<Info>("info"),
  revision: () => invoke<string>("revision"),
  listResumes: () => invoke<ResumeSummary[]>("list_resumes"),
  createResume: (title: string) => invoke<string>("create_resume", { title }),
  readParts: (file: FileId) => invoke<Parts>("read_parts", { file }),
  writeBody: (file: FileId, body: string, expectedHash?: string) =>
    invoke<Parts>("write_body", { file, body, expectedHash }),
  setFields: (file: FileId, fields: Record<string, FieldValue>, expectedHash?: string) =>
    invoke<Parts>("set_fields", { file, fields, expectedHash }),
  setMeta: (slug: string, meta: { title?: string; status?: Status; tags?: string[] }) =>
    invoke<Parts>("set_meta", { slug, ...meta }),
  readInstructions: () => invoke<InstructionFile>("read_instructions"),
  writeInstructions: (items: string[], expectedHash?: string) =>
    invoke<InstructionFile>("write_instructions", { items, expectedHash }),
  deleteResume: (slug: string) => invoke<string>("delete_resume", { slug }),
  checkResume: (slug: string) => invoke<Report>("check_resume", { slug }),
  /** Asks where to save; resolves to the saved path, or null if cancelled. */
  savePdf: (slug: string) => invoke<string | null>("save_pdf", { slug }),
  listSkills: () => invoke<Skill[]>("list_skills"),
  installSkills: (agent: "claude" | "agents") => invoke<string>("install_skills", { agent }),
};

export const str = (v: string | string[] | undefined) => (Array.isArray(v) ? v.join(", ") : (v ?? ""));
export const list = (v: string | string[] | undefined) =>
  Array.isArray(v) ? v : v ? v.split(",").map((s) => s.trim()).filter(Boolean) : [];
