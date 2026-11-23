export interface ApiOptions extends Omit<RequestInit, 'headers'> {
  headers?: Record<string, string>;
}

export async function api<T = unknown>(endpoint: string, options: ApiOptions = {}): Promise<T> {
  const response = await fetch(`/api/${endpoint}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      'X-ResuFlow-Request': '1',
      ...(options.headers || {}),
    },
  });
  if (response.status === 204) return null as T;
  const payload = await response.json().catch(() => ({} as { error?: string }));
  if (!response.ok) {
    throw new Error(payload.error || `Request failed (${response.status})`);
  }
  return payload as T;
}

export function withSlug(slug: string, options: ApiOptions = {}): ApiOptions {
  return {
    ...options,
    headers: {
      'X-ResuFlow-Slug': String(slug),
      ...(options.headers || {}),
    },
  };
}
