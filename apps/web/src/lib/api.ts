export interface SiteState { installed: boolean; owner?: string | null; site: { name: string; description: string } | null }
export interface Metadata {
  title: string;
  slug: string;
  author: string;
  author_name: string;
  description: string;
  tags: string[];
  cover: string | null;
  created_at: string;
  date: string;
  updated: string;
  status: 'published' | 'draft';
}
export interface DocumentLink { name: string; url: string }
export interface Article {
  meta: Metadata;
  markdown: string;
  format: 'markdown' | 'text';
  entry?: string;
  name?: string;
  pages: DocumentLink[];
  assets: DocumentLink[];
}
export class ApiError extends Error {
  constructor(public status: number) { super('文章服务暂时不可用'); }
}

export async function readApi<T>(path: string, headers?: Record<string, string>): Promise<T> {
  const base = process.env.API_BASE_URL || 'http://127.0.0.1:3001';
  const response = await fetch(new URL(path, base), {
    signal: AbortSignal.timeout(5000),
    headers: { Accept: 'application/json', ...headers },
    cache: 'no-store',
  });
  if (!response.ok) throw new ApiError(response.status);
  return response.json() as Promise<T>;
}
export const postUrl = (slug: string) => `/posts/${encodeURIComponent(slug)}`;
export const pageUrl = (slug: string, name: string, entry?: string) =>
  name === entry ? postUrl(slug) : `${postUrl(slug)}/${encodeURIComponent(name)}`;
export const assetUrl = (slug: string, name: string) =>
  `/api/assets/${encodeURIComponent(slug)}/${encodeURIComponent(name)}`;
export function displayDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? '' : new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric', month: 'long', day: 'numeric', timeZone: 'Asia/Shanghai',
  }).format(date);
}
