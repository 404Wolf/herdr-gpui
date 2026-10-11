// Page behaviour for the Herdr GPUI site. Runs at the end of <body>, before
// kit.js mounts the [data-hm-app] renderings on DOMContentLoaded.
(function () {
  // `?draft` shows placeholders for the film and screenshots that have not been
  // captured yet, so the layout can be reviewed before the media exists.
  const DRAFT = new URLSearchParams(location.search).has('draft');
  const NARROW = matchMedia('(max-width: 640px)');
  const NARROW_SCALE = 0.68;

  // ---- Theme toggle: system → light → dark. The <head> script applies a
  // stored choice before first paint; "system" stores nothing. ----
  (function () {
    const btn = document.getElementById('theme');
    const meta = document.querySelector('meta[name="theme-color"]');
    const root = document.documentElement;
    const media = matchMedia('(prefers-color-scheme: light)');
    const ORDER = ['system', 'light', 'dark'];
    const sync = () => {
      const mode = root.dataset.theme || 'system';
      btn.querySelectorAll('svg').forEach(s => { s.hidden = s.dataset.t !== mode; });
      btn.title = btn.ariaLabel = 'Theme: ' + mode;
      meta.content = getComputedStyle(root).getPropertyValue('--g0').trim();
    };
    btn.addEventListener('click', () => {
      const next = ORDER[(ORDER.indexOf(root.dataset.theme || 'system') + 1) % ORDER.length];
      if (next === 'system') delete root.dataset.theme; else root.dataset.theme = next;
      try { if (next === 'system') localStorage.removeItem('theme'); else localStorage.setItem('theme', next); } catch (e) {}
      sync();
    });
    media.addEventListener('change', sync);
    sync();
  })();

  // ---- TUI rendering: a character grid with coloured regions ----
  (function () {
    const W = 150, H = 26;
    const g = Array.from({ length: H }, () => Array.from({ length: W }, () => [' ', '']));
    const put = (x, y, s, c = '') => { for (let i = 0; i < s.length && x + i < W; i++) g[y][x + i] = [s[i], c]; };
    const box = (x, y, w, h, title, c = 't-b') => {
      put(x, y, '┌' + '─'.repeat(w - 2) + '┐', c); put(x, y + h - 1, '└' + '─'.repeat(w - 2) + '┘', c);
      for (let r = 1; r < h - 1; r++) { put(x, y + r, '│', c); put(x + w - 1, y + r, '│', c); }
      if (title) put(x + 2, y, ' ' + title + ' ', 't-h');
    };
    box(0, 0, 34, H - 1, 'herdr · default');
    put(2, 2, 'LOCAL', 't-d');
    put(2, 3, '▾ herdr-gpui', 't-h');
    put(4, 4, '● sidebar-polish', 't-sel'); g[4][4] = ['●', 't-w t-sel'];
    put(4, 5, '● agent/412-ports'); g[5][4] = ['●', 't-r'];
    put(4, 6, '● windows-ci'); g[6][4] = ['●', 't-g'];
    put(2, 8, 'BUILD-BOX · SSH', 't-d');
    put(2, 9, '▸ arbor', 't-h'); put(24, 9, '2 agents', 't-d');
    put(2, 10, '▸ moltis', 't-h'); put(26, 10, 'idle', 't-d');
    put(2, 13, 'AGENTS', 't-d');
    put(2, 15, '●', 't-w'); put(4, 15, 'claude    working');
    put(2, 16, '●', 't-r'); put(4, 16, 'codex     blocked');
    put(2, 17, '●', 't-g'); put(4, 17, 'opencode  done');
    put(2, 22, 'ctrl-b ? help', 't-d');
    box(34, 0, 62, H - 1, 'claude · sidebar-polish', 't-c');
    [
      ['✻ Claude Code', 't-m'], ['', ''], ['› tighten host rows in compact mode', ''], ['', ''],
      ['● Read src/sidebar/rows.rs', 't-w'], ['● Update rows.rs', 't-w'], ['    +    .gap(px(6.))', 't-g'], ['    -    .gap(px(10.))', 't-r'],
      ['● Bash just ci', 't-w'], ['  ⎿ test result: ok', 't-d'], ['', ''], ['✶ Thinking…', 't-w'],
    ].forEach(([s, c], i) => put(37, 2 + i, s, c));
    box(96, 0, 54, H - 1, 'zsh · herdr-gpui');
    [
      ['herdr-gpui main ❯ just run', 't-c'], ['   Compiling herdr-gpui', 't-d'], ['    Finished release', 't-g'], ['', ''],
      ['herdr-gpui main ❯ gh pr checks', 't-c'], ['✓ Format, lint, and test', 't-g'], ['✓ windows', 't-g'], ['● release-dry-run', 't-w'], ['', ''],
      ['herdr-gpui main ❯ █', 't-c'],
    ].forEach(([s, c], i) => put(99, 2 + i, s, c));
    put(0, H - 1, ' NORMAL ', 't-sel'); put(9, H - 1, ' herdr-gpui ▸ sidebar-polish   3 agents · 1 blocked', 't-d');
    const esc = (ch) => ch === '<' ? '&lt;' : ch === '&' ? '&amp;' : ch;
    document.getElementById('tui').innerHTML = g.map(row => {
      let out = '', cur = null, buf = '';
      for (const [ch, c] of row) { if (c !== cur) { if (buf) out += cur ? `<span class="${cur}">${buf}</span>` : buf; buf = ''; cur = c; } buf += esc(ch); }
      if (buf) out += cur ? `<span class="${cur}">${buf}</span>` : buf;
      return out;
    }).join('\n');
  })();

  // ---- drag to compare ----
  (function () {
    const cmp = document.getElementById('cmp'), h = document.getElementById('handle');
    const set = (pct) => { pct = Math.max(3, Math.min(97, pct)); cmp.style.setProperty('--split', pct + '%'); h.setAttribute('aria-valuenow', Math.round(pct)); };
    const at = (e) => { const r = cmp.getBoundingClientRect(); set((e.clientX - r.left) / r.width * 100); };
    let drag = false;
    cmp.addEventListener('pointerdown', (e) => {
      if (NARROW.matches) return; // phones pan the stage instead of dragging
      // Without preventDefault, WebKit starts a text selection under the drag.
      e.preventDefault();
      window.getSelection()?.removeAllRanges();
      drag = true; cmp.setPointerCapture(e.pointerId); at(e);
    });
    cmp.addEventListener('dragstart', (e) => e.preventDefault());
    cmp.addEventListener('selectstart', (e) => e.preventDefault());
    cmp.addEventListener('pointermove', (e) => drag && at(e));
    cmp.addEventListener('pointerup', () => drag = false);
    cmp.addEventListener('pointercancel', () => drag = false);
    h.addEventListener('keydown', (e) => {
      const v = +h.getAttribute('aria-valuenow');
      if (e.key === 'ArrowLeft') { set(v - 5); e.preventDefault(); }
      if (e.key === 'ArrowRight') { set(v + 5); e.preventDefault(); }
    });
    set(46);

    // Phones show one client at a time; the tabs pick which.
    const tabs = document.querySelectorAll('#cmp-tabs button');
    const show = (view) => {
      tabs.forEach(b => b.setAttribute('aria-pressed', b.dataset.view === view));
      document.querySelectorAll('.labels span').forEach(s => s.classList.toggle('on', s.dataset.view === view));
      if (NARROW.matches) { cmp.style.setProperty('--split', view === 'tui' ? '100%' : '0%'); document.getElementById('cmp-fit').scrollLeft = 0; }
      else set(46);
    };
    tabs.forEach(b => b.addEventListener('click', () => show(b.dataset.view)));
    NARROW.addEventListener('change', () => show('gpui'));
    show('gpui');
  })();

  // ---- capability rows ----
  // Claims about Herdr's TUI follow Herdr's own README and docs; keep them in
  // step when either project changes.
  const ROWS = [
    { k: 'Terminals & agent status', t: 'The core, shared', p: 'Panes, splits, scrollback and the daemon’s working, blocked and done states. Both clients show the same state from the same snapshot.',
      v: ['both', 'Both clients'], tl: 'Native to Herdr', tui: `<span class="t-b">┌─</span><span class="t-h"> agents </span><span class="t-b">─────────────────</span>
<span class="t-b">│</span> <span class="t-w">●</span> claude    working
<span class="t-b">│</span> <span class="t-r">●</span> codex     blocked
<span class="t-b">│</span> <span class="t-g">●</span> opencode  done
<span class="t-b">└───────────────────────────</span>`, g: 'terminal' },
    { k: 'Devices', t: 'Every machine at a glance', p: 'Both clients keep local and saved SSH machines together. The window adds Coder and Daytona workspaces, WSL, per-host load and a two-hour activity view.',
      v: ['more', 'Both · more in the window'], tl: 'Saved SSH machines', tui: `<span class="t-d">LOCAL</span>
<span class="t-h">▾ herdr-gpui</span>
  <span class="t-w">●</span> sidebar-polish
  <span class="t-r">●</span> agent/412-ports
<span class="t-d">BUILD-BOX · SSH</span>
<span class="t-h">▾ arbor</span>
  <span class="t-w">●</span> fan-out lane 2
  <span class="t-g">●</span> fan-out lane 3`, g: 'devices' },
    { k: 'Orchestrator', t: 'Issues to agent runs', p: 'GitHub issues, beads and pull requests for a repository, with a Dispatch button that starts an agent in a new worktree on the host with the most room.',
      v: ['only', 'Window only'], tl: 'Scriptable', tui: `<span class="t-d">Agents and scripts can drive
herdr through its CLI and
socket API:</span>

<span class="t-c">$</span> herdr worktree create \\
    --branch fix-412
<span class="t-c">$</span> herdr agent prompt ...`, g: 'orchestrator' },
    { k: 'Fan-out', t: 'One prompt, several agents', p: 'Send a prompt to Claude, Codex, OpenCode and others in parallel lanes, spread across hosts, then compare the results and keep the best.',
      v: ['only', 'Window only'], tl: 'By hand', tui: `<span class="t-d">Create a worktree per agent,
start each one, then compare
the branches yourself:</span>

<span class="t-c">$</span> herdr worktree create ...
<span class="t-c">$</span> claude "fix the flaky test"
<span class="t-c">$</span> git diff main..lane-1`, g: 'fanout' },
    { k: 'Review & checkpoints', t: 'Read it, note it, rewind it', p: 'Side-by-side diffs with line notes that go back to the agent, and a checkpoint per agent turn that you can restore.',
      v: ['only', 'Window only'], tl: 'Your usual git tools', tui: `<span class="t-c">$</span> git diff
<span class="t-d">diff --git a/sidebar/rows.rs</span>
<span class="t-r">-        .gap(px(10.))</span>
<span class="t-g">+        .gap(px(6.))</span>

<span class="t-d">Then type your feedback into
the agent's pane.</span>`, g: 'review' },
    { k: 'Teleport', t: 'Move work between machines', p: 'Send a worktree, its uncommitted changes, tabs, splits and running agents to another host, after a review step.',
      v: ['only', 'Window only'], tl: 'Not in the TUI', tui: null, g: 'teleport' },
    { k: 'VS Code & browser tabs', t: 'Editors next to agents', p: 'VS Code and browser tabs in the same editor groups as your terminals. Notes you pin on a page go back to the agent that opened it.',
      v: ['only', 'Window only'], tl: 'Separate apps', tui: `<span class="t-d">Open VS Code and a browser
next to your terminal, and
copy notes across by hand.</span>`, g: 'terminal' },
  ];
  document.getElementById('rows').innerHTML = ROWS.map(r => `
    <div class="row">
      <div><div class="eyebrow">${r.k}</div><h3>${r.t}</h3><p>${r.p}</p><span class="verdict ${r.v[0]}">${r.v[0] === 'only' ? '◐' : '●●'} ${r.v[1]}</span></div>
      <div><div class="side-lbl"><span>herdr TUI</span><span>${r.tl}</span></div>
        ${r.tui ? `<div class="mini-tui"><pre>${r.tui}</pre></div>` : `<div class="mini-tui empty"><b>—</b><div>${r.tl}.<br>The daemon keeps running the agents either way.</div></div>`}</div>
      <div><div class="side-lbl"><span>Herdr GPUI</span><span>${r.v[0] === 'both' ? 'Same data' : 'Built in'}</span></div>
        <div class="mini-gpui" aria-hidden="true"><div class="scaler" data-fit-width="780" data-fit-max="0.5"><div data-hm-app="${r.g}" data-no-sidebar></div></div></div></div>
    </div>`).join('');

  // ---- scale fixed-size renderings to their container ----
  // Phones use a fixed, legible scale and pan sideways. `zoom` rather than a
  // transform there, so the scaled size is what the scroll container sees.
  function fit() {
    const stage = document.getElementById('cmp'), box = document.getElementById('cmp-fit');
    const narrow = NARROW.matches;
    document.body.classList.toggle('narrow', narrow);
    const s = narrow ? NARROW_SCALE : Math.min(1, box.clientWidth / 1120);
    stage.style.transform = !narrow && s < 1 ? `scale(${s})` : '';
    stage.style.zoom = narrow ? s : '';
    box.style.height = narrow ? '' : (458 * s) + 'px';
    document.querySelectorAll('[data-fit-width]').forEach(el => {
      const max = parseFloat(el.dataset.fitMax) || 1;
      const k = narrow ? NARROW_SCALE : Math.min(max, el.parentElement.clientWidth / +el.dataset.fitWidth);
      el.style.transform = narrow ? '' : `scale(${k})`;
      el.style.zoom = narrow ? k : '';
    });
  }
  window.addEventListener('resize', fit);
  fit();

  // ---- film: shown once assets/film.mp4 exists ----
  (function () {
    const section = document.getElementById('film'), box = document.getElementById('film-box');
    const reveal = () => { section.hidden = false; document.querySelector('[data-film-link]').hidden = false; };
    const v = document.createElement('video');
    v.src = 'assets/film.mp4';
    v.poster = 'assets/film-poster.webp';
    v.controls = true; v.preload = 'metadata'; v.playsInline = true;
    v.addEventListener('loadedmetadata', () => { box.replaceChildren(v); reveal(); }, { once: true });
    v.addEventListener('error', () => {
      if (!DRAFT) return;
      box.innerHTML = '<div data-hm-video data-poster="orchestrator"></div>';
      HM.mountAll(box);
      reveal();
    }, { once: true });
  })();

  // ---- screenshots: each tile appears once assets/shots/<id>.webp exists ----
  (function () {
    const order = ['teleport', 'orchestrator', 'fanout', 'review', 'checkpoints', 'devices', 'pr', 'ports', 'vscode', 'annotate', 'sidebar', 'usage', 'cells'];
    const big = new Set(['teleport', 'orchestrator']);
    const gal = document.getElementById('gal');
    const shots = order.map(id => HM.SHOTS.find(s => s.id === id)).filter(Boolean);
    const tiles = shots.map(s => {
      const t = document.createElement('div');
      t.className = 't' + (big.has(s.id) ? ' big' : '');
      t.hidden = true;
      gal.appendChild(t);
      return t;
    });
    const show = (t) => { t.hidden = false; document.getElementById('shots').hidden = false; document.querySelector('[data-shots-link]').hidden = false; };
    shots.forEach((s, i) => {
      const t = tiles[i], img = new Image();
      img.alt = s.title;
      img.loading = 'lazy';
      img.onload = () => {
        t.replaceChildren(img);
        t.insertAdjacentHTML('beforeend', `<h4>${s.title}</h4><p>${s.cap}</p>`);
        img.addEventListener('click', () => zoom(img.src, s.title, s.cap));
        show(t);
      };
      img.onerror = () => {
        if (!DRAFT) return;
        t.innerHTML = `${HM.shot(s)}<h4>${s.title}</h4><p>${s.cap}</p>`;
        show(t);
      };
      img.src = `assets/shots/${s.id}.webp`;
    });
    function zoom(src, title, cap) {
      const z = document.createElement('div');
      z.className = 'zoom';
      z.innerHTML = `<div><img alt=""><p></p></div>`;
      z.querySelector('img').src = src;
      z.querySelector('p').textContent = `${title}: ${cap}`;
      const close = () => { z.remove(); document.removeEventListener('keydown', onKey); };
      const onKey = (e) => e.key === 'Escape' && close();
      z.addEventListener('click', close);
      document.addEventListener('keydown', onKey);
      document.body.appendChild(z);
    }
  })();

  // ---- GitHub stars: live count from the public API, cached per tab ----
  // The unauthenticated API allows 60 requests an hour per IP, so the count is
  // kept in sessionStorage; on any failure the button just reads "GitHub".
  (function () {
    const KEY = 'herdr-gpui-stars', TTL = 10 * 60 * 1000;
    const show = (n) => {
      if (typeof n !== 'number' || !isFinite(n)) return;
      document.getElementById('stars-n').textContent =
        n >= 1000 ? (n / 1000).toFixed(1).replace(/\.0$/, '') + 'k' : String(n);
      const el = document.getElementById('stars');
      el.hidden = false;
      el.parentElement.setAttribute('aria-label', `GitHub, ${n} stars`);
    };
    try {
      const c = JSON.parse(sessionStorage.getItem(KEY) || 'null');
      if (c && Date.now() - c.t < TTL) return show(c.n);
    } catch {}
    fetch('https://api.github.com/repos/penso/herdr-gpui', { headers: { Accept: 'application/vnd.github+json' } })
      .then(r => (r.ok ? r.json() : null))
      .then(d => {
        if (!d || typeof d.stargazers_count !== 'number') return;
        show(d.stargazers_count);
        try { sessionStorage.setItem(KEY, JSON.stringify({ n: d.stargazers_count, t: Date.now() })); } catch {}
      })
      .catch(() => {});
  })();

  document.getElementById('brew').textContent = HM.FACTS.install;
  document.getElementById('plat').textContent = HM.FACTS.platforms;
  document.getElementById('disc').textContent = HM.FACTS.disclaimer;
  document.getElementById('copy').addEventListener('click', async (e) => {
    try {
      await navigator.clipboard.writeText(HM.FACTS.install);
      e.target.textContent = 'Copied';
    } catch {
      e.target.textContent = 'Select and copy';
    }
    setTimeout(() => { e.target.textContent = 'Copy'; }, 1600);
  });
})();
