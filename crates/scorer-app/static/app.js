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
  timer: null,
  job: null,             // {id, name} of the PDF job behind the current report
};

const POLL_MS = 1500;
const STAGE_TEXT = {
  queued: 'Waiting for the previous job…',
  extracting: 'Reading PDF…',
  describing_images: 'Describing screenshots',
  classifying: 'Labelling evidence',
  annotating: 'Writing annotated PDF…',
  done: 'Done',
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

// ---------------------------------------------------------------- startup

async function init() {
  wire();
  try {
    const [health, rubric] = await Promise.all([getJson('/api/health'), getJson('/api/rubric')]);
    setStatus(`${health.provider.model} (${health.provider.kind})`, 'ok');
    loadRubric(rubric);
  } catch (e) {
    setStatus('API unreachable', 'bad');
    showError(e.message);
  }
}

async function getJson(url, opts) {
  const r = await fetch(url, opts);
  let body = null;
  try { body = await r.json(); } catch { /* non-JSON body */ }
  if (!r.ok) throw new Error((body && body.error) || `HTTP ${r.status}`);
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
  // Drag and drop anywhere on the page.
  window.addEventListener('dragover', (e) => { e.preventDefault(); document.body.classList.add('dragging'); });
  window.addEventListener('dragleave', (e) => { if (!e.relatedTarget) document.body.classList.remove('dragging'); });
  window.addEventListener('drop', (e) => {
    e.preventDefault();
    document.body.classList.remove('dragging');
    const f = e.dataTransfer.files[0];
    if (f) openFile(f);
  });
  updateHint();
}

// ---------------------------------------------------------------- UI state

function setStatus(text, kind) {
  const s = $('status');
  s.textContent = text;
  s.className = `status ${kind}`;
}

function showTab(name) {
  document.querySelectorAll('.tab').forEach((b) => b.classList.toggle('active', b.dataset.tab === name));
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
  $('error').textContent = msg;
  $('error').hidden = false;
}
function hideError() { $('error').hidden = true; }

function setBusy(on, stage = 'Scoring…') {
  $('busy-stage').textContent = stage;
  $('score').disabled = on;
  $('file').disabled = on;
  $('busy').hidden = !on;
  $('empty').hidden = true;
  clearInterval(state.timer);
  if (on) {
    $('results').hidden = true;
    const t0 = Date.now();
    $('elapsed').textContent = '0';
    state.timer = setInterval(() => { $('elapsed').textContent = Math.round((Date.now() - t0) / 1000); }, 1000);
  } else {
    $('results').hidden = !state.report;
    $('empty').hidden = !!state.report;
  }
}

async function openFile(f) {
  hideError();
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
  const text = $('input').value;
  if (!text.trim()) { showError('Paste some text first.'); return; }
  hideError();
  setBusy(true);
  try {
    const report = await getJson('/api/score', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ text, include_spans: true }),
    });
    showReport(report, text, null);
  } catch (e) {
    showError(`Scoring failed: ${e.message}`);
  } finally {
    setBusy(false);
  }
}

/** Uploads a PDF, polls the job until it finishes, then shows the result. */
async function runPdf(file) {
  setBusy(true, `Uploading ${file.name}…`);
  try {
    const form = new FormData();
    form.append('file', file);
    const { id } = await getJson('/api/jobs', { method: 'POST', body: form });
    for (;;) {
      await new Promise((r) => setTimeout(r, POLL_MS));
      const job = await getJson(`/api/jobs/${id}`);
      const p = job.progress;
      if (p.stage === 'failed') throw new Error(job.error || 'job failed');
      if (p.stage === 'done' && job.report) {
        showReport(job.report, job.report.source_text || '', { id, name: job.file_name });
        return;
      }
      const count = p.total ? ` ${Math.min(p.done + 1, p.total)} / ${p.total}` : '';
      $('busy-stage').textContent = `${STAGE_TEXT[p.stage] || p.stage}${count}`;
    }
  } catch (e) {
    showError(`Scoring ${file.name} failed: ${e.message}`);
  } finally {
    setBusy(false);
  }
}

function showReport(report, text, job) {
  Object.assign(state, { report, text, job, filter: null, selected: null });
  $('annotated-tab').disabled = false;
  $('pdf-tab').hidden = !job;
  $('download-pdf').hidden = !job;
  if (job) {
    const url = `/api/jobs/${job.id}/annotated.pdf`;
    $('pdf-frame').src = url;
    $('download-pdf').href = `${url}?download=1`;
  } else {
    $('pdf-frame').removeAttribute('src');
  }
  renderAll();
  showTab(job ? 'pdf' : 'annotated');
}

function downloadJson() {
  const blob = new Blob([JSON.stringify(state.report, null, 2)], { type: 'application/json' });
  const a = el('a', { href: URL.createObjectURL(blob), download: `score-${new Date().toISOString().slice(0, 19)}.json` });
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
  const source = state.job ? `${state.job.name} · ` : '';
  const figs = stats.figures ? ` · ${stats.figures} screenshot(s)` : '';
  $('total').textContent = fmt(score.total);
  $('max-total').textContent = fmt(score.max_total, 0);
  $('meta').textContent =
    `${source}${stats.evidence} evidence in ${stats.spans} spans${figs} · ${stats.llm_calls} LLM call(s) · ${provider.model}`;
  $('criteria').replaceChildren(...score.criteria.map(renderCriterion));

  $('warnings').hidden = warnings.length === 0;
  $('warnings').replaceChildren(
    el('b', {}, `${warnings.length} warning(s) from validating model output`),
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
  const showAll = state.filter && el('button', { class: 'btn ghost', onclick: () => toggleFilter(state.filter) }, 'Show all');
  $('legend').replaceChildren(...[...items, el('span', { class: 'sep' }), ...levels, showAll].filter(Boolean));
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
