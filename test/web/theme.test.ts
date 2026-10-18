import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import test from 'node:test';

const source = readFileSync(new URL('../../apps/web/public/theme.js', import.meta.url), 'utf8');
function mount(saved: string | null = null, blocked = false) {
  const dataset: Record<string, string> = {};
  const attributes: Record<string, string> = {};
  const events: Record<string, () => void> = {};
  const icons = ['light', 'dark', 'system'].map(mode => ({
    dataset: { themeIcon: mode }, hidden: false,
    toggleAttribute(_name: string, hidden: boolean) { this.hidden = hidden; },
  }));
  const button = { dataset: {} as Record<string, string>,
    setAttribute(name: string, value: string) { attributes[name] = value; },
    querySelectorAll() { return icons; },
    addEventListener(name: string, fn: () => void) { events[name] = fn; },
  };
  runInNewContext(source, {
    document: { documentElement: { dataset }, querySelector: () => button,
      addEventListener: (_name: string, fn: () => void) => { fn(); } },
    localStorage: {
      getItem() { if (blocked) throw new Error('storage blocked'); return saved; },
      setItem(_key: string, value: string) { if (blocked) throw new Error('storage blocked'); saved = value; },
    },
  });
  return { dataset, button, icons, attributes, click: () => events.click(), saved: () => saved };
}

test('one theme button cycles modes and keeps icon, label and stored choice in sync', () => {
  const ui = mount();
  assert.equal(ui.button.dataset.themeMode, 'system');
  for (const mode of ['light', 'dark', 'system', 'light']) {
    ui.click();
    assert.equal(ui.button.dataset.themeMode, mode);
    assert.equal(ui.dataset.theme, mode === 'system' ? undefined : mode);
    assert.deepEqual(ui.icons.filter(icon => !icon.hidden).map(icon => icon.dataset.themeIcon), [mode]);
    assert.equal(ui.saved(), mode);
    assert.match(ui.attributes['aria-label'], /当前.+，切换为.+/);
  }
  assert.equal(mount(ui.saved()).button.dataset.themeMode, 'light');
});

test('invalid or unavailable theme storage keeps system default and the button usable', () => {
  for (const ui of [mount('invalid'), mount(null, true)]) {
    assert.equal(ui.button.dataset.themeMode, 'system');
    ui.click();
    assert.equal(ui.dataset.theme, 'light');
  }
});
