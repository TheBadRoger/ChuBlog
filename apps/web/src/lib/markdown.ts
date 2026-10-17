import { createHighlighter, type BundledLanguage } from 'shiki';
import MarkdownIt from 'markdown-it';
import sanitizeHtml from 'sanitize-html';
import { assetUrl, pageUrl, type Article } from './api';

const highlighter = await createHighlighter({ themes: ['github-light'],
  langs: ['rust', 'typescript', 'javascript', 'json', 'bash', 'sql', 'toml', 'html', 'css', 'markdown', 'python'] });
const tokenClasses: Record<string, string> = {
  '#d73a49': 'red', '#cf222e': 'red', '#005cc5': 'blue', '#0550ae': 'blue', '#032f62': 'blue', '#0a3069': 'blue',
  '#6f42c1': 'purple', '#8250df': 'purple', '#22863a': 'green', '#116329': 'green',
  '#e36209': 'orange', '#953800': 'orange', '#6a737d': 'gray', '#6e7781': 'gray',
};
const escape = (text: string) => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
export function highlightCode(code: string, language: string): string {
  const lang = language.toLowerCase();
  if (!highlighter.getLoadedLanguages().includes(lang)) return '';
  return highlighter.codeToTokensBase(code, { lang: lang as BundledLanguage, theme: 'github-light' })
    .map(line => line.map(token => {
      const name = tokenClasses[token.color?.toLowerCase() || ''];
      return name ? `<span class="syntax-${name}">${escape(token.content)}</span>` : escape(token.content);
    }).join('')).join('\n');
}

export function renderMarkdown(article: Article, slug: string): string {
  const pages = new Set(article.pages.map((page) => page.name));
  const assets = new Set(article.assets.map((asset) => asset.name));
  const parser = new MarkdownIt({ html: false, linkify: false, typographer: false, highlight: highlightCode });
  const normalLink = parser.renderer.rules.link_open || ((tokens, index, options, _env, self) => self.renderToken(tokens, index, options));
  parser.renderer.rules.link_open = (tokens, index, options, env, self) => {
    const token = tokens[index];
    const href = token.attrGet('href') || '';
    if (!/^(?:https?:\/\/|mailto:|\/|#)/i.test(href)) {
      const [path, ...fragment] = href.split('#');
      const name = path.replace(/^\.\//, '');
      token.attrSet('href', pages.has(name)
        ? pageUrl(slug, name, article.entry) + (fragment.length ? `#${fragment.join('#')}` : '') : '#');
    }
    if (/^https?:\/\//i.test(href)) token.attrSet('rel', 'noopener noreferrer');
    return normalLink(tokens, index, options, env, self);
  };
  const normalImage = parser.renderer.rules.image!;
  parser.renderer.rules.image = (tokens, index, options, env, self) => {
    const token = tokens[index];
    const name = (token.attrGet('src') || '').split('#')[0].replace(/^\.\//, '');
    if (!assets.has(name)) return '';
    token.attrSet('src', assetUrl(slug, name));
    token.attrSet('loading', 'lazy');
    token.attrSet('decoding', 'async');
    return normalImage(tokens, index, options, env, self);
  };
  const used = new Map<string, number>();
  parser.core.ruler.push('heading-anchors', (state) => {
    state.tokens.forEach((token, index) => {
      if (token.type !== 'heading_open') return;
      const text = state.tokens[index + 1]?.content || '';
      const base = text.normalize('NFKC').toLowerCase().replace(/[^\p{L}\p{N}_-]+/gu, '-').replace(/^-|-$/g, '') || 'section';
      const count = used.get(base) || 0;
      used.set(base, count + 1);
      token.attrSet('id', count ? `${base}-${count}` : base);
      const level = Math.min(Number(token.tag.slice(1)) + 1, 6);
      token.tag = `h${level}`;
      const close = state.tokens[index + 2];
      if (close?.type === 'heading_close') close.tag = token.tag;
    });
  });
  return sanitizeHtml(parser.render(article.markdown), {
    allowedTags: ['span', 'p', 'a', 'em', 'strong', 's', 'blockquote', 'pre', 'code', 'ul', 'ol', 'li', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'br', 'img', 'table', 'thead', 'tbody', 'tr', 'th', 'td'],
    allowedAttributes: {
      a: ['href', 'title', 'rel'], img: ['src', 'alt', 'title', 'loading', 'decoding'],
      span: ['class'], code: ['class'], ol: ['start'], h2: ['id'], h3: ['id'], h4: ['id'], h5: ['id'], h6: ['id'],
    },
    allowedClasses: { span: [/^syntax-(red|blue|purple|green|orange|gray)$/], code: [/^language-[a-z0-9_-]+$/i] },
    allowedSchemes: ['http', 'https', 'mailto'],
    allowProtocolRelative: false,
    transformTags: {
      a: (_tag, attrs) => ({ tagName: 'a', attribs: attrs.href?.startsWith('//') ? {} : attrs }),
    },
  });
}
