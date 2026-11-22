// Psyche Docs: theme, navigation drawer, contents scroll-spy, search.
// No dependencies. Every page reads and links without this file.
(() => {
  const root = document.documentElement;

  // ── Theme ──
  const toggle = document.querySelector('[data-theme-toggle]');
  toggle?.addEventListener('click', () => {
    const next = root.classList.contains('dark') ? 'light' : 'dark';
    root.className = next;
    try {
      localStorage.setItem('ct-theme', next);
    } catch {}
  });

  // ── Navigation drawer (small screens) ──
  const menuButton = document.querySelector('[data-menu-toggle]');
  const setMenu = (open) => {
    if (open) root.dataset.menu = 'open';
    else delete root.dataset.menu;
    menuButton?.setAttribute('aria-expanded', String(open));
  };
  menuButton?.addEventListener('click', () => setMenu(root.dataset.menu !== 'open'));
  document.querySelector('[data-menu-close]')?.addEventListener('click', () => setMenu(false));

  // ── Contents scroll-spy ──
  const tocLinks = [...document.querySelectorAll('#ct-toc a')];
  if (tocLinks.length > 0) {
    const targets = tocLinks
      .map((link) => document.getElementById(decodeURIComponent(link.hash.slice(1))))
      .filter(Boolean);
    let frame = 0;
    const update = () => {
      frame = 0;
      let current = targets[0];
      for (const target of targets) {
        if (target.getBoundingClientRect().top <= 120) current = target;
        else break;
      }
      const atEnd = window.innerHeight + window.scrollY >= document.body.scrollHeight - 4;
      if (atEnd) current = targets[targets.length - 1];
      for (const link of tocLinks) {
        const active = decodeURIComponent(link.hash.slice(1)) === current?.id;
        link.dataset.active = String(active);
        if (active) link.scrollIntoView({ block: 'nearest' });
      }
    };
    window.addEventListener(
      'scroll',
      () => {
        frame ||= requestAnimationFrame(update);
      },
      { passive: true },
    );
    update();
  }

  // ── Search ──
  const dialog = document.getElementById('ct-search');
  const input = dialog?.querySelector('input');
  const results = dialog?.querySelector('.ct-search-results');
  if (!dialog || !input || !results) return;

  if (!/Mac|iPhone|iPad/.test(navigator.platform)) {
    for (const key of document.querySelectorAll('[data-mod]')) key.textContent = 'Ctrl';
  }

  let index = null;
  let selected = 0;
  const load = async () => {
    index ??= await fetch('/search.json').then((response) => response.json());
    return index;
  };

  const escapeHtml = (text) =>
    text.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);

  const excerpt = (text, terms) => {
    const lower = text.toLowerCase();
    const at = Math.max(0, ...terms.map((term) => lower.indexOf(term)).filter((i) => i >= 0).slice(0, 1));
    const start = Math.max(0, at - 40);
    let out = escapeHtml((start > 0 ? '…' : '') + text.slice(start, start + 160));
    for (const term of terms) {
      const pattern = new RegExp(escapeHtml(term).replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi');
      out = out.replace(pattern, (match) => `<mark>${match}</mark>`);
    }
    return out;
  };

  const select = (next) => {
    const items = [...results.querySelectorAll('a')];
    if (items.length === 0) return;
    selected = (next + items.length) % items.length;
    items.forEach((item, i) => item.setAttribute('aria-selected', String(i === selected)));
    items[selected].scrollIntoView({ block: 'nearest' });
  };

  const search = async () => {
    const terms = input.value.toLowerCase().split(/\s+/).filter(Boolean);
    if (terms.length === 0) {
      results.replaceChildren();
      return;
    }
    const entries = await load();
    const hits = [];
    for (const entry of entries) {
      const heading = (entry.h || entry.p).toLowerCase();
      const body = entry.t.toLowerCase();
      let score = 0;
      for (const term of terms) {
        if (heading.includes(term)) score += heading.startsWith(term) ? 12 : 8;
        else if (entry.p.toLowerCase().includes(term)) score += 3;
        else if (body.includes(term)) score += 1;
        else {
          score = 0;
          break;
        }
      }
      if (score > 0) hits.push({ entry, score });
    }
    hits.sort((a, b) => b.score - a.score);
    if (hits.length === 0) {
      results.innerHTML = '<p class="ct-search-empty">Nothing in the Psyche docs matches that.</p>';
      return;
    }
    results.innerHTML = hits
      .slice(0, 24)
      .map(
        ({ entry }) =>
          `<a href="${entry.u}" role="option"><span class="ct-search-hit-meta">${escapeHtml(entry.p)}</span>` +
          `<span class="ct-search-hit-title">${escapeHtml(entry.h || entry.p)}</span>` +
          (entry.t ? `<span class="ct-search-hit-text">${excerpt(entry.t, terms)}</span>` : '') +
          '</a>',
      )
      .join('');
    select(0);
  };

  const open = () => {
    setMenu(false);
    if (!dialog.open) dialog.showModal();
    input.select();
    load();
  };

  for (const button of document.querySelectorAll('[data-search-open]')) {
    button.addEventListener('click', open);
  }
  document.addEventListener('keydown', (event) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      dialog.open ? dialog.close() : open();
    } else if (event.key === 'Escape' && root.dataset.menu === 'open') {
      setMenu(false);
    }
  });
  input.addEventListener('input', search);
  input.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      select(selected + (event.key === 'ArrowDown' ? 1 : -1));
    } else if (event.key === 'Enter') {
      const target = results.querySelector('a[aria-selected="true"]');
      if (target) {
        event.preventDefault();
        dialog.close();
        location.assign(target.href);
      }
    }
  });
  // Click on the backdrop closes; a click on a result closes before navigating
  // so a same-page fragment link does not leave the dialog open.
  dialog.addEventListener('click', (event) => {
    if (event.target === dialog || event.target.closest('a')) dialog.close();
  });
})();
