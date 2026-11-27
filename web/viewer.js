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
let firstDiscovery = true;
let mode = modeButtons.some(button => button.dataset.renderMode === params.get('mode')) ? params.get('mode') : 'color';
let currentSnapshot = null;
let workspaces = [];
let workspaceRevision = null;
let matches = [];
let activeIndex = 0;
let displayed = '';
let revision = null;
let refreshQueued = false;
let refreshing = false;
let retryTimer = null;

function updateModeButtons() {
  for (const button of modeButtons) {
    button.setAttribute('aria-pressed', String(button.dataset.renderMode === mode));
  }
}

function renderCurrent() {
  showFlow(currentSnapshot, mode);
  renderStatus();
}

function renderStatus() {
  if (!currentSnapshot) return;

  const description = mode === 'gaze' ? 'Predicted gaze · 0–100' : 'Read-only';
  status.textContent = `rev ${currentSnapshot.revision} · ${description}`;
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
  matches = workspaces.filter(({ id, name }) => id.includes(query) || name.toLowerCase().includes(query));
  activeIndex = Math.max(0, Math.min(activeIndex, matches.length - 1));
  empty.hidden = matches.length > 0;
  const options = matches.map(({ id, name }, index) => {
    const option = document.createElement('button');
    option.type = 'button';
    option.id = `workspace-option-${index}`;
    option.tabIndex = -1;
    option.setAttribute('role', 'option');
    option.setAttribute('aria-selected', String(index === activeIndex));
    if (id === selected) option.setAttribute('aria-current', 'true');
    option.textContent = name === id ? id : `${name} · ${id}`;
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
  activeIndex = workspaces.findIndex(workspace => workspace.id === selected);
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
  requestRefresh();
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
    chooseWorkspace(matches[activeIndex].id);
  }
});
document.addEventListener('keydown', event => {
  if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== 'p') return;
  event.preventDefault();
  openWorkspaces();
});

async function refresh() {
  const listHeaders = workspaceRevision ? { 'If-None-Match': workspaceRevision } : {};
  const list = await fetch('/api/sessions?details=1', { headers: listHeaders, cache: 'no-store' });
  const generation = list.headers.get('X-UI-Viewer-Generation');
  if (generation && viewerGeneration && generation !== viewerGeneration) {
    location.reload();
    return;
  }
  if (list.status !== 304) {
    if (!list.ok) throw Error('Could not list workspaces');
    workspaces = await list.json();
    workspaceRevision = list.headers.get('ETag');
    if (picker.open) drawWorkspaces();
  }

  if (firstDiscovery) {
    firstDiscovery = false;
    if (!selected) openWorkspaces();
  }
  if (!workspaces.some(workspace => workspace.id === selected)) {
    workspacePath.textContent = selected || 'No workspace selected';
    status.textContent = selected ? 'Waiting for saved workspace' : 'Waiting for a saved workspace in this project';
    if (currentSnapshot) {
      currentSnapshot = null;
      renderCurrent();
    }
    displayed = '';
    revision = null;
    return;
  }

  const id = selected;
  const headers = displayed === id && revision ? { 'If-None-Match': revision } : {};
  const response = await fetch('/api/sessions/' + encodeURIComponent(id), { headers, cache: 'no-store' });
  if (id !== selected) return;
  if (response.status === 304) {
    renderStatus();
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

function requestRefresh() {
  clearTimeout(retryTimer);
  refreshQueued = true;
  if (refreshing === false) drainRefreshes();
}

async function drainRefreshes() {
  refreshing = true;
  while (refreshQueued) {
    refreshQueued = false;
    try {
      await refresh();
    } catch (error) {
      status.textContent = error.message;
      if (refreshQueued === false) retryTimer = setTimeout(requestRefresh, 1000);
    }
  }
  refreshing = false;
}

function connectEvents() {
  const events = new EventSource('/api/events');
  events.addEventListener('open', requestRefresh);
  events.addEventListener('change', requestRefresh);
  events.addEventListener('error', () => {
    status.textContent = 'Reconnecting…';
    if (events.readyState === EventSource.CLOSED) setTimeout(connectEvents, 1000);
  });
}

requestRefresh();
setInterval(requestRefresh, 2000);
if (typeof EventSource === 'function') connectEvents();
