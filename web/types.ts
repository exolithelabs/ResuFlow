export type ResumeStatus = 'draft' | 'active' | 'archived';

export interface ResumeSummary {
  slug: string;
  title: string;
  status: ResumeStatus;
  updated: string;
  summary: string;
  tags: string[];
}

export interface Resume extends ResumeSummary {
  body: string;
  markdown: string;
}

export interface Profile {
  name: string;
  headline: string;
  email: string;
  phone: string;
  location: string;
  website: string;
  linkedin: string;
  github: string;
  body: string;
  markdown: string;
}

export interface Skill {
  slug: string;
  source: 'builtin' | 'user';
  name: string;
  description: string;
  markdown: string;
  body: string;
  enabled: boolean;
}

export interface Memory {
  markdown: string;
}

export interface WorkspaceInfo {
  root: string;
  name: string;
  schemaVersion: number;
  defaultVault: string;
  recentVaults: string[];
}

export interface ReleaseCheck {
  checked: boolean;
  available: boolean;
  currentVersion: string;
  latestVersion?: string | null;
  tagName?: string;
  name?: string;
  publishedAt?: string | null;
  url?: string;
}

export interface McpInfo {
  url: string;
  config: {
    mcpServers: {
      resuflow: {
        url: string;
        headers: {
          Authorization: string;
        };
      };
    };
  };
}
