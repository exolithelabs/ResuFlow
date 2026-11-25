// Shared app state and the hooks views use to reach each other without circular imports.

import type { Info, Parts, ResumeSummary } from "./api";

export type Route =
  | { kind: "resume"; slug: string }
  | { kind: "template" }
  | { kind: "instructions" }
  | { kind: "skills"; name?: string }
  | { kind: "connect" }
  | { kind: "trash" }
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
  /** Front matter of the default template: the header every resume falls back to. */
  profile: {} as Parts["fields"],
  route: { kind: "welcome" } as Route,
};

/** Filled in by main.ts. */
export const app = {
  navigate: async (_route: Route) => {},
  refreshList: async () => {},
  newResume: (_from?: string) => {},
  trash: async (_slug: string) => {},
};

export const resumeRoute = (slug?: string | null): Route => (slug ? { kind: "resume", slug } : { kind: "welcome" });
export const firstResume = () => store.resumes.find((r) => r.status !== "archived") ?? store.resumes[0];
