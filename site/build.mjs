// Build the Psyche manual using the Coven Threads documentation layout.
import { execFileSync } from 'node:child_process';
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, posix, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import MarkdownIt from 'markdown-it';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..');
const dist = join(here, 'dist');
const check = process.argv.includes('--check');

const REPO = 'OpenCoven/psyche';
const SITE_NAME = 'Psyche Docs';
const BASE_URL = (process.env.SITE_URL ?? 'https://psyche-docs.vercel.app').replace(/\/$/, '');

// ── Navigation ────────────────────────────────────────────────────────────
const groups = [
  { label: 'Start here', pages: ['overview'] },
  { label: 'Operate', pages: ['CLI', 'CONFIGURATION'] },
  { label: 'Develop', pages: ['ARCHITECTURE', 'SCHEMAS', 'TESTING'] },
];
const sourceLinks = [
  { name: 'Rust workspace', path: 'crates' },
  { name: 'Test fixtures', path: 'crates/psyche-core/tests/fixtures' },
];
const navNames = {
  overview: 'Overview', CLI: 'Command line', CONFIGURATION: 'Configuration',
  ARCHITECTURE: 'Architecture', SCHEMAS: 'Schemas', TESTING: 'Testing',
};
const leads = {
  overview: 'The Coven-native runtime and orchestration protocol for persistent AI identities.',
  CLI: 'Start the foreground daemon, check a local installation, and interpret its JSON output.',
  CONFIGURATION: 'Configure local storage, the Coven socket, and versioned extensions in psyche.toml.',
  ARCHITECTURE: 'Trace ownership of contracts, storage, adapter ports, and runtime startup across the Rust workspace.',
  SCHEMAS: 'Identify, validate, and persist records in the v1 contract registry.',
  TESTING: 'Exercise adapter contracts with deterministic fixtures, fault injection, and restart tests.',
};

function git(...args) {
  return execFileSync('git', ['-C', repoRoot, ...args], { encoding: 'utf8' }).trim();
}
// Pin source links to the latest runtime or manual change, excluding site-only commits.
const commit = git('log', '-1', '--format=%H', '--', '.', ':(exclude)site/**');
const shortCommit = commit.slice(0, 7);
const commitDate = git('show', '-s', '--format=%cs', commit);
const blobUrl = (path) => `https://github.com/${REPO}/blob/${commit}/${path}`;
const treeUrl = (path) => `https://github.com/${REPO}/tree/${commit}/${path}`;

const esc = (s) =>
  String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

// GitHub's heading-anchor algorithm, so in-repo fragment links keep working.
function makeSlugger() {
  const seen = new Map();
  return (text) => {
    const base = text
      .trim()
      .toLowerCase()
      .replace(/[^\p{L}\p{M}\p{N}\p{Pc}\- ]/gu, '')
      .replace(/ /g, '-');
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    return n === 0 ? base : `${base}-${n}`;
  };
}

const pageUrl = (id) => (id === 'overview' ? '/' : `/${id.toLowerCase()}/`);
const pageIds = groups.flatMap((g) => g.pages);
const problems = [];

// ── Markdown ──────────────────────────────────────────────────────────────
const md = new MarkdownIt({ html: false, linkify: true, typographer: false });
// Only explicit URLs autolink, as on GitHub. Fuzzy matching would turn
// `SOUL.md` and `MEMORY.md` into links to the .md top-level domain.
md.linkify.set({ fuzzyLink: false, fuzzyEmail: false, fuzzyIP: false });

const inlineText = (token) =>
  (token.children ?? [])
    .filter((t) => t.type === 'text' || t.type === 'code_inline')
    .map((t) => t.content)
    .join('');

// Resolve a Markdown href written relative to docs/ into a site or GitHub URL.
function resolveHref(href, env, { image = false } = {}) {
  if (/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(href)) return { url: href, external: true };
  if (href.startsWith('#')) {
    env.links.push({ page: env.id, hash: href.slice(1), raw: href });
    return { url: href };
  }
  const [rawPath, hash = ''] = href.split('#');
  const repoPath = posix.normalize(posix.join('docs', rawPath));
  if (repoPath.startsWith('..')) {
    problems.push(`${env.id}.md: link leaves the repository: ${href}`);
    return { url: href };
  }
  const match = /^docs\/([^/]+)\.md$/.exec(repoPath);
  if (match && pageIds.includes(match[1])) {
    if (hash) env.links.push({ page: match[1], hash, raw: href });
    return { url: pageUrl(match[1]) + (hash ? `#${hash}` : '') };
  }
  const abs = join(repoRoot, repoPath);
  if (!existsSync(abs)) {
    problems.push(`${env.id}.md: link target not in repository: ${href}`);
    return { url: blobUrl(repoPath), external: true };
  }
  if (image) {
    env.assets.add(repoPath);
    return { url: `/${repoPath.replace(/^docs\//, '')}` };
  }
  const url = statSync(abs).isDirectory() ? treeUrl(repoPath) : blobUrl(repoPath);
  return { url: url + (hash ? `#${hash}` : ''), external: true };
}

md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
  const token = tokens[idx];
  const { url, external } = resolveHref(token.attrGet('href') ?? '', env);
  token.attrSet('href', url);
  if (external) {
    token.attrSet('target', '_blank');
    token.attrSet('rel', 'noreferrer noopener');
  }
  env.linkDepth = (env.linkDepth ?? 0) + 1;
  return self.renderToken(tokens, idx, options);
};
md.renderer.rules.link_close = (tokens, idx, options, env, self) => {
  env.linkDepth -= 1;
  return self.renderToken(tokens, idx, options);
};

// GitHub renders `OpenCoven/repo#123` as a link; keep that affordance.
md.renderer.rules.text = (tokens, idx, options, env) => {
  const text = esc(tokens[idx].content);
  if (env.linkDepth > 0) return text;
  return text.replace(
    /\bOpenCoven\/([A-Za-z0-9._-]+)#(\d+)\b/g,
    (m, repo, n) =>
      `<a href="https://github.com/OpenCoven/${repo}/issues/${n}" target="_blank" rel="noreferrer noopener">${m}</a>`,
  );
};

md.renderer.rules.image = (tokens, idx, options, env) => {
  const token = tokens[idx];
  const { url } = resolveHref(token.attrGet('src') ?? '', env, { image: true });
  const alt = esc(token.content);
  return `<a class="ct-figure" href="${esc(url)}" target="_blank" rel="noreferrer noopener"><img src="${esc(url)}" alt="${alt}" loading="lazy" decoding="async"></a>`;
};

md.renderer.rules.heading_open = (tokens, idx, options, env) => {
  const token = tokens[idx];
  const text = inlineText(tokens[idx + 1]);
  const id = env.slug(text);
  const level = Number(token.tag.slice(1));
  env.anchors.add(id);
  if (level === 2 || level === 3) {
    env.toc.push({ id, level, html: md.renderInline(tokens[idx + 1].content, { ...env, linkDepth: 1, links: [] }) });
  }
  env.sections.push({ id, title: text, text: '' });
  return `<${token.tag} id="${esc(id)}"><a class="ct-anchor" href="#${esc(id)}">`;
};
md.renderer.rules.heading_close = (tokens, idx) => `</a></${tokens[idx].tag}>\n`;

md.renderer.rules.softbreak = () => ' ';
md.renderer.rules.code_block = (tokens, idx) => `<figure class="ct-code"><pre><code>${esc(tokens[idx].content)}</code></pre></figure>\n`;

md.renderer.rules.table_open = () => '<div class="ct-table"><table>\n';
md.renderer.rules.table_close = () => '</table></div>\n';

md.renderer.rules.fence = (tokens, idx) => {
  const token = tokens[idx];
  const lang = token.info.trim().split(/\s+/)[0];
  return `<figure class="ct-code"${lang ? ` data-lang="${esc(lang)}"` : ''}><pre><code>${esc(token.content)}</code></pre></figure>\n`;
};

// Collect running text per section for the search index.
md.core.ruler.push('ct_search_text', (state) => {
  const out = (state.env.sectionText = []);
  let current = { heading: null, text: [] };
  out.push(current);
  for (let i = 0; i < state.tokens.length; i++) {
    const token = state.tokens[i];
    if (token.type === 'heading_open') {
      current = { heading: inlineText(state.tokens[i + 1]), text: [] };
      out.push(current);
      i += 2;
    } else if (token.type === 'inline') {
      current.text.push(inlineText(token));
    }
  }
});

// ── Load pages ────────────────────────────────────────────────────────────
function readingMinutes(source) {
  const text = source
    .replace(/```[\s\S]*?```/g, '')
    .replace(/[#*`_>\-[\](){}!|]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  const words = text ? text.split(' ').length : 0;
  return Math.max(1, Math.round(words / 220));
}

const pages = new Map();
for (const group of groups) {
  for (const id of group.pages) {
    const source = id === 'overview'
      ? readFileSync(join(here, 'overview.md'), 'utf8')
      : git('show', `${commit}:docs/${id}.md`) + '\n';
    const h1 = /^# (.+)\n/.exec(source);
    if (!h1) throw new Error(`docs/${id}.md must open with an H1 title`);
    const env = {
      id,
      slug: makeSlugger(),
      anchors: new Set(),
      toc: [],
      sections: [],
      links: [],
      assets: new Set(),
    };
    env.slug(h1[1]); // the title keeps its slot in GitHub's anchor numbering
    const body = md.render(source.slice(h1[0].length), env);
    pages.set(id, {
      id,
      group: group.label,
      title: h1[1],
      navName: navNames[id] ?? h1[1],
      lead: leads[id],
      url: pageUrl(id),
      body,
      env,
      minutes: readingMinutes(source),
    });
  }
}

for (const page of pages.values()) {
  for (const link of page.env.links) {
    const target = pages.get(link.page);
    if (!target.env.anchors.has(decodeURIComponent(link.hash))) {
      problems.push(`${page.id}.md: fragment not found: ${link.raw}`);
    }
  }
}

// ── Icons (authored for this site; 24px grid, 1.5 stroke) ─────────────────
const icon = (body, size = 16, extra = '') =>
  `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"${extra}>${body}</svg>`;
const icons = {
  search: icon('<circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/>'),
  arrowOut: (s = 12) => icon('<path d="M7 17 17 7M9 7h8v8"/>', s),
  arrowLeft: icon('<path d="M19 12H5m6-6-6 6 6 6"/>', 12),
  arrowRight: icon('<path d="M5 12h14m-6-6 6 6-6 6"/>', 12),
  bug: icon(
    '<rect x="8" y="8" width="8" height="11" rx="4"/><path d="M12 12v7M5 13h3m8 0h3M6 7l2.5 2.5M18 7l-2.5 2.5M6 20l2.2-2.6M18 20l-2.2-2.6M9.5 5.5a2.5 2.5 0 0 1 5 0"/>',
    14,
  ),
  file: icon('<path d="M14 3H7a1 1 0 0 0-1 1v16a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1V7z"/><path d="M14 3v4h4M9 13h6M9 17h4"/>', 14),
  seal: icon('<path d="M12 3 5 6v5.5c0 4.2 2.9 7.6 7 9.5 4.1-1.9 7-5.3 7-9.5V6z"/><path d="m9 12 2.2 2.2L15.2 10"/>', 14),
  toc: icon('<path d="M4 6h16M4 12h10M4 18h13"/>', 14),
  menu: icon('<path d="M4 7h16M4 12h16M4 17h16"/>', 20),
  close: icon('<path d="m6 6 12 12M18 6 6 18"/>', 20),
  sun: icon('<circle cx="12" cy="12" r="4"/><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.4 1.4M17.6 17.6 19 19M5 19l1.4-1.4M17.6 6.4 19 5"/>'),
  moon: icon('<path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z"/>'),
  book: icon('<path d="M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2z"/><path d="M4 21a2 2 0 0 0 2 2h13v-4M9 8h6"/>', 18),
  github:
    '<svg width="18" height="18" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8"/></svg>',
};

// ── Templates ─────────────────────────────────────────────────────────────
const assetVersion = shortCommit;
const brand = `<a class="coven-docs-brand" href="/" aria-label="${SITE_NAME} overview">
  <img class="coven-docs-brand-logo" src="/assets/opencoven-logo.svg" alt="" width="24" height="24">
  <span class="coven-docs-brand-product">Psyche</span>
  <span class="coven-docs-brand-label">Docs</span>
</a>`;

function sidebar(current) {
  const tree = groups
    .map(
      (group) =>
        `<p>${esc(group.label)}</p>\n` +
        group.pages
          .map((id) => {
            const page = pages.get(id);
            const active = id === current.id;
            return `<a href="${page.url}" data-active="${active}"${active ? ' aria-current="page"' : ''}>${esc(page.navName)}</a>`;
          })
          .join('\n'),
    )
    .join('\n');
  const source =
    `<p>Source</p>\n` +
    sourceLinks
      .map(
        (link) =>
          `<a href="${treeUrl(link.path)}" target="_blank" rel="noreferrer noopener" data-active="false">${esc(link.name)}<span class="ct-out">${icons.arrowOut(11)}</span></a>`,
      )
      .join('\n');
  return `<aside id="ct-sidebar" aria-label="Documentation">
  <div class="ct-sidebar-head">
    ${brand}
    <button type="button" class="ct-search-toggle" data-search-open>
      ${icons.search}<span>Search</span><span class="ct-kbd"><kbd data-mod>⌘</kbd><kbd>K</kbd></span>
    </button>
  </div>
  <nav class="ct-tree" aria-label="Pages">
${tree}
${source}
  </nav>
  <div class="ct-sidebar-foot">
    <a href="https://docs.opencoven.ai" target="_blank" rel="noreferrer noopener" aria-label="Coven Docs" title="Coven Docs">${icons.book}</a>
    <a href="https://github.com/${REPO}" target="_blank" rel="noreferrer noopener" aria-label="GitHub" title="GitHub">${icons.github}</a>
    <button type="button" class="ct-theme" data-theme-toggle aria-label="Toggle theme">
      <span data-theme-icon="light">${icons.sun}</span><span data-theme-icon="dark">${icons.moon}</span>
    </button>
  </div>
</aside>`;
}

function toc(page) {
  if (page.env.toc.length === 0) return '';
  const items = page.env.toc
    .map((item) => `<a href="#${esc(item.id)}" data-level="${item.level}">${item.html}</a>`)
    .join('\n');
  return `<aside id="ct-toc" aria-label="On this page">
  <p class="ct-toc-title">${icons.toc}On this page</p>
  <nav>
${items}
  </nav>
</aside>`;
}

function footerNav(page) {
  const index = pageIds.indexOf(page.id);
  const previous = pages.get(pageIds[index - 1]);
  const next = pages.get(pageIds[index + 1]);
  const card = (target, label, cls, arrow) =>
    target
      ? `<a class="ct-pager-card ${cls}" href="${target.url}"><span class="ct-pager-label">${arrow}${label}</span><span class="ct-pager-name">${esc(target.navName)}</span></a>`
      : '<div></div>';
  return `<nav class="ct-pager" aria-label="Page navigation">
${card(previous, 'Previous', 'is-prev', icons.arrowLeft)}
${card(next, 'Next', 'is-next', icons.arrowRight)}
</nav>`;
}

function render(page) {
  const sourcePath = page.id === 'overview' ? 'docs' : `docs/${page.id}.md`;
  const sourceUrl = page.id === 'overview' ? treeUrl('docs') : blobUrl(sourcePath);
  const canonical = BASE_URL ? `${BASE_URL}${page.url}` : '';
  const title = page.id === 'overview' ? SITE_NAME : `${page.navName} | ${SITE_NAME}`;
  const description = md
    .renderInline(page.lead, { id: page.id, linkDepth: 1, links: [], assets: new Set() })
    .replace(/<[^>]+>/g, '');
  const issueUrl = `https://github.com/${REPO}/issues/new?${new URLSearchParams({
    title: `Docs: ${page.navName}`,
    body: `- Page: ${page.navName}\n- Source: \`${sourcePath}\` at \`${shortCommit}\`\n\n### What is the issue?\n`,
  })}`;
  const lead = md.renderInline(page.lead, { id: page.id, links: page.env.links, assets: page.env.assets });

  return `<!doctype html>
<html lang="en" class="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)}</title>
<meta name="description" content="${esc(description)}">
${canonical ? `<link rel="canonical" href="${esc(canonical)}">\n` : ''}<meta property="og:type" content="article">
<meta property="og:site_name" content="${SITE_NAME}">
<meta property="og:title" content="${esc(page.navName)}">
<meta property="og:description" content="${esc(description)}">
<meta name="twitter:card" content="summary">
<meta name="color-scheme" content="dark light">
<link rel="icon" href="/assets/favicon.svg" type="image/svg+xml">
<link rel="apple-touch-icon" href="/assets/apple-touch-icon.png">
<link rel="preload" href="/assets/fonts/inter-normal-latin.woff2" as="font" type="font/woff2" crossorigin>
<link rel="preload" href="/assets/fonts/eb-garamond-normal-latin.woff2" as="font" type="font/woff2" crossorigin>
<link rel="stylesheet" href="/assets/style.css?v=${assetVersion}">
<script>try{var t=localStorage.getItem('ct-theme');if(t==='light'||(!t&&matchMedia('(prefers-color-scheme: light)').matches))document.documentElement.className='light'}catch(e){}</script>
</head>
<body>
<a class="ct-skip" href="#nd-page">Skip to content</a>
<header class="ct-topbar">
  ${brand}
  <div class="ct-topbar-actions">
    <button type="button" data-search-open aria-label="Search">${icons.search.replace('width="16" height="16"', 'width="20" height="20"')}</button>
    <button type="button" data-menu-toggle aria-label="Open navigation" aria-expanded="false" aria-controls="ct-sidebar">${icons.menu}</button>
  </div>
</header>
<div id="ct-layout">
${sidebar(page)}
<div class="ct-scrim" data-menu-close></div>
<main>
<article id="nd-page">
  <nav class="ct-breadcrumb" aria-label="Breadcrumb"><span>${esc(page.group)}</span><span aria-hidden="true">/</span><span aria-current="page">${esc(page.navName)}</span></nav>
  <header class="coven-docs-page-header">
    <div class="coven-docs-page-meta">
      <div class="coven-docs-status" data-docs-stability="descriptive">
        <span class="coven-docs-status-badge" title="Documentation for the linked repository revision.">${icons.seal}Descriptive</span>
        <a class="coven-docs-status-source" href="https://github.com/${REPO}" target="_blank" rel="noreferrer noopener"><span class="coven-docs-status-label">Contract</span>${REPO}${icons.arrowOut()}</a>
      </div>
      <div class="coven-docs-page-toolbar">
        <a href="${esc(issueUrl)}" target="_blank" rel="noreferrer noopener" aria-label="Report an issue with this page on GitHub">${icons.bug}Report issue</a>
        <a href="${sourceUrl}" target="_blank" rel="noreferrer noopener" aria-label="View the documentation source on GitHub">${icons.file}Source</a>
      </div>
    </div>
    <h1 id="${makeSlugger()(page.title)}">${esc(page.title)}</h1>
    ${lead ? `<p>${lead}</p>` : ''}
    <div class="coven-docs-seal-rule">
      <span class="coven-docs-seal" aria-hidden="true"></span>
      <span class="coven-docs-seal-line" aria-hidden="true"></span>
      <p class="coven-docs-reading-time">${page.minutes} min read</p>
    </div>
  </header>
  <div class="prose">
${page.body}
  </div>
  ${footerNav(page)}
  <footer class="ct-colophon">
    <p>${page.id === 'overview' ? 'Based on' : 'Rendered from'} <a href="${sourceUrl}"><code>${sourcePath}</code></a> at <a href="https://github.com/${REPO}/commit/${commit}"><code>${shortCommit}</code></a> (${commitDate}).</p>
  </footer>
</article>
</main>
${toc(page)}
</div>
<dialog id="ct-search" aria-label="Search the documentation">
  <div class="ct-search-field">${icons.search}<input type="search" placeholder="Search the Psyche docs" autocomplete="off" spellcheck="false" aria-label="Search"><kbd>Esc</kbd></div>
  <div class="ct-search-results" role="listbox" aria-label="Results"></div>
</dialog>
<script src="/assets/app.js?v=${assetVersion}" defer></script>
</body>
</html>
`;
}

// ── Write ─────────────────────────────────────────────────────────────────
rmSync(dist, { recursive: true, force: true });
mkdirSync(dist, { recursive: true });
cpSync(join(here, 'assets'), join(dist, 'assets'), { recursive: true });

const searchIndex = [];
for (const page of pages.values()) {
  const out = page.id === 'overview' ? dist : join(dist, page.id.toLowerCase());
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'index.html'), render(page));

  const slug = makeSlugger();
  slug(page.title);
  for (const section of page.env.sectionText) {
    const id = section.heading === null ? '' : slug(section.heading);
    searchIndex.push({
      p: page.navName,
      h: section.heading ?? '',
      u: page.url + (id ? `#${id}` : ''),
      t: section.text.join(' ').replace(/\s+/g, ' ').slice(0, 1200),
    });
  }
  for (const asset of page.env.assets) {
    const target = join(dist, asset.replace(/^docs\//, ''));
    mkdirSync(dirname(target), { recursive: true });
    cpSync(join(repoRoot, asset), target);
  }
}
writeFileSync(join(dist, 'search.json'), JSON.stringify(searchIndex));

const notFound = { ...pages.get('overview') };
writeFileSync(
  join(dist, '404.html'),
  render({
    ...notFound,
    id: 'overview',
    title: 'Page not found',
    navName: 'Not found',
    group: 'Psyche',
    lead: 'This address is not part of the Psyche docs. Use the navigation, or start from the overview.',
    body: '<p><a href="/">Return to the overview</a></p>',
    env: { ...notFound.env, toc: [] },
    minutes: 1,
  }).replace('<p class="coven-docs-reading-time">1 min read</p>', ''),
);

writeFileSync(join(dist, 'build.txt'), `${commit}\n`);

console.log(`built ${pages.size} pages from ${REPO}@${shortCommit} -> ${dist}`);
if (problems.length > 0) {
  console.log(`\n${problems.length} link problem(s) in the source Markdown:`);
  for (const problem of problems) console.log(`  - ${problem}`);
  if (check) process.exit(1);
}
