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

async function api(path) {
  const r = await fetch(path, { headers: { accept: 'application/json' } });
  if (!r.ok) throw new Error(`${path} -> ${r.status}`);
  return await r.json();
}
const escHtml = s => String(s ?? '').replace(/[&<>"']/g, c => ({
  '&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'
}[c]));
const onImgFail = `this.style.visibility='hidden'`;

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
      <div class="leader-row" data-player-id="${escHtml(r.id)}" style="--face:url('${face}'); --logo:url('${logo}')">
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
    openPlayer(id);
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
      openPlayer(row.dataset.playerId);
    }));
  } catch {
    el.innerHTML = '<div class="muted">pinned unavailable</div>';
  }
}

// ---------- player tab ----------
function openPlayer(id) {
  if (!id) return;
  document.querySelector('nav button[data-tab="player"]').click();
  const inp = document.getElementById('player-id');
  if (inp) inp.value = id;
  loadPlayer(id);
}

async function loadPlayer(id) {
  const out = document.getElementById('player-out');
  out.innerHTML = '<div class="skeleton" style="height:100px"></div>';
  try {
    const p = await api('/api/players/' + encodeURIComponent(id));
    const face = faceOf({ id, ...p });
    const logo = teamLogo(p.team);
    const t = p.traditional || {};
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

    // --- bio grid: EVERYTHING we know about this player ---
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

    const pinned = pinnedIds().includes(String(id));
    const shortSub = [p.team, p.position, p.jersey ? '#' + p.jersey : '', p.height_display, p.weight_display]
      .filter(x => x).join(' · ');

    // --- inline mini-projection: pick a stat, show band ---
    const miniPred = `
      <div class="mini-pred">
        <div class="mini-pred-head">
          <span>Next-game projection</span>
          <select id="mini-pred-stat">
            <option>PTS</option><option>REB</option><option>AST</option>
            <option>PRA</option><option>PR</option><option>PA</option><option>RA</option>
            <option>STL</option><option>BLK</option>
          </select>
          <button class="ghost" id="mini-pred-open">open full predictor →</button>
        </div>
        <div id="mini-pred-body"><div class="muted">loading…</div></div>
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
      ${miniPred}
      ${renderGameLog(p.game_log || [])}
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
    // wire up mini predictor
    const miniSel = document.getElementById('mini-pred-stat');
    const miniBody = document.getElementById('mini-pred-body');
    const runMini = async () => {
      miniBody.innerHTML = '<div class="muted">projecting…</div>';
      try {
        const d = await api(`/api/predict/${encodeURIComponent(id)}?stat=${miniSel.value}&garbage=exclude`);
        miniBody.innerHTML = renderProjectionChips(d, miniSel.value);
      } catch (e) {
        miniBody.innerHTML = `<div class="muted">not enough game log to project</div>`;
      }
    };
    miniSel.addEventListener('change', runMini);
    document.getElementById('mini-pred-open').addEventListener('click', () => {
      document.querySelector('nav button[data-tab="predict"]').click();
      const inp = document.getElementById('pred-id'); if (inp) inp.value = id;
      document.getElementById('pred-stat').value = miniSel.value;
      const go = document.getElementById('pred-go'); if (go) go.click();
    });
    runMini();
  } catch (e) {
    out.innerHTML = `<div class="muted">could not load player ${escHtml(id)}: ${escHtml(e.message)}</div>`;
  }
}

// ------- projection chips (shared by mini predictor + main predict tab) -------
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
  const trend = (l5Avg - l10Avg).toFixed(1);
  const trendArrow = l5Avg > l10Avg + 0.1 ? '▲' : (l5Avg < l10Avg - 0.1 ? '▼' : '→');
  // hit-rate over a common threshold
  const threshold = pt ? Math.round(pt - 0.5) : Math.round(mean || 0);
  const overs = l10.filter(v => v >= threshold + 0.5).length;
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
        <div class="range ${l5Avg > l10Avg ? 'up' : (l5Avg < l10Avg ? 'down' : '')}">${trendArrow} ${trend >= 0 ? '+' : ''}${trend}</div>
      </div>
      <div class="chip-card">
        <div class="lbl">season mean</div>
        <div class="val">${mean.toFixed(1)}</div>
        <div class="range">σ ${sd.toFixed(2)}</div>
      </div>
      <div class="chip-card">
        <div class="lbl">over ${threshold + 0.5}</div>
        <div class="val">${overs}/${l10.length}</div>
        <div class="range">L10 hit rate</div>
      </div>
    </div>`;
}

function renderGameLog(games) {
  if (!games.length) return '';
  const rows = games.slice(0, 10).map(g => `
    <tr>
      <td>${escHtml(g.game_date || '').slice(0,10)}</td>
      <td>${g.venue === 'away' ? '@' : 'vs'} ${escHtml(g.opponent || '')}</td>
      <td>${escHtml(g.result || '')} ${escHtml(g.score || '')}</td>
      <td style="font-variant-numeric:tabular-nums">${(g.minutes||0).toFixed(0)}</td>
      <td style="font-variant-numeric:tabular-nums"><b>${g.pts ?? '—'}</b></td>
      <td style="font-variant-numeric:tabular-nums">${g.reb ?? '—'}</td>
      <td style="font-variant-numeric:tabular-nums">${g.ast ?? '—'}</td>
      <td style="font-variant-numeric:tabular-nums">${g.fgm ?? ''}-${g.fga ?? ''}</td>
      <td style="font-variant-numeric:tabular-nums">${g.fg3m ?? ''}-${g.fg3a ?? ''}</td>
    </tr>`).join('');
  return `
    <h3 style="margin:12px 0 6px;font-size:13px;letter-spacing:.5px;color:var(--muted)">LAST 10 GAMES</h3>
    <div style="overflow-x:auto">
    <table style="width:100%;border-collapse:collapse;font-size:12px">
      <thead><tr style="color:var(--muted);text-align:left">
        <th style="padding:4px 6px">Date</th><th>Opp</th><th>Result</th>
        <th>MIN</th><th>PTS</th><th>REB</th><th>AST</th><th>FG</th><th>3P</th>
      </tr></thead>
      <tbody>${rows}</tbody>
    </table></div>`;
}

document.getElementById('player-load').addEventListener('click', () => {
  const id = document.getElementById('player-id').value.trim();
  if (id) loadPlayer(id);
});
document.getElementById('player-id').addEventListener('keydown', e => {
  if (e.key === 'Enter') document.getElementById('player-load').click();
});

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
    const id = `${chip.dataset.abbr}-${SEASON}`;
    const inp = document.getElementById('team-id');
    if (inp) inp.value = id;
    loadTeam(id);
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
      <div class="roster-card" data-player-id="${escHtml(pl.id)}" style="--face:url('${pl.headshot || headshot(pl.id)}')">
        <div class="face"></div>
        <div class="info">
          <div class="nm">${escHtml(pl.name)}</div>
          <div class="meta">${escHtml(pl.position || '')} ${pl.jersey ? '· #'+escHtml(pl.jersey) : ''}</div>
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
      openPlayer(rc.dataset.playerId);
    }));
  } catch (e) {
    out.innerHTML = `<div class="muted">could not load team ${escHtml(id)}: ${escHtml(e.message)}</div>`;
  }
}

document.getElementById('team-load').addEventListener('click', () => {
  const id = document.getElementById('team-id').value.trim();
  if (id) loadTeam(id);
});
document.getElementById('team-id').addEventListener('keydown', e => {
  if (e.key === 'Enter') document.getElementById('team-load').click();
});

// ---------- predict ----------
const predBtn = document.getElementById('pred-go') || (() => {
  const b = document.createElement('button');
  b.id = 'pred-go';
  b.textContent = 'Project';
  b.className = 'primary';
  b.style.marginTop = '6px';
  document.querySelector('section[data-panel="predict"] .card').appendChild(b);
  return b;
})();
predBtn.addEventListener('click', async () => {
  const id = document.getElementById('pred-id').value.trim();
  if (!id) return;
  const params = new URLSearchParams({
    stat: document.getElementById('pred-stat').value,
    min: document.getElementById('pred-min').value,
    garbage: document.getElementById('pred-garbage').checked ? 'exclude' : 'include',
    opp_drtg: document.getElementById('pred-opp').value,
    rest: document.getElementById('pred-rest').value,
  });
  try {
    const data = await api(`/api/predict/${encodeURIComponent(id)}?${params}`);
    drawProjection(data);
    const sum = document.getElementById('pred-summary');
    sum.innerHTML = renderProjectionChips(data, document.getElementById('pred-stat').value);
  } catch (e) {
    document.getElementById('pred-summary').innerHTML = `<div class="muted">could not project: ${escHtml(e.message)}</div>`;
  }
});

function drawProjection(data) {
  const canvas = document.getElementById('pred-chart');
  const ctx = canvas.getContext('2d');
  const cs = getComputedStyle(document.body);
  const accent = cs.getPropertyValue('--accent').trim() || '#f97316';
  const accent2 = cs.getPropertyValue('--accent-2').trim() || '#38bdf8';
  const border = cs.getPropertyValue('--border').trim() || '#30363d';
  const muted = cs.getPropertyValue('--muted').trim() || '#8d96a0';
  const bad = cs.getPropertyValue('--bad').trim() || '#f85149';

  const w = canvas.width, h = canvas.height;
  ctx.clearRect(0, 0, w, h);
  const series = data.series || []; const smoothed = data.smoothed || [];
  if (!series.length) return;
  const padL = 40, padR = 20, padT = 18, padB = 30;
  const innerW = w - padL - padR, innerH = h - padT - padB;
  const max = Math.max(...series, ...smoothed, data.next_game_high || 0) * 1.1 + 1;
  const min = Math.min(0, ...series, ...smoothed, data.next_game_low || 0);
  const xstep = innerW / (series.length + 1);
  const yfor = v => padT + innerH - ((v - min) / (max - min)) * innerH;

  ctx.strokeStyle = border; ctx.beginPath();
  for (let i = 0; i <= 4; i++) {
    const y = padT + (innerH / 4) * i;
    ctx.moveTo(padL, y); ctx.lineTo(w - padR, y);
  }
  ctx.stroke();

  if (data.next_game_low != null && data.next_game_high != null) {
    ctx.fillStyle = 'rgba(56,189,248,0.18)';
    const x = padL + xstep * (series.length + 1);
    ctx.fillRect(x - xstep * 0.4, yfor(data.next_game_high), xstep * 0.8, yfor(data.next_game_low) - yfor(data.next_game_high));
  }

  ctx.strokeStyle = muted; ctx.beginPath();
  series.forEach((v, i) => {
    const x = padL + xstep * (i + 1), y = yfor(v);
    if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
  });
  ctx.stroke();

  if (smoothed.length === series.length) {
    ctx.strokeStyle = accent; ctx.lineWidth = 2; ctx.beginPath();
    smoothed.forEach((v, i) => {
      const x = padL + xstep * (i + 1), y = yfor(v);
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    });
    ctx.stroke(); ctx.lineWidth = 1;
  }

  series.forEach((v, i) => {
    const x = padL + xstep * (i + 1), y = yfor(v);
    ctx.fillStyle = accent2; ctx.beginPath(); ctx.arc(x, y, 3, 0, Math.PI * 2); ctx.fill();
  });
  if (data.next_game_point != null) {
    const x = padL + xstep * (series.length + 1), y = yfor(data.next_game_point);
    ctx.fillStyle = accent; ctx.beginPath(); ctx.arc(x, y, 5, 0, Math.PI * 2); ctx.fill();
  }
  (data.anomalies || []).forEach(a => {
    const x = padL + xstep * (a.index + 1), y = yfor(a.value);
    ctx.strokeStyle = bad; ctx.beginPath(); ctx.arc(x, y, 7, 0, Math.PI * 2); ctx.stroke();
  });
}

// ---------- compare ----------
document.getElementById('cmp-go').addEventListener('click', async () => {
  const a = document.getElementById('cmp-a').value.trim();
  const b = document.getElementById('cmp-b').value.trim();
  const stat = document.getElementById('cmp-stat').value;
  if (!a || !b) return;
  const out = document.getElementById('cmp-out');
  out.textContent = '…';
  try {
    const data = await api(`/api/compare?a=${encodeURIComponent(a)}&b=${encodeURIComponent(b)}&stat=${stat}`);
    out.textContent = JSON.stringify(data, null, 2);
  } catch (e) { out.textContent = e.message; }
});

// ---------- search ----------
async function doSearch() {
  const q = document.getElementById('search-q').value.trim();
  if (!q) return;
  const out = document.getElementById('search-out');
  out.innerHTML = '<div class="skeleton"></div>';
  try {
    const data = await api('/api/search?q=' + encodeURIComponent(q));
    const hits = data.players || data.results || [];
    if (!hits.length && data.answer) {
      out.innerHTML = `<div class="card" style="margin:0">${escHtml(data.answer)}</div>`;
      return;
    }
    if (!hits.length) {
      out.innerHTML = '<div class="muted">no matches</div>';
      return;
    }
    out.innerHTML = '<div class="search-results">' + hits.slice(0, 25).map(h => {
      const id = h.id || h.player_id;
      return `
        <div class="search-row" data-player-id="${escHtml(id)}" style="--face:url('${headshot(id)}')">
          <div class="face"></div>
          <div>
            <div style="font-weight:600">${escHtml(h.name || h.displayName || id)}</div>
            <div class="muted" style="font-size:11px">${escHtml(h.team || '')} ${h.position ? '· ' + escHtml(h.position) : ''}</div>
          </div>
          <button class="primary" data-open="${escHtml(id)}">Open</button>
        </div>`;
    }).join('') + '</div>';
    out.querySelectorAll('[data-open]').forEach(b => b.addEventListener('click', (ev) => {
      ev.stopPropagation();
      openPlayer(b.dataset.open);
    }));
    out.querySelectorAll('.search-row').forEach(r => r.addEventListener('click', () => {
      openPlayer(r.dataset.playerId);
    }));
  } catch (e) { out.innerHTML = `<div class="muted">${escHtml(e.message)}</div>`; }
}
document.getElementById('search-go').addEventListener('click', doSearch);
document.getElementById('search-q').addEventListener('keydown', e => {
  if (e.key === 'Enter') doSearch();
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
        inputEl.value = row.dataset.name || id;
        hide();
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

// wire name search on the player + predict inputs (they'll load as soon as you pick)
attachNameSearch(document.getElementById('player-id'), (id) => loadPlayer(id));
attachNameSearch(document.getElementById('pred-id'),   (id) => {
  const go = document.getElementById('pred-go'); if (go) go.click();
});
attachNameSearch(document.getElementById('cmp-a'), () => {});
attachNameSearch(document.getElementById('cmp-b'), () => {});

// ---------- boot ----------
refreshLeaders();
refreshLive();
refreshPinned();
setInterval(() => { try { refreshLive(); } catch {} }, 8000);
setInterval(() => { try { refreshLeaders(); } catch {} }, 60000);
