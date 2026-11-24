const FLOW_CARD_WIDTH = 224;
const FLOW_CARD_HEIGHT = 398;
const FLOW_CAPTION_HEIGHT = 24;
const FLOW_THUMBNAIL_SIDE = 768;

function createFlowBoard(host, controls) {
  const viewport = host.querySelector('.flow-viewport');
  const world = host.querySelector('.flow-world');
  let identity = '';
  let selectedMode = '';
  let board = null;
  let cards = new Map();
  let width = 0;
  let height = 0;
  let x = 0;
  let y = 0;
  let zoom = 1;
  let dispose = [];
  const pointers = new Map();
  let resolutionCheck = 0;

  function place() {
    world.style.transform = `translate(${x}px, ${y}px) scale(${zoom})`;
    scheduleResolutionCheck();
  }

  function scheduleResolutionCheck() {
    if (resolutionCheck) return;

    resolutionCheck = requestAnimationFrame(() => {
      resolutionCheck = 0;
      const density = window.devicePixelRatio || 1;

      const bounds = viewport.getBoundingClientRect();
      for (const image of world.querySelectorAll('.native-frame')) {
        const url = new URL(image.src);
        if (!url.searchParams.has('thumb')) continue;

        const rect = image.getBoundingClientRect();
        const visible = rect.right > bounds.left && rect.left < bounds.right
          && rect.bottom > bounds.top && rect.top < bounds.bottom;
        const pixels = Math.max(rect.width, rect.height) * density;
        if (!visible || pixels <= FLOW_THUMBNAIL_SIDE) continue;

        url.searchParams.delete('thumb');
        image.src = url;
      }
    });
  }

  new ResizeObserver(scheduleResolutionCheck).observe(viewport);

  function fit() {
    if (!board || !viewport.clientWidth) return;
    zoom = Math.min(1.3, (viewport.clientWidth - 32) / width, (viewport.clientHeight - 64) / height);
    zoom = Math.max(0.12, zoom);
    x = (viewport.clientWidth - width * zoom) / 2;
    y = (viewport.clientHeight - height * zoom) / 2 + 14;
    place();
  }

  function zoomAt(factor, px = viewport.clientWidth / 2, py = viewport.clientHeight / 2) {
    const next = Math.max(0.12, Math.min(3, zoom * factor));
    x = px - (px - x) * next / zoom;
    y = py - (py - y) * next / zoom;
    zoom = next;
    place();
  }

  controls.querySelector('[data-flow-fit]').addEventListener('click', fit);
  controls.querySelector('[data-flow-in]').addEventListener('click', () => zoomAt(1.25));
  controls.querySelector('[data-flow-out]').addEventListener('click', () => zoomAt(0.8));
  viewport.addEventListener('wheel', event => {
    event.preventDefault();
    const rect = viewport.getBoundingClientRect();
    zoomAt(Math.exp(-event.deltaY * 0.0015), event.clientX - rect.left, event.clientY - rect.top);
  }, { passive: false });
  viewport.addEventListener('pointerdown', event => {
    if (event.button !== 0 || event.target.closest('.flow-zone')) return;
    pointers.set(event.pointerId, [event.clientX, event.clientY]);
    viewport.setPointerCapture(event.pointerId);
    viewport.classList.add('dragging');
  });
  viewport.addEventListener('pointermove', event => {
    const previous = pointers.get(event.pointerId);
    if (!previous) return;
    const [oldX, oldY] = previous;
    const [newX, newY] = [event.clientX, event.clientY];
    const other = [...pointers.entries()].find(([id]) => id !== event.pointerId)?.[1];
    if (other) {
      const rect = viewport.getBoundingClientRect();
      const oldDistance = Math.hypot(oldX - other[0], oldY - other[1]);
      if (oldDistance > 0) {
        const distance = Math.hypot(newX - other[0], newY - other[1]);
        zoomAt(distance / oldDistance, (oldX + other[0]) / 2 - rect.left, (oldY + other[1]) / 2 - rect.top);
      }
    }
    const movement = other ? 0.5 : 1;
    x += (newX - oldX) * movement;
    y += (newY - oldY) * movement;
    pointers.set(event.pointerId, [newX, newY]);
    place();
  });
  for (const event of ['pointerup', 'pointercancel', 'lostpointercapture']) {
    viewport.addEventListener(event, ({ pointerId }) => {
      pointers.delete(pointerId);
      if (!pointers.size) viewport.classList.remove('dragging');
    });
  }
  viewport.addEventListener('keydown', event => {
    const movement = { ArrowLeft: [40, 0], ArrowRight: [-40, 0], ArrowUp: [0, 40], ArrowDown: [0, -40] }[event.key];
    if (movement) {
      x += movement[0];
      y += movement[1];
      place();
    } else if (event.key === '+' || event.key === '=') zoomAt(1.25);
    else if (event.key === '-') zoomAt(0.8);
    else return;
    event.preventDefault();
  });

  function focusFrame(frame) {
    const availableHeight = Math.max(1, viewport.clientHeight - 96);
    const frameHeight = frame.previewHeight + FLOW_CAPTION_HEIGHT;
    zoom = Math.max(0.12, Math.min(1.45, (viewport.clientWidth - 32) / frame.previewWidth, availableHeight / frameHeight));
    x = viewport.clientWidth / 2 - (frame.x + frame.previewWidth / 2) * zoom;
    y = 48 + availableHeight / 2 - (frame.y + frameHeight / 2) * zoom;
    place();
    world.querySelector(`[data-flow-frame="${frame.id}"]`)?.focus({ preventScroll: true });
  }

  return function show(snapshot, mode) {
    const key = snapshot ? `${snapshot.id}/${snapshot.revision}` : '';
    if (key && identity === key && selectedMode === mode) return;
    const newBoard = identity !== key;
    identity = key;
    selectedMode = mode;
    const authored = snapshot?.flow;
    board = authored ?? (snapshot?.screen ? { frames: [{ id: 'current', title: snapshot.screen.label, x: 40, y: 40 }], edges: [] } : null);
    for (const cleanup of dispose) cleanup();
    dispose = [];
    cards = new Map();
    world.replaceChildren();
    if (!board) {
      const notice = document.createElement('p');
      notice.className = 'flow-empty';
      notice.textContent = 'No screen to display.';
      world.append(notice);
      world.style.transform = '';
      return;
    }

    width = Math.max(...board.frames.map(frame => frame.x)) + FLOW_CARD_WIDTH + 40;
    height = Math.max(...board.frames.map(frame => frame.y)) + FLOW_CAPTION_HEIGHT + FLOW_CARD_HEIGHT + 40;
    Object.assign(world.style, { width: `${width}px`, height: `${height}px` });
    const lines = flowSvg('svg', {
      class: 'flow-lines',
      width,
      height,
      viewBox: `0 0 ${width} ${height}`,
      'aria-label': board.edges.map(edge => `${edge.from} to ${edge.to}: ${edge.label}`).join('. '),
      role: 'img'
    });
    world.append(lines);
    const connections = new Map();
    let fittedTransform = null;
    function updateBounds() {
      const placements = [...cards.values()];
      width = Math.max(...placements.map(frame => frame.x + frame.previewWidth)) + 40;
      height = Math.max(...placements.map(frame => frame.y + frame.previewHeight)) + FLOW_CAPTION_HEIGHT + 40;
      Object.assign(world.style, { width: `${width}px`, height: `${height}px` });
      lines.setAttribute('width', width);
      lines.setAttribute('height', height);
      lines.setAttribute('viewBox', `0 0 ${width} ${height}`);
      lines.replaceChildren();
      connections.clear();
      drawConnections(lines, board, cards, connections);
      if (highlighted) connections.get(highlighted)?.classList.add('is-highlighted');
    }
    let highlighted = null;
    const highlight = edge => {
      if (highlighted === edge) return;
      connections.get(highlighted)?.classList.remove('is-highlighted');
      highlighted = edge;
      const connection = connections.get(edge);
      if (connection) {
        connection.classList.add('is-highlighted');
        lines.append(connection);
      }
    };

    for (const frame of board.frames) {
      const card = document.createElement('article');
      card.className = 'flow-card';
      card.dataset.flowFrame = frame.id;
      card.tabIndex = -1;
      card.style.left = `${frame.x}px`;
      card.style.top = `${frame.y}px`;
      const title = document.createElement('h2');
      title.textContent = frame.title;
      const preview = document.createElement('div');
      preview.className = 'flow-card-preview';
      preview.textContent = 'Rendering…';
      card.append(title, preview);
      world.append(card);
      const placement = { ...frame, previewWidth: FLOW_CARD_WIDTH, previewHeight: FLOW_CARD_HEIGHT };
      cards.set(frame.id, placement);
      const renderer = createRenderer(preview, height => {
        if (height === placement.previewHeight) return;
        placement.previewHeight = height;
        preview.style.height = `${height}px`;
        updateBounds();
        if (fittedTransform === world.style.transform) {
          fit();
          fittedTransform = world.style.transform;
        }
      });
      dispose.push(renderer.dispose);
      const query = `revision=${snapshot.revision}&flow=${encodeURIComponent(frame.id)}`;
      const state = authored ? fetch(`/api/sessions/${encodeURIComponent(snapshot.id)}?${query}`).then(async response => {
        if (!response.ok) throw Error(await response.text());
        return response.json();
      }) : Promise.resolve(snapshot);
      state.then(frameSnapshot => {
        if (!card.isConnected) return;
        const screen = frameSnapshot.screen;
        const landscape = screen && screen.width > screen.height;
        if (screen && (landscape || frame.screen)) {
          const { width: screenWidth, height: screenHeight } = screen;
          const maxWidth = frame.screen ? 520 : FLOW_CARD_HEIGHT;
          const scale = Math.min(0.38, maxWidth / screenWidth, FLOW_CARD_HEIGHT / screenHeight);
          placement.previewWidth = screenWidth * scale;
          placement.previewHeight = screenHeight * scale;
          card.style.width = `${placement.previewWidth}px`;
          preview.style.height = `${placement.previewHeight}px`;
          updateBounds();
          if (fittedTransform === world.style.transform) {
            fit();
            fittedTransform = world.style.transform;
          }
        }
        const outgoing = board.edges.filter(edge => edge.from === frame.id);
        renderer(frameSnapshot, mode, authored ? query + '&thumb=1' : 'thumb=1');
        scheduleResolutionCheck();
        addInteractionZones(preview, placement, frameSnapshot, outgoing, highlight, edge => focusFrame(cards.get(edge.to)));
      }).catch(error => {
        if (card.isConnected) preview.textContent = error.message;
      });
    }
    updateBounds();
    if (newBoard) {
      fit();
      fittedTransform = world.style.transform;
    }
  };
}

function addInteractionZones(preview, placement, snapshot, edges, highlight, activate) {
  const screen = snapshot.screen;
  if (!screen) return;
  const contentHeight = screen.canvasMode === 'content_height';
  const scale = Math.min(placement.previewWidth / screen.width, contentHeight ? 1 : placement.previewHeight / screen.height, 1);
  const insetX = (placement.previewWidth - screen.width * scale) / 2;
  const insetY = contentHeight ? 0 : (placement.previewHeight - screen.height * scale) / 2;
  for (const edge of edges) {
    const element = screen.elements.find(node => node.id === edge.action);
    const rect = element ? [element.x, element.y, element.width, element.height] : null;
    if (!rect) continue;
    const [x, y, width, height] = rect;
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'flow-zone';
    button.setAttribute('aria-label', edge.label);
    button.title = edge.label;
    Object.assign(button.style, { left: `${insetX + x * scale}px`, top: `${insetY + y * scale}px`, width: `${width * scale}px`, height: `${height * scale}px` });
    button.addEventListener('pointerenter', () => highlight(edge));
    button.addEventListener('pointerleave', () => { if (document.activeElement !== button) highlight(null); });
    button.addEventListener('focus', () => highlight(edge));
    button.addEventListener('blur', () => highlight(null));
    button.addEventListener('click', () => activate(edge));
    preview.append(button);
  }
}

function drawConnections(layer, board, cards, connections) {
  const measure = document.createElement('canvas').getContext('2d');
  measure.font = '600 13px system-ui, sans-serif';
  const occupied = [...cards.values()].map(card => ({
    left: card.x, right: card.x + card.previewWidth,
    top: card.y, bottom: card.y + FLOW_CAPTION_HEIGHT + card.previewHeight
  }));
  for (const edge of board.edges) {
    const source = cards.get(edge.from);
    const destination = cards.get(edge.to);
    const side = connectionSide(source, destination);
    const siblings = board.edges.filter(candidate => candidate.from === edge.from && connectionSide(source, cards.get(candidate.to)) === side);
    const horizontal = side === 'left' || side === 'right';
    const forward = side === 'right' || side === 'bottom';
    const direction = forward ? 1 : -1;
    const reciprocal = board.edges.some(other => other.from === edge.to && other.to === edge.from);
    let fraction = (siblings.indexOf(edge) + 1) / (siblings.length + 1);
    let destinationFraction = 0.5;
    if (reciprocal) {
      const parallel = siblings.filter(other => other.to === edge.to);
      const lane = (parallel.indexOf(edge) + 1) / (parallel.length + 1);
      fraction = (forward ? 0.34 : 0.51) + lane * 0.15;
      destinationFraction = fraction;
    }
    const startX = source.x + (horizontal ? (forward ? source.previewWidth : 0) : source.previewWidth * fraction);
    const endX = destination.x + (horizontal ? (forward ? 0 : destination.previewWidth) : destination.previewWidth * destinationFraction);
    const startY = source.y + FLOW_CAPTION_HEIGHT + (horizontal ? source.previewHeight * fraction : forward ? source.previewHeight : 0);
    const endY = destination.y + FLOW_CAPTION_HEIGHT + (horizontal ? destination.previewHeight * destinationFraction : forward ? 0 : destination.previewHeight);
    const reach = Math.max(26, Math.abs(horizontal ? endX - startX : endY - startY) * 0.55);
    const bow = reciprocal ? -14 * direction : 0;
    const firstX = startX + (horizontal ? reach * direction : bow);
    const secondX = endX + (horizontal ? -reach * direction : bow);
    const firstY = startY + (horizontal ? bow : reach * direction);
    const secondY = endY + (horizontal ? bow : -reach * direction);

    const labelX = (startX + 3 * firstX + 3 * secondX + endX) / 8;
    const labelY = (startY + 3 * firstY + 3 * secondY + endY) / 8;
    const group = flowSvg('g', { class: 'flow-connection' });
    const spline = flowSvg('path', {
      class: 'flow-spline',
      d: `M ${startX} ${startY} C ${firstX} ${firstY} ${secondX} ${secondY} ${endX} ${endY}`
    });
    const title = flowSvg('title', {});
    title.textContent = edge.label;
    group.append(title, spline, connectionNode(startX, startY), connectionNode(endX, endY));
    const gap = horizontal ? Math.abs(endX - startX) : 150;
    const label = placeConnectionLabel(edge.label, labelX, labelY, gap, measure, occupied);
    if (label) group.append(label);
    layer.append(group);
    connections.set(edge, group);
  }
}

function placeConnectionLabel(text, x, y, gap, measure, occupied) {
  const lineHeight = 15;
  const padding = 5;
  const widths = [...new Set([Math.min(145, gap - 2 * padding), 95, 70, 48])];
  for (const width of widths) {
    if (width < 24) continue;
    const lines = [];
    let line = '';
    for (const word of text.trim().split(/\s+/)) {
      const joined = line ? line + ' ' + word : word;
      if (line && measure.measureText(joined).width > width) {
        lines.push(line);
        line = word;
      } else {
        line = joined;
      }
    }
    if (line) lines.push(line);
    if (!lines.length || lines.length > 4) continue;

    const textWidth = Math.max(...lines.map(value => measure.measureText(value).width));
    if (textWidth > width) continue;
    const halfWidth = textWidth / 2 + padding;
    const halfHeight = lines.length * lineHeight / 2 + padding;
    for (const [dx, dy] of [[0, 0], [0, -18], [0, 18], [0, -35], [0, 35], [-22, 0], [22, 0], [-44, 0], [44, 0]]) {
      const cx = x + dx;
      const cy = y + dy;
      const bounds = { left: cx - halfWidth, right: cx + halfWidth, top: cy - halfHeight, bottom: cy + halfHeight };
      if (occupied.some(rect => rect.left < bounds.right && rect.right > bounds.left && rect.top < bounds.bottom && rect.bottom > bounds.top)) continue;

      const label = flowSvg('text', { class: 'flow-link-label' });
      lines.forEach((value, index) => {
        const span = flowSvg('tspan', { x: cx, y: cy + (index - (lines.length - 1) / 2) * lineHeight });
        span.textContent = value;
        label.append(span);
      });
      occupied.push(bounds);
      return label;
    }
  }
  return null;
}

function connectionNode(x, y) {
  return flowSvg('polygon', { class: 'flow-link-node', points: `${x},${y - 6} ${x + 6},${y} ${x},${y + 6} ${x - 6},${y}` });
}

function flowSvg(tag, attributes) {
  const node = document.createElementNS('http://www.w3.org/2000/svg', tag);
  for (const [name, value] of Object.entries(attributes)) node.setAttribute(name, value);
  return node;
}

function connectionSide(source, destination) {
  if (destination.x >= source.x + source.previewWidth) return 'right';
  if (source.x >= destination.x + destination.previewWidth) return 'left';
  return destination.y >= source.y ? 'bottom' : 'top';
}
