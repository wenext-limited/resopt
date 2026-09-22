// Only one effect preview may own a decoder or an animation callback at a time.
let effectCleanup = null;
function stopEffectPlayback() { const cleanup = effectCleanup; effectCleanup = null; if (cleanup) cleanup(); }

function vapPlayer(r) {
  const info = r.vap, box = el('section', 'effect-player'), stage = el('div', 'effect-stage');
  box.setAttribute('aria-label', t('vapPlayer'));
  const canvas = el('canvas'); canvas.setAttribute('role', 'img'); canvas.setAttribute('aria-label', t('vapPlayer'));
  const scale = Math.min(1, 640 / Math.max(info.width, info.height));
  canvas.width = Math.max(1, Math.round(info.width * scale)); canvas.height = Math.max(1, Math.round(info.height * scale));
  const alpha = document.createElement('canvas'); alpha.width = canvas.width; alpha.height = canvas.height;
  const rgbContext = canvas.getContext('2d', { willReadFrequently: true }), alphaContext = alpha.getContext('2d', { willReadFrequently: true });
  const video = el('video'); video.hidden = true; video.muted = true; video.loop = true; video.playsInline = true; video.preload = 'auto';
  const controls = el('div', 'effect-controls'), toggle = el('button', '', t('animPlay')), slider = el('input'), label = el('span', 'hint');
  toggle.type = 'button'; toggle.disabled = true; slider.type = 'range'; slider.min = '0'; slider.max = String(info.frames - 1); slider.step = '1'; slider.value = '0'; slider.disabled = true; slider.setAttribute('aria-label', t('effectTimeline'));
  const status = el('p', 'hint', t('effectLoading'));
  stage.append(canvas); controls.append(toggle, slider, label); box.append(el('p', 'hint', t('animInfo', count(info.width), count(info.height), info.fps, count(info.frames))), stage, controls, status, video);
  if (info.dynamic_sources) box.append(el('p', 'status-warn', t('effectDynamic', count(info.dynamic_sources))));
  let disposed = false, callback = null, frameCallback = false, valid = false, objectUrl = null;
  const request = new AbortController();
  const fail = () => { if (disposed) return; valid = false; video.pause(); toggle.disabled = slider.disabled = true; status.textContent = t('effectUnavailable'); };
  const draw = () => {
    if (disposed || !valid || video.readyState < 2) return;
    try {
      rgbContext.drawImage(video, ...info.rgb_frame, 0, 0, canvas.width, canvas.height);
      alphaContext.drawImage(video, ...info.alpha_frame, 0, 0, canvas.width, canvas.height);
      const rgb = rgbContext.getImageData(0, 0, canvas.width, canvas.height), mask = alphaContext.getImageData(0, 0, canvas.width, canvas.height);
      applyVapAlpha(rgb.data, mask.data); rgbContext.putImageData(rgb, 0, 0);
      const frame = Math.min(info.frames - 1, Math.floor(video.currentTime * info.fps));
      slider.value = String(frame); label.textContent = t('animFrame', frame + 1, info.frames);
    } catch { fail(); }
  };
  const tick = () => {
    callback = null; if (disposed || video.paused || !valid) return;
    draw(); schedule();
  };
  const schedule = () => {
    if (disposed || callback !== null) return;
    frameCallback = typeof video.requestVideoFrameCallback === 'function';
    callback = frameCallback ? video.requestVideoFrameCallback(tick) : requestAnimationFrame(tick);
  };
  video.addEventListener('loadedmetadata', () => {
    if (disposed) return;
    if (video.videoWidth !== info.video_width || video.videoHeight !== info.video_height) return fail();
    valid = true; toggle.disabled = slider.disabled = false; status.textContent = t('effectPreviewNote');
    video.currentTime = Math.min((info.frames - 1) / info.fps / 3, video.duration / 3);
  });
  video.addEventListener('loadeddata', draw); video.addEventListener('seeked', draw); video.addEventListener('error', fail);
  toggle.addEventListener('click', async () => {
    if (video.paused) { try { await video.play(); if (!disposed) { toggle.textContent = t('animPause'); schedule(); } } catch { fail(); } }
    else { video.pause(); toggle.textContent = t('animPlay'); }
  });
  slider.addEventListener('input', () => { video.pause(); toggle.textContent = t('animPlay'); video.currentTime = Number(slider.value) / info.fps; });
  effectCleanup = () => {
    disposed = true; request.abort(); video.pause();
    if (callback !== null) { if (frameCallback) video.cancelVideoFrameCallback(callback); else cancelAnimationFrame(callback); }
    video.removeAttribute('src'); video.load(); if (objectUrl) URL.revokeObjectURL(objectUrl);
  };
  const src = assetUrl(r.original_artifact);
  // A bounded local blob stays seekable even when a static report host lacks Range.
  if (src) fetch(src, { signal: request.signal }).then(response => { if (!response.ok) throw new Error('media unavailable'); return response.blob(); }).then(blob => {
    if (disposed) return; if (blob.size > 64 * 1024 * 1024) return fail();
    objectUrl = URL.createObjectURL(blob); video.src = objectUrl;
  }).catch(fail); else fail();
  return box;
}
