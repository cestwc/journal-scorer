'use strict';

// Criterion colours follow rubric order (pdf.md: blue, green, orange, purple, red).
const PALETTE = ['#2563eb', '#16a34a', '#ea580c', '#9333ea', '#dc2626', '#0891b2', '#ca8a04', '#db2777'];
const ALPHA = { weak: 0.16, moderate: 0.32, strong: 0.55 };
const RANK = { weak: 1, moderate: 2, strong: 3 };

const state = {
  rubric: null,
  subToCrit: new Map(),  // subitem id -> criterion id
  subInfo: new Map(),    // subitem id -> subitem object
  critColor: new Map(),  // criterion id -> colour
  report: null,
  text: '',              // exact text the report was computed from
  filter: null,          // criterion id or null
  selected: null,        // evidence id or null
  open: new Set(),       // criterion ids whose subitem list is expanded
  job: null,             // {id, name} of the PDF job behind the current report
  models: null,          // GET /api/models
  run: null,             // the scoring run in progress: {kind: 'text', abort} or {kind: 'pdf', id, name}
  timer: null,
};

const MODEL_KEY = 'journal-scorer.model';
const RECENT_KEY = 'journal-scorer.recent-models';
const ACTIVE_JOB_KEY = 'journal-scorer.active-job';
const RECENT_MAX = 5;
const POLL_MS = 1500;

/** localStorage that tolerates being unavailable. */
const store = {
  get(k, fallback) {
    try { const v = localStorage.getItem(k); return v == null ? fallback : JSON.parse(v); } catch { return fallback; }
  },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* ignore */ } },
  remove(k) { try { localStorage.removeItem(k); } catch { /* ignore */ } },
};

/** Stage label and the share of the progress bar each stage covers. */
const STAGES = {
  queued: { text: 'Waiting for the previous job', from: 0, to: 0 },
  extracting: { text: 'Reading the PDF', from: 0, to: 5 },
  describing_images: { text: 'Describing screenshots', from: 5, to: 30 },
  classifying: { text: 'Labelling evidence', from: 30, to: 95 },
  annotating: { text: 'Writing the annotated PDF', from: 95, to: 100 },
  done: { text: 'Done', from: 100, to: 100 },
};

const $ = (id) => document.getElementById(id);

/** Small DOM builder; children are text-escaped by construction. */
function el(tag, props = {}, ...children) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v == null || v === false) continue;
    if (k === 'class') n.className = v;
    else if (k === 'style') for (const [sk, sv] of Object.entries(v)) n.style.setProperty(sk, sv);
    else if (k === 'dataset') Object.assign(n.dataset, v);
    else if (k.startsWith('on')) n.addEventListener(k.slice(2), v);
    else n.setAttribute(k, v === true ? '' : v);
  }
  for (const c of children.flat(Infinity)) {
    if (c != null && c !== false) n.append(c instanceof Node ? c : String(c));
  }
  return n;
}

function rgba(hex, a) {
  const v = parseInt(hex.slice(1), 16);
  return `rgba(${(v >> 16) & 255}, ${(v >> 8) & 255}, ${v & 255}, ${a})`;
}
const fmt = (x, d = 2) => Number(x).toFixed(d);
const critOf = (sub) => state.subToCrit.get(sub);
const colorOf = (sub) => state.critColor.get(critOf(sub)) || '#888888';
const strongest = (sups) => sups.reduce((a, b) => (RANK[b.strength] > RANK[a.strength] ? b : a));
const where = (ev) => {
  const page = ev.span.regions && ev.span.regions[0] ? ` · p.${ev.span.regions[0].page}` : '';
  return `${page}${ev.span.kind === 'image' ? ' · screenshot' : ''}`;
};
const visibleSupports = (ev) =>
  state.filter ? ev.supports.filter((s) => critOf(s.subitem) === state.filter) : ev.supports;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---------------------------------------------------------------- startup

async function init() {
  wire();
  try {
    const [health, rubric] = await Promise.all([getJson('/api/health'), getJson('/api/rubric')]);
    setStatus('Connected', 'ok', health.provider.kind === 'opencode' ? 'Models come from OpenCode' : `Provider: ${health.provider.kind}`);
    loadRubric(rubric);
  } catch (e) {
    setStatus('Offline', 'bad', 'The scoring server is not reachable');
    showError(e.message);
    return;
  }
  try {
    loadModels(await getJson('/api/models'));
  } catch (e) {
    showError(`Could not list models: ${e.message}`);
  }
  resumeJob();
}

async function getJson(url, opts) {
  const r = await fetch(url, opts);
  let body = null;
  try { body = await r.json(); } catch { /* non-JSON body */ }
  if (!r.ok) {
    const err = new Error((body && body.error) || `HTTP ${r.status}`);
    err.status = r.status;
    throw err;
  }
  return body;
}

function loadRubric(r) {
  state.rubric = r;
  r.criteria.forEach((c, i) => {
    state.critColor.set(c.id, PALETTE[i % PALETTE.length]);
    for (const s of c.subitems) {
      state.subToCrit.set(s.id, c.id);
      state.subInfo.set(s.id, s);
    }
  });
}

function wire() {
  document.querySelectorAll('.tab').forEach((b) => b.addEventListener('click', () => showTab(b.dataset.tab)));
  $('score').addEventListener('click', runScore);
  $('cancel').addEventListener('click', cancelRun);
  $('input').addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') runScore();
  });
  $('input').addEventListener('input', updateHint);
  $('file').addEventListener('change', (e) => {
    const f = e.target.files[0];
    e.target.value = '';
    if (f) openFile(f);
  });
  $('download').addEventListener('click', downloadJson);
  $('model').addEventListener('change', () => {
    rememberModel($('model').value);
    renderModelOptions($('model').value);
  });
  // Drag and drop anywhere on the page.
  let depth = 0;
  const overlay = (on) => { $('drop-overlay').hidden = !on || !!state.run; };
  window.addEventListener('dragenter', (e) => { e.preventDefault(); depth++; overlay(true); });
  window.addEventListener('dragover', (e) => e.preventDefault());
  window.addEventListener('dragleave', () => { if (--depth <= 0) { depth = 0; overlay(false); } });
  window.addEventListener('drop', (e) => {
    e.preventDefault();
    depth = 0;
    overlay(false);
    const f = e.dataTransfer.files[0];
    if (f && !state.run) openFile(f);
  });
  updateHint();
}

// ---------------------------------------------------------------- models

function modelLabel(m) {
  return m.vision === false ? `${m.name} · text only` : m.name;
}

function modelTitle(m) {
  const parts = [m.spec];
  if (m.context) parts.push(`${Math.round(m.context / 1000)}K context`);
  if (m.cost) parts.push(`$${m.cost[0]} / $${m.cost[1]} per million tokens in / out`);
  if (m.vision === false) parts.push('Cannot read screenshots: they are skipped.');
  return parts.join('\n');
}

function loadModels(list) {
  state.models = list;
  const saved = store.get(MODEL_KEY, null);
  const known = list.models.some((m) => m.spec === saved);
  renderModelOptions(list.switchable && known ? saved : list.default);
  syncControls();
}

/** Default first, then recently used, then every model grouped by provider. */
function renderModelOptions(current) {
  const list = state.models;
  const known = new Map(list.models.map((m) => [m.spec, m]));
  const option = (m) => el('option', { value: m.spec, title: modelTitle(m) }, modelLabel(m));
  const recent = store.get(RECENT_KEY, []).filter((s) => known.has(s) && s !== list.default);
  const groups = [el('optgroup', { label: 'Default' }, option(known.get(list.default)))];
  if (recent.length) groups.push(el('optgroup', { label: 'Recently used' }, recent.map((s) => option(known.get(s)))));
  const byProvider = new Map();
  for (const m of list.models) {
    const p = m.spec.split('/')[0];
    if (!byProvider.has(p)) byProvider.set(p, []);
    byProvider.get(p).push(m);
  }
  for (const [p, ms] of byProvider) groups.push(el('optgroup', { label: `${p} (${ms.length})` }, ms.map(option)));
  $('model').replaceChildren(...groups);
  $('model').value = current;
  $('model').title = modelTitle(known.get(current) || { spec: current });
}

/** The chosen model, or undefined for the server default. */
function chosenModel() {
  const m = state.models;
  const v = $('model').value;
  return m && m.switchable && v && v !== m.default ? v : undefined;
}

function rememberModel(spec) {
  if (!state.models || !state.models.switchable) return;
  store.set(MODEL_KEY, spec);
  if (spec !== state.models.default) {
    store.set(RECENT_KEY, [spec, ...store.get(RECENT_KEY, []).filter((s) => s !== spec)].slice(0, RECENT_MAX));
  }
}

// ---------------------------------------------------------------- UI state

function setStatus(text, kind, title) {
  const s = $('status');
  s.textContent = text;
  s.className = `status ${kind}`;
  s.title = title || '';
}

function showTab(name) {
  document.querySelectorAll('.tab').forEach((b) => {
    b.classList.toggle('active', b.dataset.tab === name);
    b.setAttribute('aria-selected', b.dataset.tab === name);
  });
  $('edit-view').hidden = name !== 'edit';
  $('annotated-view').hidden = name !== 'annotated';
  $('pdf-view').hidden = name !== 'pdf';
}

function updateHint() {
  const t = $('input').value.trim();
  const words = t ? t.split(/\s+/).length : 0;
  $('input-hint').textContent = `${words} words · Ctrl+Enter to score`;
}

function showError(msg) {
  $('notice').hidden = true;
  $('error').textContent = msg;
  $('error').hidden = false;
}
function showNotice(msg) {
  $('error').hidden = true;
  $('notice').textContent = msg;
  $('notice').hidden = false;
}
function clearMessages() {
  $('error').hidden = true;
  $('notice').hidden = true;
}

/** Enables or disables inputs to match whether a run is in progress. */
function syncControls() {
  const busy = !!state.run;
  $('score').disabled = busy;
  $('file').disabled = busy;
  $('open-btn').classList.toggle('disabled', busy);
  $('model').disabled = busy || !(state.models && state.models.switchable);
}

/** Starts the busy panel for a new run. */
function beginRun(run, title, model) {
  state.run = run;
  clearMessages();
  syncControls();
  $('busy-title').textContent = title;
  $('busy-title').title = title;
  $('busy-model').textContent = model ? `Model: ${model}` : '';
  $('cancel').disabled = false;
  $('cancel').textContent = 'Cancel';
  setProgress(null, 'Starting…');
  $('busy').hidden = false;
  $('empty').hidden = true;
  $('results').hidden = true;
  const t0 = Date.now();
  const tick = () => {
    const s = Math.round((Date.now() - t0) / 1000);
    $('elapsed').textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  };
  tick();
  clearInterval(state.timer);
  state.timer = setInterval(tick, 1000);
}

/** Ends the current run (whatever the outcome) and restores the side panel. */
function endRun() {
  state.run = null;
  clearInterval(state.timer);
  store.remove(ACTIVE_JOB_KEY);
  $('busy').hidden = true;
  $('results').hidden = !state.report;
  $('empty').hidden = !!state.report;
  syncControls();
}

/** `percent` null = indeterminate. */
function setProgress(percent, text) {
  const fill = $('progress-fill');
  fill.classList.toggle('indeterminate', percent == null);
  fill.style.width = percent == null ? '' : `${Math.max(2, percent)}%`;
  $('busy-stage').textContent = text;
}

function showJobProgress(p) {
  const st = STAGES[p.stage] || { text: p.stage, from: 0, to: 0 };
  const share = p.total ? Math.min(p.done, p.total) / p.total : 0;
  const count = p.total ? ` · ${Math.min(p.done + 1, p.total)} of ${p.total}` : '';
  setProgress(p.stage === 'queued' ? null : st.from + (st.to - st.from) * share, `${st.text}${count}`);
}

async function openFile(f) {
  if (state.run) return;
  clearMessages();
  if (/\.pdf$/i.test(f.name) || f.type === 'application/pdf') {
    runPdf(f);
    return;
  }
  $('input').value = await f.text();
  updateHint();
  showTab('edit');
}

// ---------------------------------------------------------------- scoring

async function runScore() {
  if (state.run) return;
  const text = $('input').value;
  if (!text.trim()) { showError('Paste some text first.'); return; }
  const abort = new AbortController();
  const model = chosenModel();
  beginRun({ kind: 'text', abort }, 'Scoring text', model || state.models?.default);
  setProgress(null, 'Labelling evidence');
  try {
    const report = await getJson('/api/score', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ text, include_spans: true, model }),
      signal: abort.signal,
    });
    showReport(report, text, null);
  } catch (e) {
    if (e.name === 'AbortError') showNotice('Scoring cancelled.');
    else showError(`Scoring failed: ${e.message}`);
  } finally {
    endRun();
  }
}

/** Uploads a PDF and follows its job until it finishes. */
async function runPdf(file) {
  const model = chosenModel();
  const run = { kind: 'pdf', id: null, name: file.name };
  beginRun(run, file.name, model || state.models?.default);
  setProgress(null, 'Uploading…');
  try {
    const form = new FormData();
    if (model) form.append('model', model);
    form.append('file', file);
    const { id } = await getJson('/api/jobs', { method: 'POST', body: form });
    run.id = id;
    store.set(ACTIVE_JOB_KEY, { id, name: file.name });
    if (run.cancelled) await cancelJob(id); // Cancel pressed during upload.
    await followJob(run);
  } catch (e) {
    showError(`Scoring ${file.name} failed: ${e.message}`);
  } finally {
    if (state.run === run) endRun();
  }
}

/** Re-attaches to a PDF job that was running when the page was closed. */
async function resumeJob() {
  const saved = store.get(ACTIVE_JOB_KEY, null);
  if (!saved || !saved.id) return;
  let job;
  try {
    job = await getJson(`/api/jobs/${saved.id}`);
  } catch {
    store.remove(ACTIVE_JOB_KEY); // Server restarted or job evicted.
    return;
  }
  const run = { kind: 'pdf', id: saved.id, name: job.file_name };
  beginRun(run, job.file_name, job.model);
  try {
    await followJob(run);
  } catch (e) {
    showError(`Scoring ${job.file_name} failed: ${e.message}`);
  } finally {
    if (state.run === run) endRun();
  }
}

/** Polls a job until it is done, failed or cancelled. */
async function followJob(run) {
  for (;;) {
    const job = await getJson(`/api/jobs/${run.id}`);
    if (state.run !== run) return;
    $('busy-model').textContent = `Model: ${job.model}`;
    const p = job.progress;
    if (p.stage === 'failed') throw new Error(job.error || 'job failed');
    if (p.stage === 'cancelled') { showNotice(`Cancelled ${run.name}.`); return; }
    if (p.stage === 'done' && job.report) {
      showReport(job.report, job.report.source_text || '', { id: run.id, name: run.name });
      return;
    }
    showJobProgress(p);
    await sleep(POLL_MS);
  }
}

async function cancelRun() {
  const run = state.run;
  if (!run) return;
  $('cancel').disabled = true;
  $('cancel').textContent = 'Cancelling…';
  if (run.kind === 'text') {
    run.abort.abort();
    return;
  }
  run.cancelled = true;
  if (!run.id) return; // Still uploading; cancelled once the job id is known.
  try {
    await cancelJob(run.id);
  } catch (e) {
    showError(`Could not cancel: ${e.message}`);
    $('cancel').disabled = false;
    $('cancel').textContent = 'Cancel';
  }
}

async function cancelJob(id) {
  try {
    await getJson(`/api/jobs/${id}/cancel`, { method: 'POST' });
  } catch (e) {
    if (e.status !== 409) throw e; // 409: finished meanwhile; polling shows the outcome.
  }
}

function showReport(report, text, job) {
  Object.assign(state, { report, text, job, filter: null, selected: null });
  $('annotated-tab').disabled = false;
  $('pdf-tab').hidden = !job;
  if (job) $('pdf-frame').src = `/api/jobs/${job.id}/annotated.pdf`;
  else $('pdf-frame').removeAttribute('src');
  renderAll();
  showTab(job ? 'pdf' : 'annotated');
}

function downloadJson() {
  const blob = new Blob([JSON.stringify(state.report, null, 2)], { type: 'application/json' });
  const stem = state.job ? state.job.name.replace(/\.pdf$/i, '') : 'text';
  const a = el('a', { href: URL.createObjectURL(blob), download: `${stem}-score.json` });
  a.click();
  URL.revokeObjectURL(a.href);
}

function toggleFilter(critId) {
  state.filter = state.filter === critId ? null : critId;
  renderAll();
}

function select(evId, scroll = true) {
  state.selected = state.selected === evId ? null : evId;
  renderDoc();
  renderEvidence();
  renderDetail();
  if (state.selected && scroll) {
    showTab('annotated');
    document.querySelector(`mark[data-ev="${evId}"]`)?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  }
}

// ---------------------------------------------------------------- rendering

function renderAll() {
  renderSummary();
  renderLegend();
  renderDoc();
  renderDetail();
  renderEvidence();
}

function renderSummary() {
  const { score, stats, provider, warnings } = state.report;
  $('total').textContent = fmt(score.total);
  $('max-total').textContent = fmt(score.max_total, 0);
  const rows = [
    ['Source', state.job ? state.job.name : 'Pasted text'],
    ['Model', provider.model],
    ['Evidence', `${stats.evidence} of ${stats.spans} sentences${stats.figures ? `, ${stats.figures} screenshot(s)` : ''}`],
    ['Model calls', stats.llm_calls],
  ];
  $('meta').replaceChildren(...rows.flatMap(([k, v]) => [el('dt', {}, k), el('dd', { title: String(v) }, v)]));
  $('criteria').replaceChildren(...score.criteria.map(renderCriterion));

  $('warnings').hidden = warnings.length === 0;
  $('warnings').replaceChildren(
    el('b', {}, warnings.length === 1 ? '1 warning' : `${warnings.length} warnings`),
    el('ul', {}, warnings.map((w) => el('li', {}, w))),
  );
}

function renderCriterion(c) {
  const color = state.critColor.get(c.criterion);
  const active = state.filter === c.criterion;
  const cls = ['crit', active && 'active', state.filter && !active && 'dim'].filter(Boolean).join(' ');
  return el('div', { class: cls, style: { '--c': color } },
    el('button', { class: 'crit-head', title: 'Highlight only this criterion', onclick: () => toggleFilter(c.criterion) },
      el('span', { class: 'crit-name' }, c.name),
      el('span', { class: 'crit-score' }, fmt(c.score), el('span', { class: 'muted' }, ` / ${fmt(c.max, 0)}`))),
    el('div', { class: 'bar' }, el('div', { class: 'fill', style: { width: `${(100 * c.score) / c.max}%` } })),
    el('details', {
      open: state.open.has(c.criterion),
      ontoggle: (e) => (e.target.open ? state.open.add(c.criterion) : state.open.delete(c.criterion)),
    },
      el('summary', {}, 'Subitems and rationale'),
      el('div', { class: 'classes' }, c.classes.map(renderClass)),
      el('p', { class: 'rationale muted small' }, c.rationale)));
}

function renderClass(cl) {
  return el('div', { class: cl.contributions.length ? 'cls' : 'cls none' },
    el('div', { class: 'cls-row' },
      el('span', { class: 'code' }, cl.subitem),
      el('span', { class: 'cls-name' }, cl.name),
      el('span', { class: 'cls-score' }, fmt(cl.score))),
    el('div', { class: 'mini-bar' }, el('div', { class: 'fill', style: { width: `${100 * cl.score}%` } })),
    cl.contributions.length > 0 && el('div', { class: 'chips' }, cl.contributions.map((ct) =>
      el('button', { class: 'chip', onclick: () => select(ct.evidence_id) }, `${ct.evidence_id} · ${ct.strength}`))));
}

function renderLegend() {
  const items = state.rubric.criteria.map((c) => el('button', {
    class: `item${state.filter && state.filter !== c.id ? ' off' : ''}`,
    style: { '--c': state.critColor.get(c.id) },
    title: 'Show only this criterion',
    onclick: () => toggleFilter(c.id),
  }, el('span', { class: 'sw' }), `${c.id} ${c.name}`));
  const levels = Object.keys(ALPHA).map((k) => el('span', { class: 'lvl', style: { background: rgba('#6b7280', ALPHA[k]) } }, k));
  const showAll = state.filter && el('button', { class: 'link', onclick: () => toggleFilter(state.filter) }, 'Show all criteria');
  $('legend').replaceChildren(
    el('div', { class: 'legend-row' }, items),
    el('div', { class: 'legend-row' }, el('span', { class: 'legend-label' }, 'Strength'), levels, showAll),
  );
}

/** Rebuilds the original text, wrapping evidence spans. Span offsets are UTF-8 byte offsets. */
function renderDoc() {
  const bytes = new TextEncoder().encode(state.text);
  const dec = new TextDecoder();
  const slice = (a, b) => dec.decode(bytes.subarray(a, b));
  const evBySpan = new Map(state.report.evidence.map((e) => [e.span.id, e]));
  const out = [];
  let pos = 0;

  for (const sp of state.report.spans) {
    if (sp.start > pos) { out.push(slice(pos, sp.start)); pos = sp.start; }
    const isFigure = sp.kind === 'image';
    const seg = isFigure ? sp.text : slice(sp.start, sp.end);
    pos = Math.max(pos, sp.end);
    const ev = evBySpan.get(sp.id);
    const sups = ev ? visibleSupports(ev) : [];
    const wrap = (node) => (isFigure ? el('span', { class: 'figure' }, el('span', { class: 'figure-label' }, `${sp.id} screenshot`), node) : node);
    if (!sups.length) { out.push(wrap(seg)); continue; }

    const best = strongest(sups);
    const color = colorOf(best.subitem);
    out.push(wrap(el('mark', {
      class: `ev${state.selected === ev.id ? ' sel' : ''}`,
      dataset: { ev: ev.id },
      style: { background: rgba(color, ALPHA[best.strength]), '--c': color },
      title: `${ev.id}: ${sups.map((s) => `${s.subitem} ${s.strength}`).join(', ')}`,
      onclick: () => select(ev.id, false),
    }, seg, el('span', { class: 'tags' }, sups.map((s) =>
      el('span', { class: 'tag', style: { background: rgba(colorOf(s.subitem), ALPHA[s.strength] + 0.12) } }, s.subitem))))));
  }
  if (pos < bytes.length) out.push(slice(pos, bytes.length));
  $('doc').replaceChildren(...out);
}

function renderDetail() {
  const ev = state.report.evidence.find((e) => e.id === state.selected);
  $('detail').hidden = !ev;
  if (!ev) return;
  $('detail').replaceChildren(
    el('div', { class: 'detail-head' },
      el('b', {}, `Evidence ${ev.id}`, el('span', { class: 'muted' }, where(ev))),
      el('button', { class: 'x', title: 'Close', onclick: () => select(ev.id) }, '×')),
    el('blockquote', {}, ev.span.text),
    ...ev.supports.map((s) => {
      const info = state.subInfo.get(s.subitem);
      const color = colorOf(s.subitem);
      return el('div', { class: 'sup' },
        el('div', { class: 'sup-head' },
          el('span', { class: 'code', style: { '--c': color } }, s.subitem),
          el('span', { class: 'sup-name' }, info ? info.name : ''),
          el('span', { class: 'pill', style: { background: rgba(color, ALPHA[s.strength] + 0.1) } }, s.strength),
          s.confidence != null && el('span', { class: 'muted small' }, `${Math.round(s.confidence * 100)}%`)),
        s.reason && el('div', { class: 'small' }, s.reason),
        info && el('div', { class: 'muted small' }, `Rubric (${s.strength}): ${info[s.strength]}`));
    }));
}

function renderEvidence() {
  const rows = state.report.evidence.filter((e) => visibleSupports(e).length > 0);
  $('evidence-sec').hidden = false;
  $('ev-count').textContent = state.filter ? `(${rows.length}, criterion ${state.filter})` : `(${rows.length})`;
  $('ev-body').replaceChildren(...rows.map((e) => el('tr', {
    class: state.selected === e.id ? 'sel' : '',
    onclick: () => select(e.id),
  },
    el('td', {}, e.id, el('div', { class: 'muted small' }, where(e).replace(/^ · /, ''))),
    el('td', {}, e.span.text),
    el('td', {}, el('div', { class: 'labels' }, visibleSupports(e).map((s) => el('div', { class: 'label' },
      el('span', { class: 'pill', style: { background: rgba(colorOf(s.subitem), ALPHA[s.strength] + 0.1) } },
        `${s.subitem} ${s.strength}`),
      s.reason && el('span', { class: 'reason' }, s.reason))))))));
}

init();
