import type { Account, Grant, Role, SessionState, Metadata } from '../lib/api';
let csrf: string | undefined;
const roles: Record<Role, string> = { reader: '普通用户', publisher: '作者', admin: '管理员' };

class RequestError extends Error {
  constructor(public status: number, message: string) { super(message); }
}

async function api<T>(path: string, method = 'GET', data?: unknown): Promise<T> {
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (data !== undefined) headers['Content-Type'] = 'application/json';
  if (csrf && method !== 'GET') headers['X-CSRF-Token'] = csrf;
  const response = await fetch(path, { method, credentials: 'same-origin', headers,
    body: data === undefined ? undefined : JSON.stringify(data), signal: AbortSignal.timeout(10000) });
  const result = await response.json().catch(() => null);
  if (!response.ok) throw new RequestError(response.status, result?.error || (response.status === 429 ? '请求过于频繁，请稍后重试。' : '操作未完成，请稍后重试。'));
  return result as T;
}
function showStatus(element: HTMLElement | null, message: string, error = false) {
  if (!element) return;
  element.textContent = message;
  element.dataset.loading = 'false';
  element.removeAttribute('aria-busy');
  element.dataset.error = String(error);
}
function showLoading(element: HTMLElement | null, message: string) {
  showStatus(element, message);
  if (element) { element.dataset.loading = 'true'; element.setAttribute('aria-busy', 'true'); }
}
function errorMessage(error: unknown): string {
  return error instanceof Error && error.name !== 'TimeoutError' && error.name !== 'TypeError'
    ? error.message : '无法连接服务器，请检查网络后重试。';
}
function navigate(path: string) {
  const progress = document.querySelector<HTMLElement>('#page-progress');
  if (progress) progress.hidden = false;
  location.assign(path);
}
function formAction(id: string, action: (data: FormData, form: HTMLFormElement) => Promise<void>) {
  const form = document.querySelector<HTMLFormElement>(`#${id}`);
  form?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const button = form.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    const message = form.querySelector<HTMLElement>('.form-status');
    if (button.disabled) return;
    button.disabled = true;
    form.setAttribute('aria-busy', 'true');
    showLoading(message, '正在处理…');
    try { await action(new FormData(form), form); }
    catch (error) { showStatus(message, errorMessage(error), true); }
    finally { button.disabled = false; form.removeAttribute('aria-busy'); }
  });
}
const value = (data: FormData, name: string) => String(data.get(name) || '');
function checkPassword(password: string) {
  const size = [...password].length;
  if (size < 6 || size > 128) throw new Error('密码需要在 6 至 128 个字符之间。');
}

formAction('setup-form', async (data, form) => {
  const password = value(data, 'password');
  checkPassword(password);
  if (password !== value(data, 'confirm')) throw new Error('两次输入的密码不一致。');
  await api('/api/setup', 'POST', { token: value(data, 'token').trim(), name: value(data, 'name'),
    description: value(data, 'description'), username: value(data, 'username'), nickname: value(data, 'nickname'), password });
  form.reset();
  navigate('/login?installed=1');
});

formAction('auth-form', async (data, form) => {
  const credentials = { username: value(data, 'username'), password: value(data, 'password') };
  checkPassword(credentials.password);
  if (form.dataset.mode === 'register') {
    checkPassword(credentials.password);
    if (credentials.password !== value(data, 'confirm')) throw new Error('两次输入的密码不一致。');
    await api('/api/auth/register', 'POST', { ...credentials, nickname: value(data, 'nickname') });
    // Registration succeeded even if the following login is temporarily rate-limited.
    try { await api('/api/auth/login', 'POST', credentials); }
    catch { navigate('/login?registered=1'); return; }
  } else { await api('/api/auth/login', 'POST', credentials); }
  form.reset();
  navigate('/account');
});

formAction('password-form', async (data, form) => {
  const password = value(data, 'new_password');
  checkPassword(password);
  await api('/api/account/password', 'PUT', { old_password: value(data, 'old_password'), new_password: password });
  form.reset();
  csrf = undefined;
  navigate('/login?changed=1');
});

document.querySelector<HTMLButtonElement>('#logout')?.addEventListener('click', async (event) => {
  const button = event.currentTarget as HTMLButtonElement;
  button.disabled = true;
  button.dataset.loading = 'true';
  try { await api('/api/auth/logout', 'POST', {}); csrf = undefined; navigate('/'); }
  catch (error) { showStatus(document.querySelector('#account-status'), errorMessage(error), true); button.disabled = false; button.dataset.loading = 'false'; }
});

function element<K extends keyof HTMLElementTagNameMap>(tag: K, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  return node;
}
type PendingEntry = { commit: string; reason: string };

async function loadUserAdmin(account: Account) {
  const section = document.querySelector<HTMLElement>('#admin-section');
  if (!section) return;
  section.hidden = false;
  const list = document.querySelector<HTMLElement>('#admin-users')!;
  const status = document.querySelector<HTMLElement>('#admin-status');
  showLoading(status, '正在读取用户…');
  list.setAttribute('aria-busy', 'true');
  try {
    const users = await api<Record<string, Grant & { nickname: string }>>('/api/admin/users');
    const fragment = document.createDocumentFragment();
    for (const [username, grant] of Object.entries(users)) {
      const form = element('form'); form.className = 'admin-user';
      form.append(element('h4', `${grant.nickname}（${username}）${username === account.username ? ' · 当前账户' : ''}`));
      if (username === account.username) { form.append(element('p', '当前管理员')); fragment.append(form); continue; }
      const controls = element('div'); controls.className = 'admin-user-controls';
      const select = element('select'); select.setAttribute('aria-label', `${grant.nickname} 的状态与权限`);
      for (const [role, label] of Object.entries({ banned: '已封禁', ...roles })) {
        const option = element('option', label); option.value = role;
        option.selected = role === (grant.enabled ? grant.role : 'banned');
        if (role === 'admin' && grant.role !== 'admin') option.disabled = true;
        select.append(option);
      }
      const button = element('button', '保存授权'); button.className = 'button button-quiet'; button.type = 'submit';
      controls.append(select, button);
      const message = element('p'); message.className = 'form-status'; message.setAttribute('role', 'status');
      form.append(controls, message);
      form.addEventListener('submit', async (event) => {
        event.preventDefault(); if (button.disabled) return; button.disabled = true;
        form.setAttribute('aria-busy', 'true');
        showLoading(message, '正在保存…');
        try { await api(`/api/admin/users/${encodeURIComponent(username)}`, 'PATCH', { enabled: select.value !== 'banned', role: select.value === 'banned' ? 'reader' : select.value }); showStatus(message, '授权已保存。'); }
        catch (error) { showStatus(message, errorMessage(error), true); }
        finally { button.disabled = false; form.removeAttribute('aria-busy'); }
      });
      fragment.append(form);
    }
    list.replaceChildren(fragment);
    showStatus(status, '');
  } catch (error) {
    showRetry(status, errorMessage(error), () => { void loadUserAdmin(account); });
  } finally {
    list.setAttribute('aria-busy', 'false');
  }
}

function renderPending(container: HTMLElement, status: HTMLElement | null, entries: [string, PendingEntry][], users: Record<string, { nickname: string }> | null) {
  container.replaceChildren();
  if (!entries.length) container.append(element('p', '没有待修复的索引。'));
  for (const [author, item] of entries) {
    const row = element('div'); row.className = 'pending-item';
    row.append(element('strong', `${users?.[author]?.nickname || author} / ${item.commit.slice(0, 8)}`), element('p', item.reason));
    container.append(row);
  }
  showStatus(status, '');
}

async function loadAdminArticles() {
  const list = document.querySelector<HTMLElement>('#admin-articles');
  if (!list) return;
  const status = document.querySelector<HTMLElement>('#articles-status');
  const pending = document.querySelector<HTMLElement>('#pending-indexes');
  const pendingStatus = document.querySelector<HTMLElement>('#pending-status');
  showLoading(status, '正在读取文章…');
  showLoading(pendingStatus, '正在读取同步状态…');
  list.setAttribute('aria-busy', 'true');
  pending?.setAttribute('aria-busy', 'true');
  try {
    const [articles, state, users] = await Promise.all([
      api<{ posts: Metadata[] }>('/api/admin/articles'),
      api<{ pending: Record<string, PendingEntry> }>('/api/admin/status'),
      api<Record<string, { nickname: string }>>('/api/admin/users').catch(() => null),
    ]);
    list.replaceChildren();
    const username = (JSON.parse(document.body.dataset.session || '{}') as { account?: Account }).account?.username;
    for (const post of articles.posts) list.append(articleSettings(post, true, username));
    showStatus(status, articles.posts.length ? '' : '暂无文章。');
    if (pending) renderPending(pending, pendingStatus, Object.entries(state.pending), users);
  } catch (error) {
    showRetry(status, errorMessage(error), () => { void loadAdminArticles(); });
    showRetry(pendingStatus, errorMessage(error), () => { void loadAdminArticles(); });
  } finally {
    list.setAttribute('aria-busy', 'false');
    pending?.setAttribute('aria-busy', 'false');
  }
}

function showRetry(target: HTMLElement | null, message: string, retry: () => void) {
  showStatus(target, message, true);
  if (!target) return;
  const button = element('button', '重试'); button.type = 'button'; button.className = 'session-retry';
  button.addEventListener('click', retry, { once: true });
  target.append(' ', button);
}

let accountRequest = 0;
async function loadAccount(refresh = false) {
  const request = ++accountRequest;
  const content = document.querySelector<HTMLElement>('#account-content');
  const status = document.querySelector<HTMLElement>('#account-status');
  const link = document.querySelector<HTMLAnchorElement>('[data-account-link]');
  const usersLink = document.querySelector<HTMLAnchorElement>('[data-users-link]');
  const articlesLink = document.querySelector<HTMLAnchorElement>('[data-articles-link]');
  const requiredRole = document.querySelector<HTMLElement>('[data-requires-role]')?.dataset.requiresRole;
  const sessionStatus = document.querySelector<HTMLElement>('#session-status');
  const sessionMessage = document.querySelector<HTMLElement>('#session-message');
  const retry = document.querySelector<HTMLButtonElement>('#session-retry');
  link?.setAttribute('data-loading', 'true');
  if (retry) retry.disabled = true;
  if (status && !content?.hidden) showLoading(status, '正在确认账户…');
  try {
    let state: SessionState = JSON.parse(document.body.dataset.session || '{"status":"unavailable"}');
    if (refresh || state.status === 'unavailable') {
      state = { status: 'authenticated', account: await api<Account>('/api/account') };
    }
    if (request !== accountRequest) return;
    document.body.dataset.session = JSON.stringify(state);
    if (state.status !== 'authenticated') {
      csrf = undefined;
      if (content) content.hidden = true;
      if (usersLink) usersLink.hidden = true;
      if (articlesLink) articlesLink.hidden = true;
      if (link) { link.href = '/login'; link.firstChild!.textContent = '登录'; }
      showStatus(status, state.status === 'forbidden' ? '此账户已停用。' : '登录后即可管理账户与发布文章。');
      if (state.status === 'anonymous' && status) {
        const login = element('a', '前往登录'); login.href = '/login'; login.className = 'text-link';
        status.append(' ', login);
      }
      if (sessionStatus) sessionStatus.hidden = true;
      if (requiredRole) location.replace(state.status === 'anonymous' ? '/login' : '/');
      return;
    }
    const account = state.account;
    const role = account.permissions.role;
    csrf = account.csrf;
    if (link) { link.href = '/account'; link.firstChild!.textContent = '账户'; }
    if (usersLink) usersLink.hidden = role !== 'admin';
    if (articlesLink) articlesLink.hidden = role === 'reader';
    if (sessionStatus) sessionStatus.hidden = true;
    if (requiredRole && (requiredRole === 'admin' ? role !== 'admin' : role === 'reader')) {
      location.replace('/');
      return;
    }
    if (document.querySelector('#admin-section')) { void loadUserAdmin(account); return; }
    if (document.querySelector('#admin-articles')) { void loadAdminArticles(); return; }
    if (document.querySelector('#my-articles')) { void loadMyArticles(); return; }
    if (!content) return;
    content.hidden = false;
    showStatus(status, '');
    document.querySelector('#account-name')!.textContent = account.nickname;
    document.querySelector('#account-username')!.textContent = `(${account.username})`;
    document.querySelector('#account-role')!.textContent = roles[account.permissions.role];
    document.querySelector('#publish-status')!.textContent = account.permissions.role === 'reader'
      ? '管理员可以为你授予发布权限，授权后即可使用 Git 克隆仓库和发布文章。'
      : '已获得发布权限，可以使用网站账号密码访问个人文章仓库。';
  } catch (error) {
    if (request !== accountRequest) return;
    if (error instanceof RequestError && [401, 403].includes(error.status)) {
      document.body.dataset.session = JSON.stringify({ status: error.status === 401 ? 'anonymous' : 'forbidden' });
      void loadAccount();
      return;
    }
    showStatus(status, '账户暂时无法读取，请使用顶部的重试按钮。', true);
    showStatus(sessionMessage, errorMessage(error), true);
    if (sessionStatus) sessionStatus.hidden = false;
  } finally {
    if (request === accountRequest) {
      link?.setAttribute('data-loading', 'false');
      if (retry) retry.disabled = false;
    }
  }
}
document.querySelector('#session-retry')?.addEventListener('click', () => { void loadAccount(true); });
window.addEventListener('pageshow', event => { if (event.persisted) void loadAccount(true); });
void loadAccount();

const pageProgress = document.querySelector<HTMLElement>('#page-progress');
document.addEventListener('click', event => {
  if (event.defaultPrevented || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
  const anchor = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>('a[href]') : null;
  if (!anchor || anchor.hasAttribute('download') || (anchor.target && anchor.target !== '_self')) return;
  const url = new URL(anchor.href);
  if (url.origin === location.origin && (url.pathname !== location.pathname || url.search !== location.search) && pageProgress) pageProgress.hidden = false;
});
document.addEventListener('submit', event => { if (!event.defaultPrevented && pageProgress) pageProgress.hidden = false; });
window.addEventListener('pageshow', () => { if (pageProgress) pageProgress.hidden = true; });

type AccessUser = { username: string; nickname: string; source: 'PASSWORD' | 'GRANTED' };

function accessPanel(slug: string): HTMLElement {
  const panel = element('div'); panel.className = 'access-panel';
  panel.append(element('h4', '绝密文章可见用户'));
  const list = element('div'); list.className = 'access-list';
  const form = element('form'); form.className = 'access-form';
  const input = element('input'); input.type = 'text'; input.name = 'username'; input.placeholder = '用户账号'; input.autocomplete = 'off'; input.required = true;
  input.setAttribute('aria-label', '要授权的用户账号');
  const add = element('button', '授权'); add.type = 'submit'; add.className = 'button button-quiet';
  const status = element('p'); status.className = 'form-status'; status.setAttribute('role', 'status');
  form.append(input, add);
  panel.append(list, form, status);
  const endpoint = `/api/articles/${encodeURIComponent(slug)}/access`;
  const refresh = async () => {
    const { users } = await api<{ users: AccessUser[] }>(endpoint);
    list.replaceChildren();
    if (!users.length) list.append(element('p', '尚未指定用户。'));
    for (const user of users) {
      const row = element('div'); row.className = 'access-row';
      row.append(element('span', `${user.nickname}（${user.username}）${user.source === 'PASSWORD' ? ' · 口令解锁' : ''}`));
      const remove = element('button', '移除'); remove.type = 'button'; remove.className = 'text-link';
      remove.addEventListener('click', async () => {
        remove.disabled = true;
        try { await api(endpoint, 'POST', { username: user.username, allowed: false }); showStatus(status, '已移除授权。'); await refresh(); }
        catch (error) { showStatus(status, errorMessage(error), true); remove.disabled = false; }
      });
      row.append(remove); list.append(row);
    }
  };
  form.addEventListener('submit', async event => {
    event.preventDefault(); if (add.disabled) return;
    add.disabled = true; form.setAttribute('aria-busy', 'true'); showLoading(status, '正在授权…');
    try {
      await api(endpoint, 'POST', { username: input.value.trim(), allowed: true });
      input.value = ''; showStatus(status, '已授权。'); await refresh();
    } catch (error) { showStatus(status, errorMessage(error), true); }
    finally { add.disabled = false; form.removeAttribute('aria-busy'); }
  });
  void refresh().catch(error => showStatus(status, errorMessage(error), true));
  return panel;
}

function articleSettings(post: Metadata, admin: boolean, username?: string): HTMLElement {
  const isAuthor = post.author === username;
  const form = element('form'); form.className = 'admin-user admin-article';
  const blocked = post.review === 'blocked';
  form.append(element('h3', `${post.title} · ${post.author_name}`));
  form.append(element('p', blocked ? '已封禁：仅作者和管理员可见，对外显示为文章已删除。'
    : post.status === 'draft' ? '作者草稿，管理员设置不会将其发布。' : '已发布'));
  const controls = element('div'); controls.className = 'admin-user-controls';
  let featured: HTMLInputElement | undefined;
  if (admin) {
    const label = element('label', '精华 '); featured = element('input'); featured.type = 'checkbox'; featured.checked = !!post.featured; label.append(featured); controls.append(label);
  }
  const visibility = element('select'); visibility.setAttribute('aria-label', `${post.title} 的可见性`);
  for (const [value, label] of Object.entries({ default: '默认（所有人）', secret: '秘密（登录用户）', confidential: '机密（口令）', top: '绝密（指定用户）' })) {
    const option = element('option', label); option.value = value; option.selected = value === (post.visibility || 'default'); visibility.append(option);
  }
  const hidden = element('select'); hidden.setAttribute('aria-label', `${post.title} 的隐藏等级`);
  for (const [value, label] of Object.entries({ semi: '半隐藏（可搜索）', full: '全隐藏' })) {
    const option = element('option', label); option.value = value; option.selected = value === (post.hidden || 'full'); hidden.append(option);
  }
  const hiddenLabel = element('label', '隐藏等级 '); hiddenLabel.append(hidden);
  if (!isAuthor) hiddenLabel.title = '隐藏等级仅文章作者可设置';
  controls.append(hiddenLabel);
  const password = element('input'); password.type = 'password'; password.minLength = 6; password.maxLength = 128; password.autocomplete = 'new-password';
  password.placeholder = post.password_required ? '已设置口令，留空保留' : '设置查阅口令（至少 6 位）'; password.setAttribute('aria-label', `${post.title} 的查阅口令`);
  const clearLabel = element('label', '移除口令 '); const clear = element('input'); clear.type = 'checkbox'; clearLabel.append(clear);
  const save = element('button', '保存设置'); save.type = 'submit'; save.className = 'button button-quiet';
  controls.append(visibility, password, clearLabel, save);
  const message = element('p'); message.className = 'form-status'; message.setAttribute('role', 'status');
  form.append(controls, message);
  const sync = () => {
    const confidential = visibility.value === 'confidential';
    if (confidential && isAuthor) hidden.value = 'full';
    hidden.disabled = !isAuthor || confidential;
    password.disabled = !confidential; clear.disabled = !confidential;
    if (!confidential) { password.value = ''; clear.checked = false; }
  };
  visibility.addEventListener('change', sync); sync();
  const path = `${admin ? '/api/admin/articles' : '/api/articles'}/${encodeURIComponent(post.slug)}`;
  form.addEventListener('submit', async event => {
    event.preventDefault(); save.disabled = true;
    try {
      const data: Record<string, unknown> = { visibility: visibility.value };
      if (admin && featured) data.featured = featured.checked;
      if (isAuthor) data.hidden = hidden.value;
      if (visibility.value === 'confidential') {
        if (clear.checked) data.password = '';
        else if (password.value) data.password = password.value;
      }
      await api(path, 'PATCH', data);
      password.value = ''; clear.checked = false; showStatus(message, '设置已保存。');
      if (!admin) void loadMyArticles();
      else void loadAdminArticles();
    } catch (error) { showStatus(message, errorMessage(error), true); }
    finally { save.disabled = false; }
  });
  if (admin) {
    const review = element('button', blocked ? '解除封禁' : '封禁文章'); review.type = 'button'; review.className = 'button button-quiet';
    review.addEventListener('click', async () => {
      if (blocked) {
        review.disabled = true;
        try { await api(path, 'PATCH', { review: 'open' }); showStatus(message, '已解除封禁。'); await loadAdminArticles(); }
        catch (error) { showStatus(message, errorMessage(error), true); review.disabled = false; }
        return;
      }
      if (!confirm(`封禁《${post.title}》？网站会将它显示为已删除，作者修改并发布后自动重新开放。`)) return;
      review.disabled = true;
      try { await api(path, 'DELETE'); await loadAdminArticles(); }
      catch (error) { showStatus(message, errorMessage(error), true); review.disabled = false; }
    });
    controls.append(review);
  } else if (blocked) {
    message.textContent = '封禁期间修改正文并推送后，文章会自动重新开放。';
  }
  if (post.visibility === 'top') form.append(accessPanel(post.slug));
  return form;
}

async function loadMyArticles() {
  const list = document.querySelector<HTMLElement>('#my-articles');
  const status = document.querySelector<HTMLElement>('#my-articles-status');
  const pending = document.querySelector<HTMLElement>('#my-pending-indexes');
  const pendingStatus = document.querySelector<HTMLElement>('#my-pending-status');
  if (!list || !document.body.dataset.session) return;
  let account: Account;
  try { account = JSON.parse(document.body.dataset.session).account; }
  catch { return; }
  if (!account?.username || account.permissions.role !== 'publisher') { location.replace('/'); return; }
  showLoading(status, '正在读取文章…');
  showLoading(pendingStatus, '正在读取同步状态…');
  list.setAttribute('aria-busy', 'true');
  pending?.setAttribute('aria-busy', 'true');
  try {
    const { posts, pending: pendingMap } = await api<{ posts: Metadata[]; pending?: Record<string, PendingEntry> }>('/api/articles');
    const mine = posts.filter(post => post.author === account.username);
    list.replaceChildren();
    for (const post of mine) list.append(articleSettings(post, false, account.username));
    if (!mine.length) list.append(element('p', '还没有文章。'));
    showStatus(status, '');
    if (pending && pendingMap) renderPending(pending, pendingStatus, Object.entries(pendingMap), { [account.username]: { nickname: account.nickname } });
    else showStatus(pendingStatus, '');
  } catch (error) {
    showRetry(status, errorMessage(error), () => { void loadMyArticles(); });
    showRetry(pendingStatus, errorMessage(error), () => { void loadMyArticles(); });
  } finally {
    list.setAttribute('aria-busy', 'false');
    pending?.setAttribute('aria-busy', 'false');
  }
}

formAction('article-unlock-form', async (data, form) => {
  if (!csrf) { const account = await api<Account>('/api/account'); csrf = account.csrf; }
  await api(`/api/posts/${encodeURIComponent(form.dataset.slug!)}/unlock`, 'POST', { password: value(data, 'password') });
  form.reset(); location.reload();
});
