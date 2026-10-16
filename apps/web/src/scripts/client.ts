export {};
type Role = 'reader' | 'publisher' | 'admin';
interface Grant { enabled: boolean; role: Role; ssh_key: string | null }
interface Account { username: string; permissions: Grant; csrf: string }
let csrf: string | undefined;
const roles: Record<Role, string> = { reader: '普通用户', publisher: '作者', admin: '管理员' };

async function api<T>(path: string, method = 'GET', data?: unknown): Promise<T> {
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (data !== undefined) headers['Content-Type'] = 'application/json';
  if (csrf && method !== 'GET') headers['X-CSRF-Token'] = csrf;
  const response = await fetch(path, { method, credentials: 'same-origin', headers,
    body: data === undefined ? undefined : JSON.stringify(data), signal: AbortSignal.timeout(10000) });
  const result = await response.json().catch(() => null);
  if (!response.ok) throw new Error(result?.error || (response.status === 429 ? '请求过于频繁，请稍后重试。' : '操作未完成，请稍后重试。'));
  return result as T;
}
function showStatus(element: HTMLElement | null, message: string, error = false) {
  if (!element) return;
  element.textContent = message;
  element.dataset.error = String(error);
}
function errorMessage(error: unknown): string {
  return error instanceof Error && error.name !== 'TimeoutError' && error.name !== 'TypeError'
    ? error.message : '无法连接服务器，请检查网络后重试。';
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
    showStatus(message, '正在处理…');
    try { await action(new FormData(form), form); }
    catch (error) { showStatus(message, errorMessage(error), true); }
    finally { button.disabled = false; form.removeAttribute('aria-busy'); }
  });
}
const value = (data: FormData, name: string) => String(data.get(name) || '');
function checkPassword(password: string) {
  const size = new TextEncoder().encode(password).length;
  if (size < 12 || size > 1024) throw new Error('密码需要在 12 至 1024 个字节之间。');
}

formAction('auth-form', async (data, form) => {
  const credentials = { username: value(data, 'username'), password: value(data, 'password') };
  if (form.dataset.mode === 'register') {
    checkPassword(credentials.password);
    if (credentials.password !== value(data, 'confirm')) throw new Error('两次输入的密码不一致。');
    await api('/api/auth/register', 'POST', credentials);
    // Registration succeeded even if the following login is temporarily rate-limited.
    try { await api('/api/auth/login', 'POST', credentials); }
    catch { location.assign('/login?registered=1'); return; }
  } else { await api('/api/auth/login', 'POST', credentials); }
  form.reset();
  location.assign('/account');
});

formAction('key-form', async (data, form) => {
  const key = value(data, 'public_key').trim();
  if (key.includes('PRIVATE KEY')) throw new Error('这里需要 .pub 公钥，请勿提交私钥。');
  await api('/api/account/ssh-key', 'PUT', { public_key: key });
  showStatus(form.querySelector('.form-status'), '公钥已保存。发布仍需站长授权。');
});
formAction('password-form', async (data, form) => {
  const password = value(data, 'new_password');
  checkPassword(password);
  await api('/api/account/password', 'PUT', { old_password: value(data, 'old_password'), new_password: password });
  form.reset();
  csrf = undefined;
  location.assign('/login?changed=1');
});

document.querySelector<HTMLButtonElement>('#logout')?.addEventListener('click', async (event) => {
  const button = event.currentTarget as HTMLButtonElement;
  button.disabled = true;
  try { await api('/api/auth/logout', 'POST', {}); csrf = undefined; location.assign('/'); }
  catch (error) { showStatus(document.querySelector('#account-status'), errorMessage(error), true); button.disabled = false; }
});

function element<K extends keyof HTMLElementTagNameMap>(tag: K, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  return node;
}
async function loadAdmin(account: Account) {
  const section = document.querySelector<HTMLElement>('#admin-section')!;
  section.hidden = false;
  try {
    const [users, state] = await Promise.all([
      api<Record<string, Grant>>('/api/admin/users'),
      api<{ pending: Record<string, { commit: string; reason: string }> }>('/api/admin/status'),
    ]);
    const list = document.querySelector<HTMLElement>('#admin-users')!;
    for (const [username, grant] of Object.entries(users)) {
      const form = element('form'); form.className = 'admin-user';
      form.append(element('h4', username === account.username ? `${username}（当前账户）` : username));
      if (username === account.username) { form.append(element('p', '当前管理员')); list.append(form); continue; }
      const controls = element('div'); controls.className = 'admin-user-controls';
      const select = element('select'); select.setAttribute('aria-label', `${username} 的角色`);
      for (const [role, label] of Object.entries(roles)) {
        const option = element('option', label); option.value = role; option.selected = role === grant.role; select.append(option);
      }
      const label = element('label'); const enabled = element('input'); enabled.type = 'checkbox'; enabled.checked = grant.enabled;
      label.append(enabled, document.createTextNode('启用账户'));
      const button = element('button', '保存授权'); button.className = 'button button-quiet'; button.type = 'submit';
      controls.append(select, label, button);
      const message = element('p'); message.className = 'form-status'; message.setAttribute('role', 'status');
      form.append(controls, message);
      form.addEventListener('submit', async (event) => {
        event.preventDefault(); if (button.disabled) return; button.disabled = true;
        try { await api(`/api/admin/users/${encodeURIComponent(username)}`, 'PATCH', { enabled: enabled.checked, role: select.value }); showStatus(message, '授权已保存。'); }
        catch (error) { showStatus(message, errorMessage(error), true); }
        finally { button.disabled = false; }
      });
      list.append(form);
    }
    const pending = document.querySelector<HTMLElement>('#pending-indexes')!;
    const entries = Object.entries(state.pending);
    if (!entries.length) pending.append(element('p', '没有待修复的索引。'));
    for (const [author, item] of entries) {
      const row = element('div'); row.className = 'pending-item'; row.append(element('strong', `${author} / ${item.commit.slice(0, 8)}`), element('p', item.reason)); pending.append(row);
    }
  } catch (error) { showStatus(document.querySelector('#admin-status'), errorMessage(error), true); }
}

async function loadAccount() {
  const content = document.querySelector<HTMLElement>('#account-content');
  try {
    const account = await api<Account>('/api/account');
    csrf = account.csrf;
    const link = document.querySelector<HTMLAnchorElement>('[data-account-link]');
    if (link) { link.href = '/account'; link.textContent = '账户'; }
    if (!content) return;
    content.hidden = false;
    showStatus(document.querySelector('#account-status'), '');
    document.querySelector('#account-name')!.textContent = account.username;
    document.querySelector('#account-role')!.textContent = roles[account.permissions.role];
    document.querySelector<HTMLTextAreaElement>('#public-key')!.value = account.permissions.ssh_key || '';
    document.querySelector('#publish-status')!.textContent = account.permissions.role === 'reader'
      ? '发布文章需要登记公钥，并由站长授予发布权限。'
      : account.permissions.ssh_key ? '已获得发布权限，可以使用下方配置连接个人仓库。' : '已获得发布权限，登记公钥后即可连接个人仓库。';
    if (account.permissions.role === 'admin') await loadAdmin(account);
  } catch (error) {
    if (!content) return;
    showStatus(document.querySelector('#account-status'), errorMessage(error), true);
    const link = element('a', '前往登录'); link.href = '/login'; link.className = 'text-link';
    document.querySelector('#account-status')!.after(link);
  }
}
void loadAccount();
