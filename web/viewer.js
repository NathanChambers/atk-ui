const workspaceButton = document.getElementById('workspace-open');
const workspacePath = document.getElementById('workspace-path');
const picker = document.getElementById('workspace-picker');
const search = document.getElementById('workspace-search');
const results = document.getElementById('workspace-results');
const empty = document.getElementById('workspace-empty');
const status = document.getElementById('status');
const flowHost = document.getElementById('flow-board');
const showFlow = createFlowBoard(flowHost, document.querySelector('.flow-tools'));
const modeButtons = [...document.querySelectorAll('[data-render-mode]')];
const params = new URLSearchParams(location.search);
const viewerGeneration = document.querySelector('meta[name="ui-viewer-generation"]')?.content;
let selected = params.get('session') ?? '';
let mode = modeButtons.some(button => button.dataset.renderMode === params.get('mode')) ? params.get('mode') : 'color';
let currentSnapshot = null;
let workspaces = [];
let matches = [];
let activeIndex = 0;
let displayed = '';
let revision = null;

function updateModeButtons() {
  for (const button of modeButtons) {
    button.setAttribute('aria-pressed', String(button.dataset.renderMode === mode));
  }
}

function renderCurrent() {
  showFlow(currentSnapshot, mode);
  if (!currentSnapshot) return;

  const description = mode === 'gaze' ? 'Predicted gaze · 0–100' : 'Read-only';
  status.textContent = `Revision ${currentSnapshot.revision} · ${description}`;
  status.title = mode === 'gaze' ? currentSnapshot.gaze?.rationale ?? '' : '';
}

for (const button of modeButtons) {
  button.addEventListener('click', () => {
    if (mode === button.dataset.renderMode) return;

    mode = button.dataset.renderMode;
    const url = new URL(location.href);
    url.searchParams.set('mode', mode);
    history.replaceState(null, '', url);
    updateModeButtons();
    renderCurrent();
  });
}
updateModeButtons();

function drawWorkspaces() {
  const query = search.value.trim().toLowerCase();
  matches = workspaces.filter(id => id.includes(query));
  activeIndex = Math.max(0, Math.min(activeIndex, matches.length - 1));
  empty.hidden = matches.length > 0;
  const options = matches.map((id, index) => {
    const option = document.createElement('button');
    option.type = 'button';
    option.id = `workspace-option-${index}`;
    option.tabIndex = -1;
    option.setAttribute('role', 'option');
    option.setAttribute('aria-selected', String(index === activeIndex));
    if (id === selected) option.setAttribute('aria-current', 'true');
    option.textContent = id;
    option.addEventListener('click', () => chooseWorkspace(id));
    return option;
  });
  results.replaceChildren(...options);
  if (options.length > 0) {
    search.setAttribute('aria-activedescendant', options[activeIndex].id);
    options[activeIndex].scrollIntoView({ block: 'nearest' });
  } else {
    search.removeAttribute('aria-activedescendant');
  }
}

function openWorkspaces() {
  if (picker.open) return;
  search.value = '';
  activeIndex = workspaces.indexOf(selected);
  picker.showModal();
  search.focus();
  drawWorkspaces();
}

function chooseWorkspace(id) {
  selected = id;
  const url = new URL(location.href);
  url.searchParams.set('session', id);
  history.replaceState(null, '', url);
  picker.close();
}

workspaceButton.addEventListener('click', openWorkspaces);
picker.addEventListener('click', event => { if (event.target === picker) picker.close(); });
search.addEventListener('input', () => {
  activeIndex = 0;
  drawWorkspaces();
});
search.addEventListener('keydown', event => {
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    activeIndex += event.key === 'ArrowDown' ? 1 : -1;
    drawWorkspaces();
  } else if (event.key === 'Enter' && matches.length > 0) {
    event.preventDefault();
    chooseWorkspace(matches[activeIndex]);
  }
});
document.addEventListener('keydown', event => {
  if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== 'p') return;
  event.preventDefault();
  openWorkspaces();
});

async function refresh() {
  const list = await fetch('/api/sessions', { cache: 'no-store' });
  if (!list.ok) throw Error('Could not list workspaces');
  const generation = list.headers.get('X-UI-Viewer-Generation');
  if (generation && viewerGeneration && generation !== viewerGeneration) {
    location.reload();
    return;
  }
  const ids = await list.json();
  if (!ids.includes(selected)) selected = ids[0] ?? '';
  if (ids.join('|') !== workspaces.join('|')) {
    workspaces = ids;
    if (picker.open) drawWorkspaces();
  }

  if (!selected) {
    workspacePath.textContent = 'No workspace selected';
    status.textContent = 'Read-only viewer';
    currentSnapshot = null;
    renderCurrent();
    displayed = '';
    revision = null;
    return;
  }

  const id = selected;
  const headers = displayed === id && revision ? { 'If-None-Match': revision } : {};
  const response = await fetch('/api/sessions/' + encodeURIComponent(id), { headers, cache: 'no-store' });
  if (id !== selected) return;
  if (response.status === 304) {
    renderCurrent();
    return;
  }
  if (!response.ok) throw Error(await response.text());
  const snapshot = await response.json();
  if (id !== selected) return;

  currentSnapshot = snapshot;
  renderCurrent();
  displayed = id;
  revision = response.headers.get('ETag');
  workspacePath.textContent = snapshot.name;
}

async function poll() {
  try {
    await refresh();
  } catch (error) {
    status.textContent = error.message;
  }
  setTimeout(poll, 650);
}

poll();
