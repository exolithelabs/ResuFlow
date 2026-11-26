// Shared app state and the hooks views use to reach each other without circular imports.

import type { Info, ResumeSummary } from "./api";

export type Route =
  | { kind: "resume"; slug: string }
  | { kind: "template" }
  | { kind: "instructions" }
  | { kind: "skills"; name?: string }
  | { kind: "connect" }
  | { kind: "archive" }
  | { kind: "about" }
  | { kind: "welcome" };

export interface View {
  section: HTMLElement;
  show(route: Route): Promise<void>;
  /** Save anything pending before another view takes over. */
  leave(): Promise<void>;
  /** Something changed on disk (an agent, another app). */
  onDiskChange(): Promise<void>;
}

export const store = {
  info: null as Info | null,
  resumes: [] as ResumeSummary[],
  route: { kind: "welcome" } as Route,
};

/** Filled in by main.ts. */
export const app = {
  navigate: async (_route: Route) => {},
  refreshList: async () => {},
  newResume: (_from?: string) => {},
  archive: async (_slug: string) => {},
  restore: async (_slug: string) => {},
  /** Asks first, then erases the resume for good. */
  deleteResume: async (_slug: string) => {},
  /** Save every pending edit now (before an update restarts the app). */
  saveAll: async () => {},
};

export const resumeRoute = (slug?: string | null): Route => (slug ? { kind: "resume", slug } : { kind: "welcome" });
/** Archived resumes live in the Archive view, not the sidebar list. */
export const activeResumes = () => store.resumes.filter((r) => r.status !== "archived");
export const firstResume = () => activeResumes()[0];
