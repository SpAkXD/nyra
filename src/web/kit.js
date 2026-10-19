/*! Nyra web kit: the behaviors of `use web` (docs/WEB.md). The build includes the chunks a page
    uses; a chunk runs from its marker to the next one. Every page works without this file: these
    only enhance what the HTML already does. Clicks are delegated to the document, so a region the
    page renders again keeps working. */

/*! chunk: nav */
(() => {
  const nav = document.querySelector('.nav');
  if (!nav) return;
  const bar = nav.querySelector('.nav-progress');
  let queued = false;
  const onScroll = () => {
    queued = false;
    const y = window.scrollY;
    nav.classList.toggle('is-scrolled', y > 40);
    if (bar) {
      const max = document.documentElement.scrollHeight - window.innerHeight;
      bar.style.setProperty('--progress', max > 0 ? Math.min(y / max, 1).toFixed(4) : '0');
    }
  };
  window.addEventListener('scroll', () => {
    if (!queued) {
      queued = true;
      requestAnimationFrame(onScroll);
    }
  }, { passive: true });
  onScroll();

  // The mobile menu is a <details>: it opens without JavaScript. Here it also closes on a link,
  // on Escape, on a click outside and when the screen gets wide, and its label follows its state.
  const more = nav.querySelector('.nav-more');
  if (more) {
    const toggle = more.querySelector('.nav-toggle');
    more.addEventListener('toggle', () => {
      toggle.setAttribute('aria-label', more.open ? toggle.dataset.close : toggle.dataset.open);
      toggle.setAttribute('aria-expanded', String(more.open));
    });
    toggle.setAttribute('aria-expanded', 'false');
    more.addEventListener('click', (e) => {
      if (e.target.closest('.nav-menu a')) more.open = false;
    });
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape' && more.open) {
        more.open = false;
        toggle.focus();
      }
    });
    document.addEventListener('click', (e) => {
      if (more.open && !nav.contains(e.target)) more.open = false;
    });
    window.matchMedia('(min-width: 1024px)').addEventListener('change', (m) => {
      if (m.matches) more.open = false;
    });
  }

  // Scroll-spy: the section that crosses the upper third of the screen marks its link.
  const links = [...nav.querySelectorAll('.nav-links a, .nav-menu ul a')].filter((a) => /^#./.test(a.getAttribute('href')));
  const sections = [...new Set(links.map((a) => a.getAttribute('href').slice(1)))]
    .map((id) => document.getElementById(id))
    .filter(Boolean);
  if (!sections.length || !('IntersectionObserver' in window)) return;
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (!e.isIntersecting) continue;
      for (const a of links) {
        if (a.getAttribute('href') === '#' + e.target.id) a.setAttribute('aria-current', 'true');
        else a.removeAttribute('aria-current');
      }
    }
  }, { rootMargin: '-35% 0px -60% 0px' });
  sections.forEach((s) => io.observe(s));
})();

/*! chunk: hours */
(() => {
  // A badge's `data-hours` is "1-5 8:00-16:00|6 8:30-11:30": days (1 = Monday ... 7 = Sunday),
  // then one or more time ranges. `data-tz` is the site's time zone.
  const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  const now = (tz) => {
    try {
      const parts = new Intl.DateTimeFormat('en-GB', {
        timeZone: tz || undefined, weekday: 'short', hour: '2-digit', minute: '2-digit', hourCycle: 'h23',
      }).formatToParts(new Date());
      const get = (t) => (parts.find((p) => p.type === t) || {}).value;
      const d = DAYS.indexOf(get('weekday'));
      return { day: d === 0 ? 7 : d, min: Number(get('hour')) * 60 + Number(get('minute')) };
    } catch {
      const d = new Date();
      return { day: d.getDay() || 7, min: d.getHours() * 60 + d.getMinutes() };
    }
  };
  const days = (spec) => {
    const out = new Set();
    for (const part of spec.split(',')) {
      const [a, b] = part.split('-').map(Number);
      if (Number.isNaN(a)) continue;
      for (let d = a; d <= (Number.isNaN(b) || b === undefined ? a : b); d++) out.add(d === 0 ? 7 : d);
    }
    return out;
  };
  const mins = (t) => {
    const [h, m] = t.split(':').map(Number);
    return h * 60 + (m || 0);
  };
  const inRange = (r, m) => {
    const [a, b] = r.split('-');
    if (!a || !b) return false;
    const from = mins(a);
    const to = mins(b);
    return to > from ? m >= from && m < to : m >= from || m < to;
  };
  const openOn = (row, t) => {
    const [d, ...ranges] = row.trim().split(/\s+/);
    return days(d).has(t.day) && ranges.some((r) => inRange(r, t.min));
  };
  const update = () => {
    for (const b of document.querySelectorAll('.badge[data-hours]')) {
      const t = now(b.dataset.tz);
      const open = b.dataset.hours.split('|').some((row) => row && openOn(row, t));
      b.dataset.state = open ? 'open' : 'closed';
      const label = b.querySelector('.badge-label');
      if (label) label.textContent = open ? b.dataset.open : b.dataset.closed;
    }
    for (const card of document.querySelectorAll('.hours')) {
      const badge = card.querySelector('.badge');
      const t = now(badge && badge.dataset.tz);
      for (const tr of card.querySelectorAll('tr[data-days]')) tr.classList.toggle('is-today', days(tr.dataset.days).has(t.day));
    }
  };
  update();
  setInterval(update, 60000);
})();

/*! chunk: map */
(() => {
  // The map loads only when asked for: no request to Google before a click.
  document.addEventListener('click', (e) => {
    const link = e.target.closest('.map-load');
    const frame = link && link.closest('.map-frame');
    if (!frame || !frame.dataset.src || e.metaKey || e.ctrlKey) return;
    e.preventDefault();
    const f = document.createElement('iframe');
    f.src = frame.dataset.src;
    f.title = frame.dataset.title || '';
    f.loading = 'lazy';
    f.referrerPolicy = 'no-referrer-when-downgrade';
    f.allowFullscreen = true;
    frame.replaceChildren(f);
  });
})();

/*! chunk: form */
(() => {
  // A `mailto:` form composes the e-mail itself: the message, a line, then the other fields.
  document.addEventListener('submit', (e) => {
    const form = e.target.closest && e.target.closest('.form-body[data-mailto]');
    if (!form) return;
    e.preventDefault();
    if (!form.reportValidity()) return;
    const fields = [...form.elements].filter((el) => el.name && !['submit', 'button'].includes(el.type));
    const message = fields.find((el) => el.tagName === 'TEXTAREA');
    const label = (el) => {
      const l = el.closest('label') || form.querySelector(`label[for="${el.id}"]`);
      return (l ? l.textContent : el.name).replace('*', '').trim();
    };
    const lines = fields
      .filter((el) => el !== message && (el.type === 'checkbox' ? el.checked : el.value.trim()))
      .map((el) => (el.type === 'checkbox' ? label(el) : el.value.trim()));
    const body = (message ? message.value.trim() + '\n\n—\n' : '') + lines.join('\n');
    const subject = form.dataset.subject ? 'subject=' + encodeURIComponent(form.dataset.subject) + '&' : '';
    window.location.href = 'mailto:' + form.dataset.mailto + '?' + subject + 'body=' + encodeURIComponent(body);
    const card = form.closest('.form');
    const done = card && card.querySelector('.form-done');
    if (done) {
      form.hidden = true;
      done.hidden = false;
      const edit = done.querySelector('.form-edit');
      if (edit) edit.focus();
    }
  });
  document.addEventListener('click', (e) => {
    const edit = e.target.closest('.form-edit');
    const card = edit && edit.closest('.form');
    if (!card) return;
    card.querySelector('.form-body').hidden = false;
    card.querySelector('.form-done').hidden = true;
    const first = card.querySelector('input, textarea, select');
    if (first) first.focus();
  });
})();

/*! chunk: gallery */
(() => {
  const ICON = {
    x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
    prev: '<path d="m15 18-6-6 6-6"/>',
    next: '<path d="m9 18 6-6-6-6"/>',
  };
  const svg = (k) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${ICON[k]}</svg>`;
  for (const tabs of document.querySelectorAll('.gallery .tabs')) tabs.hidden = false;

  const filter = (tab) => {
    const g = tab.closest('.gallery');
    const f = tab.dataset.filter;
    for (const t of g.querySelectorAll('.tabs [data-filter]')) {
      t.setAttribute('aria-selected', String(t === tab));
      t.tabIndex = t === tab ? 0 : -1;
    }
    for (const it of g.querySelectorAll('.gallery-item')) it.hidden = Boolean(f) && it.dataset.tag !== f;
  };
  for (const t of document.querySelectorAll('.gallery .tabs [data-filter]')) t.tabIndex = t.getAttribute('aria-selected') === 'true' ? 0 : -1;

  // The lightbox: a modal <dialog> (it keeps the focus inside and closes on Escape), arrows,
  // swipes and a counter.
  let box = null;
  let items = [];
  let index = 0;
  let opener = null;
  const show = (dir) => {
    const it = items[index];
    const img = it.querySelector('img');
    const big = document.createElement('img');
    big.src = it.getAttribute('href');
    big.alt = img ? img.alt : '';
    big.style.setProperty('--dir', String(dir));
    box.querySelector('.lb-stage').replaceChildren(big);
    box.querySelector('.lb-text').textContent = big.alt;
    box.querySelector('.lb-count').textContent = `${index + 1} / ${items.length}`;
    box.setAttribute('aria-label', big.alt);
  };
  const step = (d) => {
    index = (index + d + items.length) % items.length;
    show(d);
  };
  const build = (g) => {
    box = document.createElement('dialog');
    box.className = 'lightbox';
    box.innerHTML = '<div class="lb-stage"></div><p class="lb-cap"><span class="lb-text"></span><span class="lb-count"></span></p>'
      + `<button type="button" class="lb-btn lb-close">${svg('x')}</button>`
      + `<button type="button" class="lb-btn lb-prev">${svg('prev')}</button>`
      + `<button type="button" class="lb-btn lb-next">${svg('next')}</button>`;
    box.querySelector('.lb-close').setAttribute('aria-label', g.dataset.dismiss || 'Close');
    box.querySelector('.lb-prev').setAttribute('aria-label', g.dataset.prev || 'Previous');
    box.querySelector('.lb-next').setAttribute('aria-label', g.dataset.next || 'Next');
    box.addEventListener('click', (e) => {
      if (e.target.closest('.lb-close')) box.close();
      else if (e.target.closest('.lb-prev')) step(-1);
      else if (e.target.closest('.lb-next')) step(1);
      else if (!e.target.closest('img, .lb-cap')) box.close();
    });
    box.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowRight') step(1);
      if (e.key === 'ArrowLeft') step(-1);
    });
    let x0 = null;
    box.addEventListener('touchstart', (e) => { x0 = e.touches[0].clientX; }, { passive: true });
    box.addEventListener('touchend', (e) => {
      if (x0 === null) return;
      const dx = e.changedTouches[0].clientX - x0;
      x0 = null;
      if (Math.abs(dx) > 50) step(dx < 0 ? 1 : -1);
    });
    box.addEventListener('close', () => {
      document.documentElement.style.overflow = '';
      if (opener) opener.focus();
    });
    document.body.append(box);
  };
  const open = (item) => {
    const g = item.closest('.gallery');
    items = [...g.querySelectorAll('.gallery-item:not([hidden])')];
    index = items.indexOf(item);
    opener = item;
    if (!box) build(g);
    show(0);
    if (box.showModal) box.showModal();
    else box.setAttribute('open', '');
    document.documentElement.style.overflow = 'hidden';
    box.querySelector('.lb-close').focus();
  };

  document.addEventListener('click', (e) => {
    const tab = e.target.closest('.gallery .tabs [data-filter]');
    if (tab) {
      filter(tab);
      return;
    }
    const item = e.target.closest('.gallery-item');
    if (item && !e.metaKey && !e.ctrlKey && !e.shiftKey && e.button === 0) {
      e.preventDefault();
      open(item);
    }
  });
  document.addEventListener('keydown', (e) => {
    const tab = e.target.closest && e.target.closest('.gallery .tabs [data-filter]');
    if (!tab || (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft')) return;
    const all = [...tab.parentElement.querySelectorAll('[data-filter]')];
    const next = all[(all.indexOf(tab) + (e.key === 'ArrowRight' ? 1 : -1) + all.length) % all.length];
    next.focus();
    filter(next);
  });
})();

/*! chunk: tabs */
(() => {
  // Without this script the labels are links to the panels, which all show. With it: a tab list,
  // one panel at a time, arrow keys, and the panel a link (#id) names opens first.
  const select = (box, tab, focus) => {
    for (const t of box.querySelectorAll(':scope > .tabs > a')) {
      const on = t === tab;
      t.setAttribute('aria-selected', String(on));
      t.tabIndex = on ? 0 : -1;
      const p = document.getElementById(t.getAttribute('href').slice(1));
      if (p) p.hidden = !on;
    }
    if (focus) tab.focus();
  };
  for (const box of document.querySelectorAll('.tabbed')) {
    const nav = box.querySelector(':scope > .tabs');
    const tabs = nav ? [...nav.querySelectorAll(':scope > a')] : [];
    if (!tabs.length) continue;
    nav.setAttribute('role', 'tablist');
    for (const t of tabs) {
      const id = t.getAttribute('href').slice(1);
      t.setAttribute('role', 'tab');
      t.id = id + '-tab';
      t.setAttribute('aria-controls', id);
      t.removeAttribute('aria-current');
      const p = document.getElementById(id);
      if (p) {
        p.setAttribute('role', 'tabpanel');
        p.setAttribute('aria-labelledby', t.id);
        p.tabIndex = 0;
      }
    }
    box.dataset.ready = '';
    select(box, tabs.find((t) => t.getAttribute('href') === location.hash) || tabs[0], false);
  }
  document.addEventListener('click', (e) => {
    const t = e.target.closest('.tabbed[data-ready] > .tabs > a');
    if (!t) return;
    e.preventDefault();
    select(t.closest('.tabbed'), t, false);
  });
  document.addEventListener('keydown', (e) => {
    const t = e.target.closest && e.target.closest('.tabbed[data-ready] > .tabs > a');
    if (!t || !['ArrowRight', 'ArrowLeft', 'Home', 'End'].includes(e.key)) return;
    e.preventDefault();
    const all = [...t.parentElement.children];
    const i = all.indexOf(t);
    const step = e.key === 'ArrowRight' ? 1 : -1;
    const next = e.key === 'Home' ? all[0] : e.key === 'End' ? all[all.length - 1] : all[(i + step + all.length) % all.length];
    select(t.closest('.tabbed'), next, true);
  });
})();

/*! chunk: app */
(() => {
  // The drawer of an app layout on phones is a checkbox: here it also closes on a link and on Escape.
  document.addEventListener('click', (e) => {
    const a = e.target.closest('.app-side a');
    const cb = a && a.closest('.app').querySelector('.app-cb');
    if (cb) cb.checked = false;
  });
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    for (const cb of document.querySelectorAll('.app-cb:checked')) {
      cb.checked = false;
      cb.focus();
    }
  });
})();

/*! chunk: video */
(() => {
  // A YouTube or Vimeo video loads its player only when asked for (a click), then plays; before
  // that the page makes no request to the video site but the poster.
  document.addEventListener('click', (e) => {
    const link = e.target.closest('[data-embed]');
    if (!link || e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
    const box = link.closest('.video, .media');
    if (!box) return;
    e.preventDefault();
    const f = document.createElement('iframe');
    f.src = link.dataset.embed;
    f.title = link.dataset.title || link.getAttribute('aria-label') || link.textContent.trim() || 'Video';
    f.allow = 'autoplay; encrypted-media; picture-in-picture; fullscreen';
    f.allowFullscreen = true;
    f.referrerPolicy = 'strict-origin-when-cross-origin';
    if (box.classList.contains('video')) {
      box.querySelector('.video-load').replaceWith(f);
    } else {
      const frame = document.createElement('div');
      frame.className = 'media-thumb';
      frame.append(f);
      const thumb = box.querySelector('.media-thumb');
      if (thumb) thumb.replaceWith(frame);
      else box.prepend(frame);
    }
    f.focus();
  });
})();

/*! chunk: search */
(() => {
  // A search box filters the cards (or list items, rows) of the element after it as one types.
  const ITEMS = '.card, .media, .quote, .gallery-item, li, tr';
  const fold = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
  const run = (box) => {
    const target = box.nextElementSibling;
    if (!target) return;
    const q = fold(box.querySelector('input').value.trim());
    const items = [...target.querySelectorAll(ITEMS)].filter((el) => {
      const up = el.parentElement.closest(ITEMS);
      return !up || !target.contains(up);
    });
    let shown = 0;
    for (const it of items) {
      const hit = !q || fold(it.textContent).includes(q);
      it.hidden = !hit;
      if (hit) shown++;
    }
    box.querySelector('.finder-count').textContent = q ? (shown ? shown + ' / ' + items.length : box.dataset.none) : '';
  };
  for (const box of document.querySelectorAll('.finder')) {
    box.hidden = false;
    const target = box.nextElementSibling;
    // a live region drawn again keeps the filter
    if (target && 'MutationObserver' in window) new MutationObserver(() => run(box)).observe(target, { childList: true, subtree: true });
  }
  document.addEventListener('input', (e) => {
    const box = e.target.closest && e.target.closest('.finder');
    if (box) run(box);
  });
})();
