export interface SiteState { installed: boolean; owner?: string | null; site: { name: string; description: string } | null }
export interface Metadata {
  title: string;
  slug: string;
  author: string;
  author_name: string;
  author_is_admin: boolean;
  description: string;
  tags: string[];
  cover: string | null;
  created_at: string;
  date: string;
  updated: string;
  status: 'published' | 'draft';
  featured?: boolean;
  visibility?: 'default' | 'secret' | 'confidential' | 'top';
  hidden?: 'semi' | 'full';
  review?: 'open' | 'blocked';
  deleted?: boolean;
  password_required?: boolean;
}
export interface LockedBody {
  error: string;
  locked: true;
  reason: 'login' | 'password' | 'denied';
  meta: Metadata | null;
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
  constructor(public status: number, public body: unknown = null) { super('文章服务暂时不可用'); }
  get locked(): LockedBody | null {
    const value = this.body as Partial<LockedBody> | null;
    return value && value.locked === true && typeof value.reason === 'string' ? value as LockedBody : null;
  }
}

export async function readApi<T>(path: string, headers?: Record<string, string>): Promise<T> {
  const base = process.env.API_BASE_URL || 'http://127.0.0.1:3001';
  const response = await fetch(new URL(path, base), {
    signal: AbortSignal.timeout(5000),
    headers: { Accept: 'application/json', ...headers },
    cache: 'no-store',
  });
  if (!response.ok) throw new ApiError(response.status, await response.json().catch(() => null));
  return response.json() as Promise<T>;
}
export function postUrl(slug: string): string {
  const match = /^([a-z_-][a-z0-9_-]{2,31})~([a-z0-9-]+)$/.exec(slug);
  if (!match) throw new Error('文章标识无效');
  return `/posts/${encodeURIComponent(match[1])}/${encodeURIComponent(match[2])}`;
}
export const pageUrl = (slug: string, name: string, entry?: string) =>
  name === (entry || 'main.md') ? postUrl(slug) : `${postUrl(slug)}?page=${encodeURIComponent(name.replace(/\.(md|markdown|txt)$/, ''))}`;
export const assetUrl = (slug: string, name: string) =>
  `/api/assets/${encodeURIComponent(slug)}/${encodeURIComponent(name)}`;
export function displayDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? '' : new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric', month: 'long', day: 'numeric', timeZone: 'Asia/Shanghai',
  }).format(date);
}

export type Role = 'reader' | 'publisher' | 'admin';
export interface Grant { enabled: boolean; role: Role }
export interface Account { username: string; nickname: string; permissions: Grant; csrf: string }
export type SessionState = { status: 'authenticated'; account: Account } |
  { status: 'anonymous' | 'unavailable' | 'forbidden' };
