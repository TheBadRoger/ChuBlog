import assert from 'node:assert/strict';
import test from 'node:test';
import { renderMarkdown, highlightCode } from '../../apps/web/src/lib/markdown';
import type { Article } from '../../apps/web/src/lib/api';
const article = (markdown: string): Article => ({
  meta: { title: 'Test', slug: 'alice~first', author: 'alice', author_name: '爱丽丝', author_is_admin: false, description: '', tags: [], cover: null, created_at: '', date: '', updated: '', status: 'published' },
  markdown, format: 'markdown', entry: 'main.md',
  pages: [{ name: 'main.md', url: '' }, { name: 'chapter.md', url: '' }], assets: [{ name: 'cover.png', url: '' }],
});
test('rewrites only registered documents and assets, preserves anchors and creates heading IDs', () => {
  const html = renderMarkdown(article('# Intro\n[Next](./chapter.md#details)\n[Back](main.md)\n![Cover](cover.png)'), 'alice~first');
  assert.match(html, /<h2 id="intro">/);
  assert.match(html, /href="\/posts\/alice\/first\?page=chapter#details"/);
  assert.match(html, /href="\/posts\/alice\/first"/);
  assert.match(html, /src="\/api\/assets\/alice~first\/cover.png"/);
  assert.doesNotMatch(renderMarkdown(article('![bad](https://evil.example/a.png)'), 'alice~first'), /<img/);
});
test('treats raw HTML as text and excludes active links, forged images and handlers', () => {
  for (const text of ['<script>alert(1)</script>', '<img src=x onerror=alert(1)>', '[x](javascript:alert%281%29)', '[x](data:text/html,evil)', '[x](//evil.example)', '![bad](../secret.png)']) {
    const html = renderMarkdown(article(text), 'alice~first');
    assert.doesNotMatch(html, /<script|<img[^>]+onerror|href="(?:javascript:|data:|\/\/)|src="https?:/i);
  }
  const highlighted = renderMarkdown(article('```html\n<script>x</script>\n```'), 'alice~first');
  assert.doesNotMatch(highlighted, /<script|style=|onclick=/);
  assert.match(highlighted.replace(/<[^>]*>/g, ''), /&lt;script&gt;/);
});

test('highlights code with CSP-safe classes and escapes unknown languages', () => {
  const html = renderMarkdown(article('```rust\nfn main() { let name = "<img src=x onerror=alert(1)>"; }\n```'), 'alice~first');
  assert.match(html, /class="syntax-/);
  assert.doesNotMatch(html, /style=|<img|<script/);
  assert.match(renderMarkdown(article('```unknown-language\n<script>unsafe</script>\n```'), 'alice~first'), /&lt;script&gt;/);
});

test('shared guide highlighting colors shell and TOML while escaping HTML', () => {
  for (const [code, language] of [
    ["git commit -m '<script>alert(1)</script>'", 'bash'],
    ['title = "<img src=x onerror=alert(1)>"\npublic = true', 'toml'],
  ]) {
    const html = highlightCode(code, language);
    assert.match(html, /class="syntax-/);
    assert.doesNotMatch(html, /<script|<img|style=/);
    assert.match(html.replace(/<[^>]*>/g, ''), /&lt;/);
  }
});
