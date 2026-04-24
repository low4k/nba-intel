console.log('[nba-intel] app.v3.js loaded at', new Date().toISOString());
try {
  const dl = document.getElementById('dash-live');
  const dldrs = document.getElementById('dash-leaders');
  if (dl) dl.textContent = 'booting...';
  if (dldrs) dldrs.textContent = 'booting...';
} catch (_) {}

const tabs = document.querySelectorAll('nav button');
const panels = document.querySelectorAll('section[data-panel]');

tabs.forEach(b => b.addEventListener('click', () => {
  tabs.forEach(x => x.classList.remove('active'));
  panels.forEach(p => p.classList.remove('visible'));
  b.classList.add('active');
  document.querySelector(`section[data-panel="${b.dataset.tab}"]`).classList.add('visible');
}));

const themeBtn = document.getElementById('theme-toggle');
themeBtn.addEventListener('click', () => {
  const cur = document.body.dataset.theme;
  const next = cur === 'dark' ? 'light' : 'dark';
  document.body.dataset.theme = next;
  themeBtn.textContent = next === 'dark' ? 'light' : 'dark';
});

async function api(path) {
  const r = await fetch(path);
  return await r.json();
}

document.getElementById('player-load').addEventListener('click', async () => {
  const id = document.getElementById('player-id').value.trim();
  if (!id) return;
  const data = await api('/api/players/' + encodeURIComponent(id));
  document.getElementById('player-out').textContent = JSON.stringify(data, null, 2);
});

document.getElementById('team-load').addEventListener('click', async () => {
  const id = document.getElementById('team-id').value.trim();
  if (!id) return;
  const data = await api('/api/teams/' + encodeURIComponent(id));
  document.getElementById('team-out').textContent = JSON.stringify(data, null, 2);
});

document.getElementById('cmp-go').addEventListener('click', async () => {
  const a = document.getElementById('cmp-a').value.trim();
  const b = document.getElementById('cmp-b').value.trim();
  const stat = document.getElementById('cmp-stat').value;
  if (!a || !b) return;
  const data = await api(`/api/compare?a=${encodeURIComponent(a)}&b=${encodeURIComponent(b)}&stat=${stat}`);
  document.getElementById('cmp-out').textContent = JSON.stringify(data, null, 2);
});

document.getElementById('search-go').addEventListener('click', async () => {
  const q = document.getElementById('search-q').value.trim();
  if (!q) return;
  const data = await api('/api/search?q=' + encodeURIComponent(q));
  document.getElementById('search-out').textContent = JSON.stringify(data, null, 2);
});

const predBtn = document.createElement('button');
predBtn.textContent = 'Project';
predBtn.style.marginTop = '6px';
document.querySelector('section[data-panel="predict"] .card').appendChild(predBtn);

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
  const data = await api(`/api/predict/${encodeURIComponent(id)}?${params}`);
  drawProjection(data);
  document.getElementById('pred-summary').innerHTML =
    `samples=${data.samples} mean=${data.mean.toFixed(2)} sd=${data.stddev.toFixed(2)} ` +
    `next=${data.next_game_point ? data.next_game_point.toFixed(2) : '—'} ` +
    `band=[${data.next_game_low ? data.next_game_low.toFixed(2) : '—'}, ` +
    `${data.next_game_high ? data.next_game_high.toFixed(2) : '—'}] ` +
    `anomalies=${data.anomalies.length}`;
});

function drawProjection(data) {
  const canvas = document.getElementById('pred-chart');
  const ctx = canvas.getContext('2d');
  const w = canvas.width, h = canvas.height;
  ctx.clearRect(0, 0, w, h);
  const series = data.series || [];
  const smoothed = data.smoothed || [];
  if (series.length === 0) return;
  const padL = 40, padR = 20, padT = 18, padB = 30;
  const innerW = w - padL - padR;
  const innerH = h - padT - padB;
  const max = Math.max(...series, ...smoothed, data.next_game_high || 0) * 1.1 + 1;
  const min = Math.min(0, ...series, ...smoothed, data.next_game_low || 0);
  const xstep = innerW / (series.length + 1);
  const yfor = v => padT + innerH - ((v - min) / (max - min)) * innerH;

  ctx.strokeStyle = '#30363d';
  ctx.beginPath();
  for (let i = 0; i <= 4; i++) {
    const y = padT + (innerH / 4) * i;
    ctx.moveTo(padL, y); ctx.lineTo(w - padR, y);
  }
  ctx.stroke();

  if (data.next_game_low != null && data.next_game_high != null) {
    ctx.fillStyle = 'rgba(56,189,248,0.15)';
    const x = padL + xstep * (series.length + 1);
    ctx.fillRect(x - xstep * 0.4, yfor(data.next_game_high), xstep * 0.8, yfor(data.next_game_low) - yfor(data.next_game_high));
  }

  ctx.strokeStyle = '#8d96a0';
  ctx.beginPath();
  series.forEach((v, i) => {
    const x = padL + xstep * (i + 1);
    const y = yfor(v);
    if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
  });
  ctx.stroke();

  if (smoothed.length === series.length) {
    ctx.strokeStyle = '#f97316';
    ctx.lineWidth = 2;
    ctx.beginPath();
    smoothed.forEach((v, i) => {
      const x = padL + xstep * (i + 1);
      const y = yfor(v);
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    });
    ctx.stroke();
    ctx.lineWidth = 1;
  }

  series.forEach((v, i) => {
    const x = padL + xstep * (i + 1);
    const y = yfor(v);
    ctx.fillStyle = '#38bdf8';
    ctx.beginPath();
    ctx.arc(x, y, 3, 0, Math.PI * 2);
    ctx.fill();
  });

  if (data.next_game_point != null) {
    const x = padL + xstep * (series.length + 1);
    const y = yfor(data.next_game_point);
    ctx.fillStyle = '#f97316';
    ctx.beginPath();
    ctx.arc(x, y, 5, 0, Math.PI * 2);
    ctx.fill();
  }

  (data.anomalies || []).forEach(a => {
    const x = padL + xstep * (a.index + 1);
    const y = yfor(a.value);
    ctx.strokeStyle = '#f85149';
    ctx.beginPath();
    ctx.arc(x, y, 7, 0, Math.PI * 2);
    ctx.stroke();
  });
}

async function refreshLive() {
  try {
    const data = await api('/api/live');
    const games = Object.values(data.games || {});
    const html = games.length === 0
      ? '<div style="color:var(--muted)">no games right now</div>'
      : games.map(g => {
          const wp = typeof g.win_probability_home === 'number'
            ? `${(g.win_probability_home * 100).toFixed(1)}%` : '—';
          const pace = typeof g.pace_estimate === 'number'
            ? g.pace_estimate.toFixed(1) : '—';
          return `
          <div style="border:1px solid var(--border); border-radius:8px; padding:10px; margin:6px 0">
            <strong>${g.away.team} ${g.away.score} @ ${g.home.team} ${g.home.score}</strong>
            <div>${g.status} · Q${g.period} ${g.clock}</div>
            <div>win prob (home): ${wp} · pace est: ${pace}</div>
          </div>`;
        }).join('');
    document.getElementById('live-list').innerHTML = html;
    document.getElementById('dash-live').innerHTML = html;
  } catch (e) {
    const msg = '<div style="color:var(--muted)">live feed unavailable</div>';
    document.getElementById('live-list').innerHTML = msg;
    document.getElementById('dash-live').innerHTML = msg;
  }
}

function renderLeaderList(rows) {
  if (!rows || rows.length === 0) return '<div style="color:var(--muted)">—</div>';
  return rows.map(r => `
    <div style="display:flex; justify-content:space-between; gap:8px; padding:2px 0">
      <span>${r.name || r.id}${r.team ? ` <span style="color:var(--muted)">${r.team}</span>` : ''}</span>
      <strong>${typeof r.value === 'number' ? r.value.toFixed(1) : r.value}</strong>
    </div>`).join('');
}

async function refreshLeaders() {
  const el = document.getElementById('dash-leaders');
  try {
    const data = await api('/api/leaders');
    el.innerHTML = `
      <div style="display:grid; grid-template-columns:1fr 1fr 1fr; gap:14px">
        <div><div style="color:var(--muted); font-size:11px; letter-spacing:0.5px">PTS</div>${renderLeaderList(data.pts)}</div>
        <div><div style="color:var(--muted); font-size:11px; letter-spacing:0.5px">REB</div>${renderLeaderList(data.reb)}</div>
        <div><div style="color:var(--muted); font-size:11px; letter-spacing:0.5px">AST</div>${renderLeaderList(data.ast)}</div>
      </div>`;
  } catch (e) {
    el.innerHTML = '<div style="color:var(--muted)">leaders unavailable</div>';
  }
}

// Pinned players: localStorage-backed list of ids shown on the dashboard.
function pinnedIds() {
  try { return JSON.parse(localStorage.getItem('nba-pinned') || '[]'); }
  catch (_) { return []; }
}
function savePinned(ids) {
  localStorage.setItem('nba-pinned', JSON.stringify(ids));
}
async function refreshPinned() {
  const el = document.getElementById('dash-pinned');
  const ids = pinnedIds();
  if (ids.length === 0) {
    el.innerHTML = '<div style="color:var(--muted)">pin a player from the Player tab (click Pin after Load)</div>';
    return;
  }
  try {
    const rows = await Promise.all(ids.map(id =>
      api('/api/players/' + encodeURIComponent(id)).then(p => ({ id, p })).catch(() => ({ id, p: null }))));
    el.innerHTML = rows.map(({ id, p }) => {
      if (!p || !(p.name || '').length) return `
        <div style="display:flex; justify-content:space-between; gap:8px; padding:3px 0">
          <span style="color:var(--muted)">id ${id} (no data)</span>
          <button data-unpin="${id}" style="padding:1px 6px">×</button>
        </div>`;
      const t = p.traditional || {};
      return `
        <div style="display:flex; justify-content:space-between; gap:8px; padding:3px 0">
          <span>${p.name} <span style="color:var(--muted)">${p.team || ''}</span></span>
          <span>${(t.pts || 0).toFixed(1)} / ${(t.reb || 0).toFixed(1)} / ${(t.ast || 0).toFixed(1)}
            <button data-unpin="${id}" style="padding:1px 6px; margin-left:6px">×</button>
          </span>
        </div>`;
    }).join('');
    el.querySelectorAll('[data-unpin]').forEach(btn => btn.addEventListener('click', () => {
      const id = btn.dataset.unpin;
      savePinned(pinnedIds().filter(x => x !== id));
      refreshPinned();
    }));
  } catch (e) {
    el.innerHTML = '<div style="color:var(--muted)">pinned unavailable</div>';
  }
}

// Pin button injected into the Player tab.
(function addPinButton() {
  const playerCard = document.querySelector('section[data-panel="player"] .card');
  if (!playerCard) return;
  const pinBtn = document.createElement('button');
  pinBtn.textContent = 'Pin';
  pinBtn.style.marginLeft = '6px';
  playerCard.querySelector('#player-load').insertAdjacentElement('afterend', pinBtn);
  pinBtn.addEventListener('click', () => {
    const id = document.getElementById('player-id').value.trim();
    if (!id) return;
    const ids = pinnedIds();
    if (!ids.includes(id)) { ids.push(id); savePinned(ids); }
    refreshPinned();
  });
})();

// Kick off dashboard refreshes immediately — wrapped so later errors can't stop these.
try { refreshLive(); } catch (e) { console.error('refreshLive failed', e); }
try { refreshLeaders(); } catch (e) { console.error('refreshLeaders failed', e); }
try { refreshPinned(); } catch (e) { console.error('refreshPinned failed', e); }
setInterval(() => { try { refreshLive(); } catch (_) {} }, 8000);
setInterval(() => { try { refreshLeaders(); } catch (_) {} }, 60000);
