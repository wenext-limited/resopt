// Comparison modes shared by the inspector and the full-size dialog:
// 2-up, swipe, onion skin and an amplified per-pixel difference.
const COMPARE_MODES = ['two-up', 'swipe', 'onion', 'difference'];
const DIFFERENCE_GAINS = [1, 4, 16, 64];

function compareMode() { return COMPARE_MODES.includes(state.compareMode) ? state.compareMode : 'two-up'; }

// Buttons that switch the mode; `onChange` re-renders whatever hosts the stage.
function compareModeSwitch(onChange) {
  const group = el('div', 'compare-modes'); group.setAttribute('role', 'group'); group.setAttribute('aria-label', t('compareModes'));
  for (const mode of COMPARE_MODES) {
    const button = el('button', '', t(`compareMode_${mode}`)); button.type = 'button'; button.dataset.compareMode = mode;
    button.setAttribute('aria-pressed', String(mode === compareMode()));
    button.addEventListener('click', () => { state.compareMode = mode; store('resopt-compare', mode); onChange(); });
    group.append(button);
  }
  return group;
}

// One stage with both pictures stacked. `fit` is 'contain' in the inspector
// and 'natural' in the dialog. Returns the stage and its mode-specific control.
function compareStage({ before, after, beforeAlt, afterAlt, fit }) {
  const mode = compareMode(), box = el('div', 'compare-box');
  const stage = el('div', `compare-wrap ${fit} mode-${mode}`), controls = el('div', 'compare-control');
  stage.dataset.background = state.background;
  // Layers live in a frame that is exactly the picture's rectangle, so that a
  // split position or clip refers to the picture, not to the empty margin.
  const frame = el('div', 'compare-frame'); stage.append(frame);
  const fitFrame = (width, height) => {
    if (fit !== 'contain' || !width || !height) return;
    const scale = Math.min((stage.clientWidth - 20) / width, (stage.clientHeight - 20) / height, 1);
    frame.style.width = `${Math.max(1, Math.round(width * scale))}px`; frame.style.height = `${Math.max(1, Math.round(height * scale))}px`;
  };
  const base = el('img', 'layer base'), top = el('img', 'layer top');
  for (const img of [base, top]) img.decoding = 'async';
  const range = (label, value, apply) => {
    const input = el('input'); input.type = 'range'; input.min = '0'; input.max = '100'; input.value = String(value); input.setAttribute('aria-label', label);
    input.addEventListener('input', () => apply(Number(input.value))); apply(value);
    const wrap = el('label', 'compare-range'); wrap.append(el('span', '', label), input); controls.append(wrap); return input;
  };
  if (mode === 'swipe') {
    // Left of the handle is the original, right of it the candidate.
    base.src = after; base.alt = afterAlt; top.src = before; top.alt = beforeAlt;
    const handle = el('div', 'compare-handle'); handle.setAttribute('aria-hidden', 'true'); frame.append(base, top, handle);
    range(t('compareSwipe'), 50, value => { top.style.clipPath = `inset(0 ${100 - value}% 0 0)`; handle.style.left = `${value}%`; });
    controls.append(el('span', 'hint', `${beforeAlt} ◀ ▶ ${afterAlt}`));
  } else if (mode === 'onion') {
    base.src = before; base.alt = beforeAlt; top.src = after; top.alt = afterAlt; frame.append(base, top);
    range(t('compareOnion'), 50, value => { top.style.opacity = String(value / 100); });
    controls.append(el('span', 'hint', `0% ${beforeAlt} · 100% ${afterAlt}`));
  } else {
    // Difference: computed from pixels so that it can be amplified; a 2/255
    // error is invisible with a plain blend mode.
    const canvas = el('canvas', 'layer base'); canvas.setAttribute('role', 'img'); canvas.setAttribute('aria-label', t('compareMode_difference')); frame.append(canvas); stage.dataset.background = 'dark';
    const summary = el('span', 'hint', ''), select = el('select'); select.setAttribute('aria-label', t('compareGain'));
    for (const gain of DIFFERENCE_GAINS) { const option = el('option', '', `×${gain}`); option.value = String(gain); select.append(option); }
    select.value = String(DIFFERENCE_GAINS.includes(state.differenceGain) ? state.differenceGain : 16);
    const label = el('label', 'compare-range'); label.append(el('span', '', t('compareGain')), select); controls.append(label, summary);
    const images = [before, after].map(src => new Promise((resolve, reject) => { const img = new Image(); img.onload = () => resolve(img); img.onerror = reject; img.src = src; }));
    const draw = async () => {
      try {
        const [a, b] = await Promise.all(images), width = a.naturalWidth, height = a.naturalHeight;
        if (b.naturalWidth !== width || b.naturalHeight !== height) throw new Error('size');
        const pixels = img => { const c = document.createElement('canvas'); c.width = width; c.height = height; const ctx = c.getContext('2d', { willReadFrequently: true }); ctx.drawImage(img, 0, 0); return ctx.getImageData(0, 0, width, height).data; };
        const result = differencePixels(pixels(a), pixels(b), Number(select.value));
        canvas.width = width; canvas.height = height; fitFrame(width, height); canvas.getContext('2d').putImageData(new ImageData(result.pixels, width, height), 0, 0);
        summary.textContent = result.changed ? t('compareDiffSummary', formatPercent(result.changed / (width * height)), result.max) : t('compareDiffNone');
      } catch { summary.textContent = t('compareDiffUnavailable'); }
    };
    select.addEventListener('change', () => { state.differenceGain = Number(select.value); draw(); }); draw();
  }
  base.addEventListener('load', () => fitFrame(base.naturalWidth, base.naturalHeight));
  if (typeof ResizeObserver === 'function') new ResizeObserver(() => { const layer = frame.querySelector('img,canvas'); fitFrame(layer?.naturalWidth || layer?.width, layer?.naturalHeight || layer?.height); }).observe(stage);
  box.append(stage, controls); return box;
}
