// Pure helpers shared by the report UI. No DOM access here: tests/report-ui.cjs
// loads this file on its own.
const WARNING_KINDS = ['alpha_error_exceeds_policy', 'transparency_presence_changed', 'quality_below_policy'];
const BROWSER_FORMATS = ['png', 'jpeg', 'webp', 'gif'];

function formatNumber(value, locale) { return new Intl.NumberFormat(locale).format(value); }

// Binary units for reading; exact bytes belong in a title/secondary label.
function formatSize(value, locale = 'en-US') {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) return '—';
  if (value < 1024) return `${formatNumber(value, locale)} B`;
  const units = ['KiB', 'MiB', 'GiB', 'TiB', 'PiB'];
  let amount = value / 1024, unit = 0;
  while (amount >= 1024 && unit < units.length - 1) { amount /= 1024; unit++; }
  return `${new Intl.NumberFormat(locale, { minimumFractionDigits: 1, maximumFractionDigits: 2 }).format(amount)} ${units[unit]}`;
}

function formatPercent(ratio) { return Number.isFinite(ratio) ? `${(ratio * 100).toFixed(1)}%` : '—'; }

// Only generated artifact paths may become URLs; anything else is dropped.
function assetUrl(path) {
  if (typeof path !== 'string') return null;
  const parts = path.replaceAll('\\', '/').split('/');
  if (!['previews', 'originals', 'candidates'].includes(parts[0]) || parts.length < 2
    || parts.some(p => !p || p === '.' || p === '..' || [...p].some(ch => ch.charCodeAt(0) < 32 || ch === ':'))) return null;
  return parts.map(encodeURIComponent).join('/');
}

function basename(path) { return String(path || '').split(/[\\/]/).pop(); }

// A warning is a structurally sound lossy candidate that misses a visual policy
// threshold. Every other rejection is a hard failure and is never approvable.
function warningKind(candidate) {
  return candidate && candidate.lossy && !candidate.valid && candidate.artifact
    && WARNING_KINDS.includes(candidate.rejection) ? candidate.rejection : null;
}

// Every threshold the candidate misses; approval must name all of them.
function warningKinds(candidate) {
  if (!warningKind(candidate)) return [];
  const all = Array.isArray(candidate.warnings) ? candidate.warnings.filter(w => WARNING_KINDS.includes(w)) : [];
  return all.length ? all : [candidate.rejection];
}

function recommended(resource) {
  const index = resource && resource.smallest_candidate;
  const candidate = Number.isInteger(index) && index >= 0 ? resource.candidates && resource.candidates[index] : null;
  return candidate && candidate.valid && candidate.artifact ? candidate : null;
}

function recommendedSavings(resource) {
  const candidate = recommended(resource);
  return candidate ? Math.max(0, candidate.savings_bytes || 0) : 0;
}

function hasWarningCandidate(resource) {
  return !!(resource && resource.candidates && resource.candidates.some(c => warningKind(c)));
}

// Empty string when the candidate may be applied (possibly after a warning
// confirmation); otherwise the identifier of the blocking reason.
function blockedReason(resource, candidate) {
  if (!candidate || !candidate.artifact) return 'no_smaller_candidate';
  if (!candidate.valid && !warningKind(candidate)) return 'failed_verification';
  if (resource.resource && resource.resource.conversion_exclusion) return resource.resource.conversion_exclusion;
  const crossing = candidate.format !== resource.resource.format || resource.resource.extension_mismatch;
  if (crossing && resource.resource.format_lock) return resource.resource.format_lock;
  return '';
}

// Amplified per-pixel difference of two straight-RGBA buffers of equal size.
// Colour is compared premultiplied, so invisible colour under alpha 0 does not
// count; an alpha difference is added to every channel. `max` is the largest
// unamplified difference (0-255) and `changed` the number of differing pixels.
function differencePixels(a, b, gain) {
  const pixels = new Uint8ClampedArray(a.length); let max = 0, changed = 0;
  for (let i = 0; i < a.length; i += 4) {
    const alphaA = a[i + 3] / 255, alphaB = b[i + 3] / 255, alpha = Math.abs(a[i + 3] - b[i + 3]); let worst = alpha;
    for (let c = 0; c < 3; c++) {
      const colour = Math.abs(a[i + c] * alphaA - b[i + c] * alphaB); worst = Math.max(worst, colour);
      pixels[i + c] = (colour + alpha) * gain;
    }
    pixels[i + 3] = 255; max = Math.max(max, worst); if (worst >= 0.5) changed++;
  }
  return { pixels, max: Math.round(max), changed };
}

function displayableInBrowser(format) { return BROWSER_FORMATS.includes(format); }

// A similarity finding is one list item. Match any member, retain its reference.
function similarGroupRows(records, groups, matchingIndexes) {
  return groups.filter(group => group.members.some(index => matchingIndexes.has(index)))
    .map(group => records[group.members[0]]).filter(Boolean);
}

// VAP's alpha region stores coverage in its decoded red channel, not video alpha.
function applyVapAlpha(rgb, alpha) {
  if (rgb.length !== alpha.length || rgb.length % 4) throw new Error('VAP plane size mismatch');
  for (let i = 0; i < rgb.length; i += 4) rgb[i + 3] = alpha[i];
  return rgb;
}

// Keep report indexes stable. When the reference disappears, its scores cannot
// be relabeled as comparisons against a different image.
function remainingSimilarGroups(groups, records, missing) {
  return groups.flatMap(group => {
    const members = group.members.filter(index => records[index] && !missing.has(index));
    if (members.length < 2) return [];
    if (members.length === group.members.length) return [group];
    const reference = records[members[0]];
    const identical = reference.sha256 && members.every(index => records[index].sha256 === reference.sha256);
    const resized = members.some(index => records[index].image?.width !== reference.image?.width || records[index].image?.height !== reference.image?.height);
    return [{ ...group, members, kind: identical ? 'identical' : resized ? 'resized' : 'similar',
      comparisons: members[0] === group.members[0] && group.comparisons?.length === group.members.length ? members.map(index => group.comparisons[group.members.indexOf(index)]) : [],
      redundant_bytes: members.slice(1).reduce((bytes, index) => bytes + (records[index].resource?.bytes || 0), 0),
    }];
  });
}
