function createRenderer(host, onContentSize = () => {}) {
  let stage = null;
  let screen = null;
  const resize = new ResizeObserver(fit);
  resize.observe(host);

  function fit() {
    if (!stage) return;
    const widthScale = Math.min(1, host.clientWidth / screen.width);
    const contentHeight = screen.canvasMode === 'content_height';
    const scale = contentHeight ? widthScale : Math.min(widthScale, host.clientHeight / screen.height);
    if (contentHeight) onContentSize(screen.height * scale);
    stage.style.zoom = String(scale || 1);
  }

  function render(snapshot, mode, frameQuery = '') {
    screen = snapshot?.screen;
    stage = null;
    host.replaceChildren();
    if (!screen) {
      host.textContent = 'Create a screen with atk-ui-cli to see it here.';
      return;
    }

    stage = document.createElement('section');
    stage.className = 'prototype-screen';
    stage.setAttribute('aria-label', screen.label);
    Object.assign(stage.style, { width: `${screen.width}px`, height: `${screen.height}px` });
    const image = document.createElement('img');
    image.className = 'native-frame';
    image.alt = screen.label;
    image.width = screen.width;
    image.height = screen.height;
    const options = new URLSearchParams(frameQuery);
    options.set('mode', mode === 'gaze' ? 'grayscale' : mode);
    image.src = '/api/sessions/' + encodeURIComponent(snapshot.id) + '/frame/' + snapshot.revision + '.png?' + options;
    stage.append(image);
    if (mode === 'gaze') drawGaze(stage, screen, snapshot.gaze);
    host.append(stage);
    fit();
  }

  render.dispose = () => resize.disconnect();
  return render;
}

function drawGaze(stage, screen, study) {
  if (!study) {
    const notice = document.createElement('div');
    notice.className = 'gaze-overlay gaze-empty';
    notice.style.fontSize = `${screen.width * 0.035}px`;
    notice.textContent = 'No gaze prediction for this frame.';
    stage.append(notice);
    return;
  }

  const canvas = document.createElement('canvas');
  canvas.className = 'gaze-overlay';
  canvas.setAttribute('role', 'img');
  const names = new Map(study.targets.map(target => [target.id, target]));
  const sequence = study.path.map(id => names.get(id).label).join(' → ');
  const scores = study.targets.map(target => `${target.label}: ${target.score}/100`).join(', ');
  canvas.setAttribute('aria-label', `Predicted attention. ${scores}. Path: ${sequence}. ${study.rationale}`);
  canvas.title = study.rationale;
  const resolution = Math.min(1, 2048 / Math.max(screen.width, screen.height));
  canvas.width = Math.max(1, Math.round(screen.width * resolution));
  canvas.height = Math.max(1, Math.round(screen.height * resolution));
  stage.append(canvas);

  const context = canvas.getContext('2d');
  context.scale(canvas.width / screen.width, canvas.height / screen.height);
  const unit = Math.min(screen.width, screen.height) / 1080;
  const path = study.path.map(id => names.get(id));
  const center = ({ rect: [x, y, width, height] }) => [x + width / 2, y + height / 2];
  context.lineJoin = 'round';
  for (const { rect: [x, y, width, height], score } of study.targets) {
    const hue = 204 - score * 1.55;
    context.fillStyle = `hsla(${hue}, 92%, 58%, ${0.08 + score / 520})`;
    context.strokeStyle = `hsl(${hue}, 82%, 44%)`;
    context.lineWidth = 4 * unit;
    context.fillRect(x, y, width, height);
    context.strokeRect(x, y, width, height);
  }

  for (let index = 1; index < path.length; index++) {
    const [x, y] = center(path[index - 1]);
    const [toX, toY] = center(path[index]);
    const distance = Math.hypot(toX - x, toY - y);
    if (distance < 56 * unit) continue;
    const dx = (toX - x) / distance;
    const dy = (toY - y) / distance;
    const endX = toX - dx * 27 * unit;
    const endY = toY - dy * 27 * unit;
    context.beginPath();
    context.moveTo(x + dx * 28 * unit, y + dy * 28 * unit);
    context.lineTo(endX, endY);
    context.lineTo(endX - (dx + dy * 0.7) * 20 * unit, endY - (dy - dx * 0.7) * 20 * unit);
    context.moveTo(endX, endY);
    context.lineTo(endX - (dx - dy * 0.7) * 20 * unit, endY - (dy + dx * 0.7) * 20 * unit);
    context.strokeStyle = '#252934';
    context.lineWidth = 10 * unit;
    context.stroke();
    context.strokeStyle = '#fff6d7';
    context.lineWidth = 5 * unit;
    context.stroke();
  }

  function badge(text, x, y) {
    const padding = 10 * unit;
    const width = Math.min(screen.width, context.measureText(text).width + padding * 2);
    const left = Math.max(0, Math.min(x, screen.width - width));
    context.fillStyle = '#222635';
    context.fillRect(left, y, width, 44 * unit);
    context.fillStyle = '#fff7dd';
    context.fillText(text, left + padding, y + 22 * unit, width - padding * 2);
  }

  context.font = `700 ${28 * unit}px system-ui`;
  context.textBaseline = 'middle';
  for (const { rect: [x, y], label, score } of study.targets) {
    badge(`${label} · ${score}`, x, Math.max(0, y - 44 * unit));
  }
  for (const [index, target] of path.entries()) {
    const [x, y] = center(target);
    badge(String(index + 1), x - 14 * unit, y - 22 * unit);
  }
}
