import type { APIRoute, APIContext } from 'astro';

const hopHeaders = ['connection', 'keep-alive', 'proxy-authenticate', 'proxy-authorization',
  'te', 'trailer', 'transfer-encoding', 'upgrade', 'content-length'];

let activeGit = 0;
export const proxy: APIRoute = async (context) => {
  const git = context.url.pathname.startsWith('/git/');
  if (git && activeGit >= 2) return new Response(null, { status: 429 });
  if (git) activeGit++;
  try { return await forward(context); }
  finally { if (git) activeGit--; }
};
async function forward({ request, url }: APIContext): Promise<Response> {
  const git = url.pathname.startsWith('/git/');
  const limit = git ? 9 * 1024 * 1024 : 8192;
  let body: Uint8Array | undefined;
  if (request.body) {
    const reader = request.body.getReader();
    const chunks: Uint8Array[] = [];
    let size = 0;
    let expired = false;
    const timer = setTimeout(() => { expired = true; void reader.cancel(); }, 15000);
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        size += value.byteLength;
        if (size > limit) {
          await reader.cancel();
          return new Response(null, { status: 413 });
        }
        chunks.push(value);
      }
    } finally { clearTimeout(timer); }
    if (expired) return new Response(null, { status: 408 });
    body = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
      body.set(chunk, offset);
      offset += chunk.byteLength;
    }
  }
  const headers = new Headers(request.headers);
  // The public listener is loopback-only; the server's proxy must overwrite these headers.
  headers.set('X-Forwarded-Host', headers.get('X-Forwarded-Host') || headers.get('Host') || url.host);
  headers.set('X-Forwarded-Proto', headers.get('X-Forwarded-Proto') || 'http');
  for (const name of [...hopHeaders, 'host']) headers.delete(name);
  try {
    const upstream = await fetch(new URL(url.pathname + url.search,
      process.env.API_BASE_URL || 'http://127.0.0.1:3001'), {
      method: request.method, headers, body: body as BodyInit | undefined,
      redirect: 'manual', signal: AbortSignal.timeout(git ? 95000 : 15000),
    });
    const responseHeaders = new Headers(upstream.headers);
    for (const name of [...hopHeaders, 'content-encoding']) responseHeaders.delete(name);
    return new Response(upstream.body, { status: upstream.status, headers: responseHeaders });
  } catch {
    return Response.json({ error: '文章服务暂时不可用' }, { status: 502 });
  }
}
