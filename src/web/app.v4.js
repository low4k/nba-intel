// NBA Intel dashboard v4 — headshots, team logos, richer cards.
console.log('[nba-intel] app.v4.js loaded', new Date().toISOString());

// ---------- CDN helpers ----------
// We use ESPN player IDs throughout the app, so we pull faces from ESPN's CDN.
// ESPN serves a single high-res PNG per player at this path; it resizes fine in CSS.
const headshot = (id /*, size */) => {
  if (!id) return '';
  return `https://a.espncdn.com/i/headshots/nba/players/full/${id}.png`;
};
// A player record from our API may carry a `headshot` URL — always prefer it.
const faceOf = (p) => (p && p.headshot) ? p.headshot : headshot(p && p.id);
const teamLogo = (abbr) => {
  if (!abbr) return '';
  // ESPN's logo CDN covers every NBA team by lowercase abbr.
  return `https://a.espncdn.com/i/teamlogos/nba/500/${abbr.toLowerCase()}.png`;
};

const NBA_TEAMS = [
  'ATL','BOS','BKN','CHA','CHI','CLE','DAL','DEN','DET','GSW',
  'HOU','IND','LAC','LAL','MEM','MIA','MIL','MIN','NOP','NYK',
  'OKC','ORL','PHI','PHX','POR','SAC','SAS','TOR','UTA','WAS'
];
const SEASON = '2026';

// ---------- tabs ----------
const tabs = document.querySelectorAll('nav button');
const panels = document.querySelectorAll('section[data-panel]');
tabs.forEach(b => b.addEventListener('click', () => {
  tabs.forEach(x => x.classList.remove('active'));
  panels.forEach(p => p.classList.remove('visible'));
  b.classList.add('active');
  const panel = document.querySelector(`section[data-panel="${b.dataset.tab}"]`);
  if (panel) panel.classList.add('visible');
  if (b.dataset.tab === 'team') renderTeamGrid();
}));

const themeBtn = document.getElementById('theme-toggle');
themeBtn.addEventListener('click', () => {
  const next = document.body.dataset.theme === 'dark' ? 'light' : 'dark';
  document.body.dataset.theme = next;
  themeBtn.textContent = next === 'dark' ? 'light' : 'dark';
});

// Fix UTF-8 bytes that got re-encoded through Latin-1 ("Luka DonÄ\x8diÄ\x87" → "Luka Dončić").
// Safe no-op on strings that already contain valid extended chars from a different source.
function fixMojibake(s) {
  if (typeof s !== 'string' || s.length === 0) return s;
  // Must contain at least one 0x80-0xFF char AND look like a UTF-8 leading byte sequence.
  if (!/[\u00c2-\u00f4][\u0080-\u00bf]/.test(s)) return s;
  try {
    const bytes = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) bytes[i] = s.charCodeAt(i) & 0xff;
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch { return s; }
}
function deepFix(v) {
  if (typeof v === 'string') return fixMojibake(v);
  if (Array.isArray(v)) { for (let i = 0; i < v.length; i++) v[i] = deepFix(v[i]); return v; }
  if (v && typeof v === 'object') { for (const k in v) v[k] = deepFix(v[k]); return v; }
  return v;
}

// Lightweight in-memory cache for GETs; TTL 45s.
const _apiCache = new Map();
async function api(path, opts = {}) {
  const ttl = opts.ttl ?? 45_000;
  const now = Date.now();
  const hit = _apiCache.get(path);
  if (!opts.force && hit && now - hit.t < ttl) return hit.v;
  const r = await fetch(path, { headers: { accept: 'application/json' } });
  if (!r.ok) throw new Error(`${path} -> ${r.status}`);
  const v = deepFix(await r.json());
  _apiCache.set(path, { t: now, v });
  return v;
}
const escHtml = s => String(s ?? '').replace(/[&<>"']/g, c => ({
  '&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'
}[c]));
const onImgFail = `this.onerror=null;this.src='/static/fallback-face.svg'`;

// ---------- dashboard: leaders ----------
const LEADER_CATEGORIES = [
  { key: 'pts', label: 'PTS' },
  { key: 'reb', label: 'REB' },
  { key: 'ast', label: 'AST' },
  { key: 'stl', label: 'STL' },
  { key: 'blk', label: 'BLK' },
];
let leadersData = null;
let activeLeaderCat = 'pts';

function renderLeaderRows(rows) {
  if (!rows || rows.length === 0) {
    return '<div class="muted">no data</div>';
  }
  return '<div class="leaders-list">' + rows.map((r, i) => {
    const face = r.id ? headshot(r.id) : '';
    const logo = r.team ? teamLogo(r.team) : '';
    const val = typeof r.value === 'number' ? r.value.toFixed(1) : (r.value ?? '—');
    return `
      <div class="leader-row" data-player-id="${escHtml(r.id)}" data-player-name="${escHtml(r.name || '')}" style="--face:url('${face}'); --logo:url('${logo}')">
        <div class="rank ${i === 0 ? 'top1' : ''}">${i + 1}</div>
        <div class="face"></div>
        <div class="meta">
          <div class="name">${escHtml(r.name || r.id)}</div>
          <div class="team"><span class="logo"></span>${escHtml(r.team || '')}</div>
        </div>
        <div class="value">${val}</div>
      </div>`;
  }).join('') + '</div>';
}

function renderLeaders() {
  const el = document.getElementById('dash-leaders');
  if (!el) return;
  if (!leadersData) {
    el.innerHTML = '<div class="skeleton"></div><div class="skeleton" style="margin-top:6px"></div><div class="skeleton" style="margin-top:6px"></div>';
    return;
  }
  const tabsHtml = '<div class="leaders-tabs">' + LEADER_CATEGORIES.map(c =>
    `<button class="${c.key === activeLeaderCat ? 'active' : ''}" data-leader="${c.key}">${c.label}</button>`
  ).join('') + '</div>';
  el.innerHTML = tabsHtml + renderLeaderRows(leadersData[activeLeaderCat]);
  el.querySelectorAll('[data-leader]').forEach(b => b.addEventListener('click', () => {
    activeLeaderCat = b.dataset.leader;
    renderLeaders();
  }));
  el.querySelectorAll('.leader-row').forEach(row => row.addEventListener('click', () => {
    const id = row.dataset.playerId;
    if (!id) return;
    openPlayer(id, row.dataset.playerName);
  }));
}

async function refreshLeaders() {
  try {
    leadersData = await api('/api/leaders');
    renderLeaders();
  } catch (e) {
    const el = document.getElementById('dash-leaders');
    if (el) el.innerHTML = '<div class="muted">leaders unavailable</div>';
  }
}

// ---------- dashboard: live ----------
function renderLiveGames(games) {
  if (!games || games.length === 0) {
    return '<div class="muted center" style="padding:24px">no games scheduled right now</div>';
  }
  return '<div class="live-list">' + games.map(g => {
    const home = g.home || {}; const away = g.away || {};
    const homeScore = home.score ?? 0, awayScore = away.score ?? 0;
    const homeLead = homeScore > awayScore;
    const awayLead = awayScore > homeScore;
    const wp = typeof g.win_probability_home === 'number' ? g.win_probability_home : null;
    const pace = typeof g.pace_estimate === 'number' ? g.pace_estimate.toFixed(1) : '—';
    const st = (g.status || '').toLowerCase();
    const pillClass = st.includes('final') ? 'final' : st.includes('scheduled') ? 'scheduled' : '';
    const statusLine = st.includes('final')
      ? 'Final'
      : st.includes('scheduled')
        ? (g.date ? new Date(g.date).toLocaleString([], { weekday: 'short', hour: 'numeric', minute: '2-digit' }) : 'Scheduled')
        : `Q${g.period ?? '?'} · ${g.clock || ''}`;
    const wpBar = wp != null
      ? `<div class="wp-bar" title="home win prob ${(wp*100).toFixed(1)}%">
           <div class="home-wp" style="width:${(wp*100).toFixed(1)}%"></div>
           <div class="away-wp" style="width:${(100 - wp*100).toFixed(1)}%"></div>
         </div>` : '';
    return `
      <div class="live-game">
        <div class="teams">
          <div class="side ${awayLead ? 'leading' : ''}" style="--logo:url('${teamLogo(away.team)}')">
            <div class="logo"></div>
            <div><div class="name">${escHtml(away.team || '')}</div><div class="muted" style="font-size:11px">${escHtml(away.record || '')}</div></div>
            <div class="score">${awayScore}</div>
          </div>
          <div class="mid">
            <span class="status-pill ${pillClass}">${escHtml(statusLine)}</span>
            ${g.name ? `<span style="font-size:10px">${escHtml(g.name)}</span>` : ''}
          </div>
          <div class="side home ${homeLead ? 'leading' : ''}" style="--logo:url('${teamLogo(home.team)}')">
            <div class="score">${homeScore}</div>
            <div><div class="name">${escHtml(home.team || '')}</div><div class="muted" style="font-size:11px">${escHtml(home.record || '')}</div></div>
            <div class="logo"></div>
          </div>
        </div>
        ${wpBar}
        <div class="footer">
          <span>win prob (home): <b>${wp != null ? (wp*100).toFixed(1) + '%' : '—'}</b></span>
          <span>pace est: <b>${pace}</b></span>
        </div>
      </div>`;
  }).join('') + '</div>';
}

async function refreshLive() {
  try {
    const data = await api('/api/live');
    const games = Object.values(data.games || {});
    const html = renderLiveGames(games);
    const dl = document.getElementById('dash-live');
    const ll = document.getElementById('live-list');
    if (dl) dl.innerHTML = html;
    if (ll) ll.innerHTML = html;
  } catch (e) {
    const msg = '<div class="muted">live feed unavailable</div>';
    const dl = document.getElementById('dash-live');
    const ll = document.getElementById('live-list');
    if (dl) dl.innerHTML = msg;
    if (ll) ll.innerHTML = msg;
  }
}

// ---------- pinned ----------
function pinnedIds() {
  try { return JSON.parse(localStorage.getItem('nba-pinned') || '[]'); } catch { return []; }
}
function savePinned(ids) { localStorage.setItem('nba-pinned', JSON.stringify(ids)); }

async function refreshPinned() {
  const el = document.getElementById('dash-pinned');
  if (!el) return;
  const ids = pinnedIds();
  if (ids.length === 0) {
    el.innerHTML = '<div class="muted">click any leader or roster player to pin them here</div>';
    return;
  }
  try {
    const rows = await Promise.all(ids.map(id =>
      api('/api/players/' + encodeURIComponent(id)).then(p => ({ id, p })).catch(() => ({ id, p: null }))));
    el.innerHTML = '<div class="leaders-list">' + rows.map(({ id, p }) => {
      const face = headshot(id);
      if (!p || !(p.name || '').length) {
        return `
          <div class="leader-row" style="--face:url('${face}')">
            <div class="rank">•</div>
            <div class="face"></div>
            <div class="meta"><div class="name muted">id ${escHtml(id)} (no data)</div></div>
            <div class="value"><button data-unpin="${escHtml(id)}" title="unpin">×</button></div>
          </div>`;
      }
      const t = p.traditional || {};
      const logo = teamLogo(p.team);
      const line = `${(t.pts || 0).toFixed(1)} / ${(t.reb || 0).toFixed(1)} / ${(t.ast || 0).toFixed(1)}`;
      return `
        <div class="leader-row" data-player-id="${escHtml(id)}" style="--face:url('${face}'); --logo:url('${logo}')">
          <div class="rank">•</div>
          <div class="face"></div>
          <div class="meta">
            <div class="name">${escHtml(p.name)}</div>
            <div class="team"><span class="logo"></span>${escHtml(p.team || '')}</div>
          </div>
          <div class="value" style="display:flex;align-items:center;gap:6px">
            <span style="font-size:12px;color:var(--muted)">${line}</span>
            <button data-unpin="${escHtml(id)}" title="unpin" style="padding:1px 8px">×</button>
          </div>
        </div>`;
    }).join('') + '</div>';
    el.querySelectorAll('[data-unpin]').forEach(btn => btn.addEventListener('click', (ev) => {
      ev.stopPropagation();
      const id = btn.dataset.unpin;
      savePinned(pinnedIds().filter(x => x !== id));
      refreshPinned();
    }));
    el.querySelectorAll('.leader-row[data-player-id]').forEach(row => row.addEventListener('click', () => {
      openPlayer(row.dataset.playerId, row.dataset.playerName);
    }));
  } catch {
    el.innerHTML = '<div class="muted">pinned unavailable</div>';
  }
}

// ---------- player tab ----------
function openPlayer(id, name) {
  if (!id) return;
  document.querySelector('nav button[data-tab="player"]').click();
  const inp = document.getElementById('player-id');
  if (inp) {
    inp.value = name || id;        // show the NAME, not the numeric id
    inp.dataset.playerId = id;     // but remember the id for future fetches
  }
  loadPlayer(id);
}

// Aggregate raw totals from a game log slice to derive advanced metrics.
function aggGames(games) {
  const a = { n:0, min:0, pts:0, fgm:0, fga:0, fg3m:0, fg3a:0, ftm:0, fta:0, reb:0, ast:0, stl:0, blk:0, tov:0 };
  games.forEach(g => {
    a.n++; a.min += g.minutes||0;
    a.pts += g.pts||0; a.fgm += g.fgm||0; a.fga += g.fga||0;
    a.fg3m += g.fg3m||0; a.fg3a += g.fg3a||0; a.ftm += g.ftm||0; a.fta += g.fta||0;
    a.reb += g.reb||0; a.ast += g.ast||0; a.stl += g.stl||0; a.blk += g.blk||0; a.tov += g.tov||0;
  });
  return a;
}
function deriveEfficiency(agg) {
  if (!agg.n) return null;
  const ts  = agg.fga || agg.fta ? agg.pts / (2 * (agg.fga + 0.44 * agg.fta)) : 0;
  const efg = agg.fga ? (agg.fgm + 0.5 * agg.fg3m) / agg.fga : 0;
  const ato = agg.tov ? agg.ast / agg.tov : agg.ast;
  return {
    ts_pct: ts * 100,
    efg_pct: efg * 100,
    ast_to: ato,
    pra_avg: (agg.pts + agg.reb + agg.ast) / agg.n,
    mpg: agg.min / agg.n,
  };
}
function careerHighs(games) {
  if (!games.length) return null;
  const max = (fn) => games.reduce((m, g) => (fn(g) > m.v ? { v: fn(g), g } : m), { v: -Infinity, g: null });
  const p = max(g => g.pts||0), r = max(g => g.reb||0), a = max(g => g.ast||0), t3 = max(g => g.fg3m||0);
  return { pts: p, reb: r, ast: a, fg3m: t3 };
}
function fmtOpp(g) {
  if (!g) return '';
  return `${g.venue === 'away' ? '@' : 'vs'} ${g.opponent || ''}${g.game_date ? ' · ' + g.game_date.slice(0,10) : ''}`;
}

async function loadPlayer(id) {
  const out = document.getElementById('player-out');
  out.innerHTML = '<div class="skeleton" style="height:120px"></div><div class="skeleton" style="height:60px;margin-top:8px"></div><div class="skeleton" style="height:300px;margin-top:8px"></div>';
  try {
    const p = await api('/api/players/' + encodeURIComponent(id));
    const face = faceOf({ id, ...p });
    const logo = teamLogo(p.team);
    const t = p.traditional || {};
    const games = p.game_log || [];

    // --- season averages (big stat cards) ---
    const stats = [
      ['PTS', t.pts], ['REB', t.reb], ['AST', t.ast],
      ['STL', t.stl], ['BLK', t.blk], ['TOV', t.tov],
      ['FG%', t.fg_pct], ['3P%', t.fg3_pct], ['FT%', t.ft_pct],
      ['MIN', t.min], ['GP', t.gp],
    ];
    const statGrid = stats.filter(([,v]) => v != null && v !== 0).map(([lbl, v]) => {
      const display = (typeof v === 'number')
        ? (lbl.endsWith('%') ? (v * (v < 1 ? 100 : 1)).toFixed(1) : v.toFixed(1))
        : v;
      return `<div class="stat-box"><div class="lbl">${lbl}</div><div class="val">${display}</div></div>`;
    }).join('');

    // --- bio grid: EVERYTHING ESPN gives us ---
    const bioFields = [
      ['Team',       p.team_name || p.team],
      ['Position',   p.position_full || p.position],
      ['Jersey',     p.jersey ? '#' + p.jersey : ''],
      ['Height',     p.height_display],
      ['Weight',     p.weight_display],
      ['Wingspan',   p.wingspan_display],
      ['Age',        p.age > 0 ? p.age : ''],
      ['Born',       p.dob],
      ['Birthplace', p.birthplace],
      ['College',    p.college],
      ['Draft',      p.draft],
      ['Debut',      p.debut_year > 0 ? p.debut_year : ''],
      ['Experience', p.experience],
      ['Status',     p.status],
    ].filter(([,v]) => v != null && v !== '' && v !== 0);
    const bioHtml = bioFields.length
      ? `<div class="bio-grid">${bioFields.map(([k, v]) =>
          `<div class="bio-row"><span class="k">${k}</span><span class="v">${escHtml(v)}</span></div>`
        ).join('')}</div>`
      : '';

    // --- advanced / derived efficiency (last 10 games) ---
    const l10 = games.slice(-10);
    const eff = deriveEfficiency(aggGames(l10));
    const adv = p.advanced || {};
    const advFields = [];
    if (eff) {
      advFields.push(['TS%',     eff.ts_pct.toFixed(1) + '%']);
      advFields.push(['eFG%',    eff.efg_pct.toFixed(1) + '%']);
      advFields.push(['AST/TO',  eff.ast_to.toFixed(2)]);
      advFields.push(['PRA avg', eff.pra_avg.toFixed(1)]);
      advFields.push(['MPG',     eff.mpg.toFixed(1)]);
    }
    if (adv.per  > 0) advFields.push(['PER',  adv.per.toFixed(1)]);
    if (adv.usg_pct > 0) advFields.push(['USG%', (adv.usg_pct * (adv.usg_pct < 1 ? 100 : 1)).toFixed(1) + '%']);
    if (adv.bpm != 0 && adv.bpm != null) advFields.push(['BPM', adv.bpm.toFixed(1)]);
    const advHtml = advFields.length
      ? `<div class="stat-grid">${advFields.map(([lbl, v]) =>
          `<div class="stat-box accent"><div class="lbl">${lbl}</div><div class="val">${escHtml(v)}</div></div>`
        ).join('')}</div>`
      : '';

    // --- career-season highs ---
    const ch = careerHighs(games);
    const chHtml = ch
      ? `<div class="highs-row">
          <div class="high"><span class="lbl">Season-high PTS</span><span class="val">${ch.pts.v}</span><span class="ctx">${escHtml(fmtOpp(ch.pts.g))}</span></div>
          <div class="high"><span class="lbl">Season-high REB</span><span class="val">${ch.reb.v}</span><span class="ctx">${escHtml(fmtOpp(ch.reb.g))}</span></div>
          <div class="high"><span class="lbl">Season-high AST</span><span class="val">${ch.ast.v}</span><span class="ctx">${escHtml(fmtOpp(ch.ast.g))}</span></div>
          <div class="high"><span class="lbl">Most 3PM</span><span class="val">${ch.fg3m.v}</span><span class="ctx">${escHtml(fmtOpp(ch.fg3m.g))}</span></div>
        </div>`
      : '';

    const pinned = pinnedIds().includes(String(id));
    const shortSub = [p.team, p.position_full || p.position, p.jersey ? '#' + p.jersey : '', p.height_display, p.weight_display]
      .filter(x => x).join(' · ');

    // --- full projection card (chart + controls + chips) ---
    const projCard = `
      <div class="proj-card">
        <div class="proj-head">
          <span class="proj-title">Next-game projection</span>
          <div class="proj-controls">
            <select id="proj-stat" title="stat">
              <option>PTS</option><option>REB</option><option>AST</option>
              <option>PRA</option><option>PR</option><option>PA</option><option>RA</option>
              <option>STL</option><option>BLK</option><option>TOV</option>
              <option>FG_PCT</option><option>FG3_PCT</option>
            </select>
            <select id="proj-range" title="sample window">
              <option value="L5">last 5</option>
              <option value="L10" selected>last 10</option>
              <option value="L15">last 15</option>
              <option value="season">season</option>
            </select>
            <label class="inline-num" title="opponent defensive rating">
              opp DRtg <input id="proj-opp" type="number" value="113" step="0.5" style="width:62px">
            </label>
            <label class="inline-num" title="days of rest">
              rest <input id="proj-rest" type="number" value="1" min="0" style="width:46px">
            </label>
            <label class="inline-chk" title="drop blowouts"><input id="proj-garbage" type="checkbox" checked> no garbage</label>
          </div>
        </div>
        <canvas id="proj-chart" width="920" height="260"></canvas>
        <div id="proj-chips"></div>
      </div>`;

    out.innerHTML = `
      <div class="player-header" style="--face:url('${face}'); --logo:url('${logo}')">
        <div class="avatar"></div>
        <div class="title">
          <div class="name">${escHtml(p.full_name || p.name || ('Player ' + id))}</div>
          <div class="sub">
            <span class="logo"></span>
            <span>${escHtml(shortSub)}</span>
          </div>
        </div>
        <button class="pin-btn ${pinned ? '' : 'primary'}" id="pin-toggle">${pinned ? 'unpin' : 'pin'}</button>
      </div>
      ${bioHtml}
      <h3 class="section-h">SEASON AVERAGES</h3>
      <div class="stat-grid">${statGrid || '<div class="muted">no season stats yet</div>'}</div>
      ${advHtml ? `<h3 class="section-h">ADVANCED · L10 EFFICIENCY</h3>${advHtml}` : ''}
      ${chHtml  ? `<h3 class="section-h">SEASON HIGHS</h3>${chHtml}` : ''}
      ${projCard}
      ${renderGameLog(games, 15)}
      ${(p.sources_used && p.sources_used.length)
        ? `<div style="margin-top:10px" class="muted"><span class="chip">sources: ${p.sources_used.map(escHtml).join(', ')}</span></div>` : ''}
    `;

    document.getElementById('pin-toggle').addEventListener('click', () => {
      const ids = pinnedIds();
      const sid = String(id);
      const next = ids.includes(sid) ? ids.filter(x => x !== sid) : [...ids, sid];
      savePinned(next);
      loadPlayer(id);
      refreshPinned();
    });

    // wire projection card
    const pStat = document.getElementById('proj-stat');
    const pRange = document.getElementById('proj-range');
    const pOpp = document.getElementById('proj-opp');
    const pRest = document.getElementById('proj-rest');
    const pGb = document.getElementById('proj-garbage');
    const pChips = document.getElementById('proj-chips');
    const runProj = async () => {
      pChips.innerHTML = '<div class="muted" style="margin-top:10px">projecting…</div>';
      try {
        const params = new URLSearchParams({
          stat:   pStat.value,
          range:  pRange.value,
          opp_drtg: pOpp.value,
          rest:   pRest.value,
          garbage: pGb.checked ? 'exclude' : 'include',
        });
        const d = await api(`/api/predict/${encodeURIComponent(id)}?${params}`);
        // Attach the player's game log so the chart can show a popup on click.
        if (!d.games) d.games = p.game_log || [];
        drawProjection(d, 'proj-chart');
        pChips.innerHTML = renderProjectionChips(d, pStat.value);
      } catch (e) {
        pChips.innerHTML = `<div class="muted">not enough data to project: ${escHtml(e.message)}</div>`;
      }
    };
    [pStat, pRange, pOpp, pRest, pGb].forEach(el => el.addEventListener('change', runProj));
    runProj();
  } catch (e) {
    out.innerHTML = `<div class="muted">could not load player ${escHtml(id)}: ${escHtml(e.message)}</div>`;
  }
}

// ------- projection chips (used inline on the player page) -------
function renderProjectionChips(d, statLabel) {
  if (!d || !d.samples) return '<div class="muted">no data</div>';
  const pt   = d.next_game_point;
  const low  = d.next_game_low;
  const high = d.next_game_high;
  const mean = d.mean;
  const sd   = d.stddev;
  const series = d.series || [];
  const l10 = series.slice(-10);
  const l5  = series.slice(-5);
  const avg = a => a.length ? a.reduce((x,y)=>x+y,0)/a.length : 0;
  const l10Avg = avg(l10), l5Avg = avg(l5);
  const trend = (l5Avg - l10Avg);
  const trendArrow = l5Avg > l10Avg + 0.1 ? '▲' : (l5Avg < l10Avg - 0.1 ? '▼' : '→');
  const threshold = pt ? Math.round(pt - 0.5) : Math.round(mean || 0);
  const overs = l10.filter(v => v >= threshold + 0.5).length;
  const ceiling = Math.max(...series);
  const floor   = Math.min(...series);
  return `
    <div class="pred-chips">
      <div class="chip-card big">
        <div class="lbl">projected ${statLabel}</div>
        <div class="val">${pt != null ? pt.toFixed(1) : '—'}</div>
        <div class="range">${low != null ? low.toFixed(1) : '—'} – ${high != null ? high.toFixed(1) : '—'}</div>
      </div>
      <div class="chip-card">
        <div class="lbl">L5 / L10</div>
        <div class="val">${l5Avg.toFixed(1)} / ${l10Avg.toFixed(1)}</div>
        <div class="range ${trend > 0 ? 'up' : (trend < 0 ? 'down' : '')}">${trendArrow} ${trend >= 0 ? '+' : ''}${trend.toFixed(1)}</div>
      </div>
      <div class="chip-card">
        <div class="lbl">mean · σ</div>
        <div class="val">${mean.toFixed(1)}</div>
        <div class="range">σ ${sd.toFixed(2)} · n=${d.samples}</div>
      </div>
      <div class="chip-card">
        <div class="lbl">over ${threshold + 0.5}</div>
        <div class="val">${overs}/${l10.length}</div>
        <div class="range">L10 hit rate</div>
      </div>
      <div class="chip-card">
        <div class="lbl">floor · ceiling</div>
        <div class="val">${isFinite(floor) ? floor : '—'} · ${isFinite(ceiling) ? ceiling : '—'}</div>
        <div class="range">season range</div>
      </div>
    </div>`;
}

function renderGameLog(games, n = 10) {
  if (!games.length) return '';
  const rows = games.slice(-n).reverse().map(g => `
    <tr>
      <td>${escHtml(g.game_date || '').slice(0,10)}</td>
      <td>${g.venue === 'away' ? '@' : 'vs'} ${escHtml(g.opponent || '')}</td>
      <td class="${g.result === 'W' ? 'good' : g.result === 'L' ? 'bad' : ''}">${escHtml(g.result || '')} ${escHtml(g.score || '')}</td>
      <td style="font-variant-numeric:tabular-nums">${(g.minutes||0).toFixed(0)}</td>
      <td style="font-variant-numeric:tabular-nums"><b>${g.pts ?? '—'}</b></td>
      <td style="font-variant-numeric:tabular-nums">${g.reb ?? '—'}</td>
      <td style="font-variant-numeric:tabular-nums">${g.ast ?? '—'}</td>
      <td style="font-variant-numeric:tabular-nums">${g.stl ?? 0}/${g.blk ?? 0}/${g.tov ?? 0}</td>
      <td style="font-variant-numeric:tabular-nums">${g.fgm ?? ''}-${g.fga ?? ''}</td>
      <td style="font-variant-numeric:tabular-nums">${g.fg3m ?? ''}-${g.fg3a ?? ''}</td>
      <td style="font-variant-numeric:tabular-nums">${g.ftm ?? ''}-${g.fta ?? ''}</td>
    </tr>`).join('');
  return `
    <h3 class="section-h">LAST ${Math.min(n, games.length)} GAMES</h3>
    <div style="overflow-x:auto">
    <table class="log-table">
      <thead><tr>
        <th>Date</th><th>Opp</th><th>Result</th>
        <th>MIN</th><th>PTS</th><th>REB</th><th>AST</th><th>S/B/TO</th><th>FG</th><th>3P</th><th>FT</th>
      </tr></thead>
      <tbody>${rows}</tbody>
    </table></div>`;
}

// enter key on player search input still submits (though autocomplete also loads on pick)
(function wirePlayerInput() {
  const inp = document.getElementById('player-id');
  if (!inp) return;
  inp.addEventListener('keydown', e => {
    if (e.key === 'Enter') {
      const v = inp.value.trim();
      if (/^\d+$/.test(v)) loadPlayer(v);
    }
  });
})();

// ---------- team tab ----------
function renderTeamGrid() {
  const host = document.getElementById('team-grid');
  if (!host || host.dataset.rendered) return;
  host.dataset.rendered = '1';
  host.innerHTML = NBA_TEAMS.map(abbr => `
    <div class="team-chip" data-abbr="${abbr}" style="--logo:url('${teamLogo(abbr)}')">
      <div class="logo"></div>
      <div class="abbr">${abbr}</div>
    </div>`).join('');
  host.querySelectorAll('.team-chip').forEach(chip => chip.addEventListener('click', () => {
    host.querySelectorAll('.team-chip').forEach(c => c.classList.remove('active'));
    chip.classList.add('active');
    loadTeam(`${chip.dataset.abbr}-${SEASON}`);
  }));
}

async function loadTeam(id) {
  const out = document.getElementById('team-out');
  out.innerHTML = '<div class="skeleton" style="height:80px"></div>';
  try {
    const t = await api('/api/teams/' + encodeURIComponent(id));
    const logo = teamLogo(t.abbr);
    const r = t.ratings || {};
    const ratings = [
      ['OFF', r.off_rating], ['DEF', r.def_rating], ['NET', r.net_rating], ['PACE', r.pace],
    ].filter(([,v]) => v != null);
    const ratingsHtml = ratings.length
      ? `<div class="stat-grid">${ratings.map(([lbl, v]) =>
          `<div class="stat-box"><div class="lbl">${lbl}</div><div class="val">${typeof v === 'number' ? v.toFixed(1) : v}</div></div>`
        ).join('')}</div>` : '';
    const roster = (t.roster || []).map(pl => `
      <div class="roster-card" data-player-id="${escHtml(pl.id)}" data-player-name="${escHtml(pl.name || '')}">
        <img class="face" src="${pl.headshot || headshot(pl.id)}" onerror="${onImgFail}" loading="lazy">
        <div class="info">
          <div class="nm">${escHtml(pl.name)}</div>
          <div class="meta">${escHtml(pl.position || '')}${pl.jersey ? ' · #'+escHtml(pl.jersey) : ''}${pl.height ? ' · '+escHtml(pl.height) : ''}</div>
        </div>
      </div>`).join('');
    out.innerHTML = `
      <div class="team-header" style="--logo:url('${logo}')">
        <div class="logo"></div>
        <div class="title">
          <div class="name">${escHtml(t.name || t.abbr)}</div>
          <div class="sub">${escHtml(t.abbr || '')} · ${escHtml(t.season || '')} ${t.record ? '· ' + escHtml(t.record) : ''}</div>
        </div>
        <div></div>
      </div>
      ${ratingsHtml}
      <h3 style="margin:12px 0 8px;font-size:13px;letter-spacing:.5px;color:var(--muted)">ROSTER</h3>
      <div class="roster-grid">${roster || '<div class="muted">no roster loaded</div>'}</div>
      ${(t.sources_used && t.sources_used.length)
        ? `<div style="margin-top:10px"><span class="chip">sources: ${t.sources_used.map(escHtml).join(', ')}</span></div>` : ''}
    `;
    out.querySelectorAll('.roster-card').forEach(rc => rc.addEventListener('click', () => {
      openPlayer(rc.dataset.playerId, rc.dataset.playerName);
    }));
  } catch (e) {
    out.innerHTML = `<div class="muted">could not load team ${escHtml(id)}: ${escHtml(e.message)}</div>`;
  }
}

document.getElementById('team-grid'); // presence check only

// ---------- projection chart ----------
// Map of canvas-id -> array of { x,y, game, value } for hit-testing clicks.
const _chartHits = new Map();

function drawProjection(data, canvasId = 'proj-chart') {
  const canvas = document.getElementById(canvasId);
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  // Retina crispness
  const dpr = window.devicePixelRatio || 1;
  if (canvas.width !== canvas.clientWidth * dpr) {
    canvas.width  = canvas.clientWidth  * dpr;
    canvas.height = canvas.clientHeight * dpr;
  }
  const cs = getComputedStyle(document.body);
  const accent  = cs.getPropertyValue('--accent').trim()   || '#ff6b1a';
  const accent2 = cs.getPropertyValue('--accent-2').trim() || '#38bdf8';
  const border  = cs.getPropertyValue('--border').trim()   || '#253041';
  const muted   = cs.getPropertyValue('--muted').trim()    || '#8a96a8';
  const bad     = cs.getPropertyValue('--bad').trim()      || '#f85149';
  const text    = cs.getPropertyValue('--text').trim()     || '#e7ecf3';

  const w = canvas.width, h = canvas.height;
  ctx.clearRect(0, 0, w, h);
  const series   = data.series   || [];
  const smoothed = data.smoothed || [];
  if (!series.length) {
    ctx.fillStyle = muted; ctx.font = `${13*dpr}px system-ui, sans-serif`;
    ctx.fillText('no game log yet', 20*dpr, 30*dpr);
    return;
  }
  const padL = 44*dpr, padR = 22*dpr, padT = 22*dpr, padB = 30*dpr;
  const innerW = w - padL - padR, innerH = h - padT - padB;
  const ghHigh = data.next_game_high || 0;
  const ghLow  = data.next_game_low  || 0;
  const max = Math.max(...series, ...(smoothed.length ? smoothed : [0]), ghHigh) * 1.12 + 1;
  const min = Math.max(0, Math.min(...series, ...(smoothed.length ? smoothed : [0]), ghLow) * 0.92);
  const xstep = innerW / (series.length + 1);
  const yfor = v => padT + innerH - ((v - min) / (max - min)) * innerH;

  // y-axis grid + labels
  ctx.strokeStyle = border; ctx.lineWidth = 1*dpr;
  ctx.fillStyle   = muted; ctx.font = `${10*dpr}px system-ui, sans-serif`;
  for (let i = 0; i <= 4; i++) {
    const y = padT + (innerH / 4) * i;
    ctx.beginPath(); ctx.moveTo(padL, y); ctx.lineTo(w - padR, y); ctx.stroke();
    const val = max - (max - min) * (i / 4);
    ctx.fillText(val.toFixed(val < 10 ? 1 : 0), 6*dpr, y + 4*dpr);
  }

  // projection band for next game
  if (data.next_game_low != null && data.next_game_high != null) {
    const x  = padL + xstep * (series.length + 1);
    const yH = yfor(data.next_game_high), yL = yfor(data.next_game_low);
    const grad = ctx.createLinearGradient(0, yH, 0, yL);
    grad.addColorStop(0, 'rgba(255,107,26,0.35)');
    grad.addColorStop(1, 'rgba(56,189,248,0.10)');
    ctx.fillStyle = grad;
    ctx.fillRect(x - xstep * 0.4, yH, xstep * 0.8, yL - yH);
  }

  // mean line
  if (data.mean != null) {
    ctx.strokeStyle = border; ctx.setLineDash([4*dpr, 4*dpr]); ctx.beginPath();
    const ym = yfor(data.mean);
    ctx.moveTo(padL, ym); ctx.lineTo(w - padR, ym); ctx.stroke();
    ctx.setLineDash([]);
    ctx.fillStyle = muted;
    ctx.fillText('μ ' + data.mean.toFixed(1), w - padR - 46*dpr, ym - 4*dpr);
  }

  // raw series (thin)
  ctx.strokeStyle = muted; ctx.lineWidth = 1.4*dpr; ctx.globalAlpha = 0.7;
  ctx.beginPath();
  series.forEach((v, i) => {
    const x = padL + xstep * (i + 1), y = yfor(v);
    if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
  });
  ctx.stroke(); ctx.globalAlpha = 1;

  // smoothed (bold, accent) with glow
  if (smoothed.length === series.length) {
    ctx.shadowColor = accent; ctx.shadowBlur = 12*dpr;
    ctx.strokeStyle = accent; ctx.lineWidth = 2.5*dpr; ctx.beginPath();
    smoothed.forEach((v, i) => {
      const x = padL + xstep * (i + 1), y = yfor(v);
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    });
    ctx.stroke();
    ctx.shadowBlur = 0; ctx.lineWidth = 1*dpr;
  }

  // data points
  const hits = [];
  const games = data.games || data.log || [];
  series.forEach((v, i) => {
    const x = padL + xstep * (i + 1), y = yfor(v);
    ctx.fillStyle = accent2; ctx.beginPath(); ctx.arc(x, y, 3.2*dpr, 0, Math.PI * 2); ctx.fill();
    hits.push({ x, y, v, i, game: games[i] || null });
  });
  _chartHits.set(canvasId, { hits, dpr });

  // next game projected point
  if (data.next_game_point != null) {
    const x = padL + xstep * (series.length + 1), y = yfor(data.next_game_point);
    ctx.fillStyle = accent;
    ctx.shadowColor = accent; ctx.shadowBlur = 14*dpr;
    ctx.beginPath(); ctx.arc(x, y, 6*dpr, 0, Math.PI * 2); ctx.fill();
    ctx.shadowBlur = 0;
    ctx.fillStyle = text; ctx.font = `bold ${12*dpr}px system-ui, sans-serif`;
    ctx.fillText(data.next_game_point.toFixed(1), x - 14*dpr, y - 10*dpr);
  }

  // anomalies ringed red
  (data.anomalies || []).forEach(a => {
    const x = padL + xstep * (a.index + 1), y = yfor(a.value);
    ctx.strokeStyle = bad; ctx.lineWidth = 1.6*dpr;
    ctx.beginPath(); ctx.arc(x, y, 8*dpr, 0, Math.PI * 2); ctx.stroke();
  });

  // Wire up click + hover once per canvas
  if (!canvas._hitWired) {
    canvas._hitWired = true;
    canvas.style.cursor = 'crosshair';
    const findHit = (ev) => {
      const rec = _chartHits.get(canvas.id);
      if (!rec) return null;
      const rect = canvas.getBoundingClientRect();
      const px = (ev.clientX - rect.left) * (canvas.width  / rect.width);
      const py = (ev.clientY - rect.top)  * (canvas.height / rect.height);
      let best = null, bestD = Infinity;
      for (const h of rec.hits) {
        const d = (h.x - px) ** 2 + (h.y - py) ** 2;
        if (d < bestD) { bestD = d; best = h; }
      }
      const maxR = 18 * rec.dpr;
      return best && bestD <= maxR * maxR ? best : null;
    };
    const statLabel = () => {
      const sel = document.getElementById('proj-stat');
      const opt = sel && sel.options[sel.selectedIndex];
      return opt ? opt.textContent.trim() : 'value';
    };
    canvas.addEventListener('mousemove', (ev) => {
      canvas.style.cursor = findHit(ev) ? 'pointer' : 'crosshair';
    });
    canvas.addEventListener('click', (ev) => {
      const hit = findHit(ev);
      const pop = document.getElementById('chart-pop');
      if (!hit) { if (pop) pop.classList.remove('show'); return; }
      const g = hit.game || {};
      let pop2 = pop;
      if (!pop2) {
        pop2 = document.createElement('div');
        pop2.id = 'chart-pop';
        pop2.className = 'chart-pop';
        document.body.appendChild(pop2);
      }
      const when = g.date || g.game_date || '';
      const opp = g.opp || g.opponent || g.matchup || '';
      const res = g.result || (g.win === true ? 'W' : g.win === false ? 'L' : '');
      const line = [
        g.min != null ? g.min + ' MIN' : null,
        g.pts != null ? g.pts + ' PTS' : null,
        g.reb != null ? g.reb + ' REB' : null,
        g.ast != null ? g.ast + ' AST' : null,
        g.stl != null ? g.stl + ' STL' : null,
        g.blk != null ? g.blk + ' BLK' : null,
        g.to  != null ? g.to  + ' TO'  : null,
        g.fgm != null && g.fga != null ? `FG ${g.fgm}/${g.fga}` : null,
        g.fg3m != null && g.fg3a != null ? `3P ${g.fg3m}/${g.fg3a}` : null,
        g.ftm != null && g.fta != null ? `FT ${g.ftm}/${g.fta}` : null,
      ].filter(Boolean).join(' · ');
      pop2.innerHTML = `
        <div class="chart-pop-head">
          <span class="chart-pop-when">${escHtml(when)}</span>
          ${opp ? `<span class="chart-pop-opp">${escHtml(opp)}</span>` : ''}
          ${res ? `<span class="chart-pop-res ${res.startsWith('W')?'win':res.startsWith('L')?'loss':''}">${escHtml(res)}</span>` : ''}
        </div>
        <div class="chart-pop-value"><b>${hit.v.toFixed(1)}</b> <span class="muted">${escHtml(statLabel())}</span></div>
        <div class="chart-pop-line">${escHtml(line) || '<span class="muted">no box score available</span>'}</div>`;
      pop2.style.left = (ev.clientX + 14) + 'px';
      pop2.style.top  = (ev.clientY + 14) + 'px';
      pop2.classList.add('show');
    });
    document.addEventListener('click', (ev) => {
      if (ev.target === canvas) return;
      const pop = document.getElementById('chart-pop');
      if (pop) pop.classList.remove('show');
    });
  }
}

// ---------- compare ----------
// Resolve whatever's in the input to a player id. Prefer the id stashed by the
// autocomplete picker; if the user just typed a name and hit Compare, fall
// back to /api/search and take the top hit.
async function resolvePlayerId(inputEl) {
  const raw = (inputEl.value || '').trim();
  if (!raw) return null;
  if (inputEl.dataset.playerId && (inputEl.dataset.lastResolveText === raw)) {
    return { id: inputEl.dataset.playerId, name: raw };
  }
  if (/^\d+$/.test(raw)) return { id: raw, name: raw };
  try {
    const d = await api('/api/search?q=' + encodeURIComponent(raw));
    const hit = (d.players || d.results || [])[0];
    if (hit) {
      const id = hit.id || hit.player_id;
      inputEl.dataset.playerId = id;
      inputEl.dataset.lastResolveText = raw;
      return { id, name: hit.name || raw };
    }
  } catch {}
  return null;
}

document.getElementById('cmp-go').addEventListener('click', async () => {
  const aEl = document.getElementById('cmp-a');
  const bEl = document.getElementById('cmp-b');
  const stat = document.getElementById('cmp-stat').value;
  const out = document.getElementById('cmp-out');
  out.innerHTML = '<div class="skeleton" style="height:80px"></div>';
  const [ra, rb] = await Promise.all([resolvePlayerId(aEl), resolvePlayerId(bEl)]);
  if (!ra || !rb) { out.innerHTML = '<div class="muted">pick two players — start typing a name and choose from the dropdown.</div>'; return; }
  const a = ra.id, b = rb.id;
  try {
    const [pa, pb] = await Promise.all([
      api('/api/players/' + encodeURIComponent(a)).catch(() => null),
      api('/api/players/' + encodeURIComponent(b)).catch(() => null),
    ]);
    if (!pa || !pb) { out.innerHTML = '<div class="muted">could not load one or both players</div>'; return; }
    const tA = pa.traditional || {}, tB = pb.traditional || {};
    const row = (lbl, av, bv, fmt = v => v.toFixed(1)) => {
      const a2 = typeof av === 'number' ? fmt(av) : (av ?? '—');
      const b2 = typeof bv === 'number' ? fmt(bv) : (bv ?? '—');
      const lean = (typeof av === 'number' && typeof bv === 'number') ? (av > bv ? 'a' : av < bv ? 'b' : '') : '';
      return `<tr>
        <td class="${lean === 'a' ? 'win' : ''}">${a2}</td>
        <td class="lbl">${lbl}</td>
        <td class="${lean === 'b' ? 'win' : ''}">${b2}</td>
      </tr>`;
    };
    const head = (p) => `
      <div class="cmp-head" style="--face:url('${faceOf(p)}'); --logo:url('${teamLogo(p.team)}')">
        <div class="avatar"></div>
        <div class="meta">
          <div class="name">${escHtml(p.full_name || p.name)}</div>
          <div class="sub">${escHtml([p.team, p.position, p.height_display].filter(x=>x).join(' · '))}</div>
        </div>
      </div>`;
    out.innerHTML = `
      <div class="cmp-grid">
        ${head(pa)}
        <div></div>
        ${head(pb)}
      </div>
      <table class="cmp-table">
        ${row('PTS', tA.pts, tB.pts)}
        ${row('REB', tA.reb, tB.reb)}
        ${row('AST', tA.ast, tB.ast)}
        ${row('STL', tA.stl, tB.stl)}
        ${row('BLK', tA.blk, tB.blk)}
        ${row('TOV', tA.tov, tB.tov)}
        ${row('FG%', tA.fg_pct, tB.fg_pct)}
        ${row('3P%', tA.fg3_pct, tB.fg3_pct)}
        ${row('FT%', tA.ft_pct, tB.ft_pct)}
        ${row('MIN', tA.min, tB.min)}
        ${row('GP',  tA.gp,  tB.gp, v => v.toFixed(0))}
      </table>`;
  } catch (e) { out.innerHTML = `<div class="muted">${escHtml(e.message)}</div>`; }
});

// ---------- name autocomplete ----------
// Attach to any <input> so users can type a name and pick from a dropdown
// without ever needing to know a player id.
function attachNameSearch(inputEl, onPick) {
  if (!inputEl || inputEl.dataset.nameSearchWired === '1') return;
  inputEl.dataset.nameSearchWired = '1';
  inputEl.setAttribute('autocomplete', 'off');

  const wrap = document.createElement('div');
  wrap.className = 'namesearch-wrap';
  inputEl.parentNode.insertBefore(wrap, inputEl);
  wrap.appendChild(inputEl);
  const dd = document.createElement('div');
  dd.className = 'namesearch-dd';
  dd.hidden = true;
  wrap.appendChild(dd);

  let timer = null;
  let lastQ = '';
  const hide = () => { dd.hidden = true; dd.innerHTML = ''; };
  const render = (hits) => {
    if (!hits || !hits.length) { hide(); return; }
    dd.innerHTML = hits.slice(0, 8).map(h => {
      const id = h.id || h.player_id;
      return `
        <div class="ns-row" data-id="${escHtml(id)}" data-name="${escHtml(h.name || h.displayName || '')}">
          <img class="ns-face" src="${headshot(id)}" loading="lazy" onerror="this.style.visibility='hidden'">
          <div class="ns-meta">
            <div class="ns-name">${escHtml(h.name || h.displayName || id)}</div>
            <div class="ns-sub">${escHtml(h.team || '')}${h.position ? ' · ' + escHtml(h.position) : ''}</div>
          </div>
        </div>`;
    }).join('');
    dd.hidden = false;
    dd.querySelectorAll('.ns-row').forEach(row => {
      row.addEventListener('mousedown', (e) => {
        e.preventDefault();
        const id = row.dataset.id;
        inputEl.value = row.dataset.name || id;        inputEl.dataset.playerId = id;        hide();
        if (onPick) onPick(id, row.dataset.name);
      });
    });
  };
  const search = async (q) => {
    if (q === lastQ) return;
    lastQ = q;
    if (q.length < 2) { hide(); return; }
    try {
      const data = await api('/api/search?q=' + encodeURIComponent(q));
      const hits = data.players || data.results || [];
      render(hits);
    } catch { hide(); }
  };
  inputEl.addEventListener('input', () => {
    clearTimeout(timer);
    timer = setTimeout(() => search(inputEl.value.trim()), 180);
  });
  inputEl.addEventListener('focus', () => {
    if (lastQ && inputEl.value.trim() === lastQ) dd.hidden = false;
  });
  inputEl.addEventListener('blur', () => setTimeout(hide, 120));
  inputEl.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') hide();
  });
}

// wire name search on the player + compare inputs (they'll load as soon as you pick)
attachNameSearch(document.getElementById('player-id'), (id) => loadPlayer(id));
attachNameSearch(document.getElementById('cmp-a'), () => {});
attachNameSearch(document.getElementById('cmp-b'), () => {});

// ---------- boot ----------
refreshLeaders();
refreshLive();
refreshPinned();
setInterval(() => { try { refreshLive(); } catch {} }, 8000);
setInterval(() => { try { refreshLeaders(); } catch {} }, 60000);
