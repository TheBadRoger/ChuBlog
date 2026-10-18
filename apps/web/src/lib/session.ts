import { ApiError, readApi, type Account, type SessionState } from './api';

export function readSession(context: { request: Request; locals: App.Locals }): Promise<SessionState> {
  // Reuse verification within this request, without caching permissions across requests.
  return context.locals.accountState ??= (async (): Promise<SessionState> => {
    const cookie = context.request.headers.get('cookie') || '';
    if (!/(?:^|;)\s*chublog_session=/.test(cookie)) return { status: 'anonymous' };
    try {
      const account = await readApi<Account>('/api/account', { Cookie: cookie });
      return { status: 'authenticated', account };
    } catch (error) {
      return { status: error instanceof ApiError && error.status === 401 ? 'anonymous'
        : error instanceof ApiError && error.status === 403 ? 'forbidden' : 'unavailable' };
    }
  })();
}
