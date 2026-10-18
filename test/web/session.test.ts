import assert from 'node:assert/strict';
import test from 'node:test';
import { readSession } from '../../apps/web/src/lib/session';

const context = (cookie = '') => ({ request: new Request('http://blog.test/account', { headers: { Cookie: cookie } }), locals: {} });
const account = { username: 'owner', nickname: '管理员', permissions: { enabled: true, role: 'admin' }, csrf: 'test-csrf' };

test('one page request shares verification between permission gate and layout, with no cross-user cache', async () => {
  const original = globalThis.fetch;
  const cookies: string[] = [];
  globalThis.fetch = async (_url, init) => {
    cookies.push(new Headers(init?.headers).get('Cookie') || '');
    return Response.json(account);
  };
  try {
    const first = context('chublog_session=first');
    const gate = readSession(first);
    assert.equal(readSession(first), gate);
    assert.deepEqual(await gate, { status: 'authenticated', account });
    await readSession(context('chublog_session=second'));
    assert.deepEqual(cookies, ['chublog_session=first', 'chublog_session=second']);
  } finally { globalThis.fetch = original; }
});

test('anonymous pages skip database verification and outages remain distinct from expired sessions', async () => {
  const original = globalThis.fetch;
  let calls = 0;
  let status = 503;
  globalThis.fetch = async () => { calls++; return Response.json({ error: 'temporary' }, { status }); };
  try {
    assert.deepEqual(await readSession(context()), { status: 'anonymous' });
    assert.equal(calls, 0);
    assert.deepEqual(await readSession(context('chublog_session=valid')), { status: 'unavailable' });
    status = 401;
    assert.deepEqual(await readSession(context('chublog_session=expired')), { status: 'anonymous' });
    status = 403;
    assert.deepEqual(await readSession(context('chublog_session=banned')), { status: 'forbidden' });
    globalThis.fetch = async () => { throw new TypeError('network unavailable'); };
    assert.deepEqual(await readSession(context('chublog_session=valid')), { status: 'unavailable' });
  } finally { globalThis.fetch = original; }
});
