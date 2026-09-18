'use strict';
(() => {
const report = JSON.parse(document.getElementById('report-data').textContent);
const themeSelect = document.getElementById('theme');
const themeMedia = window.matchMedia('(prefers-color-scheme: dark)');
try { const value = localStorage.getItem('resopt-theme'); themeSelect.value = ['light','dark','system'].includes(value) ? value : 'system'; } catch {}
function applyTheme() {
  document.documentElement.dataset.theme = themeSelect.value === 'system' ? (themeMedia.matches ? 'dark' : 'light') : themeSelect.value;
}
themeSelect.addEventListener('change',()=>{ applyTheme(); try { localStorage.setItem('resopt-theme',themeSelect.value); } catch {} });
themeMedia.addEventListener('change',applyTheme); applyTheme();
const records = Array.isArray(report.resources) ? report.resources : [];
const $ = id => document.getElementById(id);
const number = n => new Intl.NumberFormat('zh-CN').format(n);
function formatSize(value) {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) return '—';
  if (value < 1024) return `${number(value)} B`;
  const units = ['KiB','MiB','GiB','TiB','PiB']; let amount = value / 1024, unit = 0;
  while (amount >= 1024 && unit < units.length - 1) { amount /= 1024; unit++; }
  return `${new Intl.NumberFormat('zh-CN',{minimumFractionDigits:1,maximumFractionDigits:2}).format(amount)} ${units[unit]}`;
}
const percent = n => Number.isFinite(n) ? `${(n * 100).toFixed(1)}%` : '—';
const best = r => Number.isInteger(r.smallest_candidate) && r.smallest_candidate >= 0 ? r.candidates?.[r.smallest_candidate] : undefined;
const saving = r => best(r)?.valid ? Math.max(0, best(r).savings_bytes || 0) : 0;
const basename = path => String(path || '').split(/[\\/]/).pop();
const pathText = r => String(r.resource?.path || '未命名资源');
function assetUrl(path) {
  if (typeof path !== 'string') return null;
  const parts = path.replaceAll('\\','/').split('/');
  if (!['previews','originals','candidates'].includes(parts[0]) || parts.length < 2 || parts.some(p => !p || p === '.' || p === '..' || /[\u0000-\u001f:]/.test(p))) return null;
  return parts.map(encodeURIComponent).join('/');
}
function el(tag, cls, text) { const node = document.createElement(tag); if (cls) node.className = cls; if (text !== undefined) node.textContent = text; return node; }
function sizeNode(value, cls = '') { const node = el('span', cls, formatSize(value)); if (typeof value === 'number') node.title = `${number(value)} 字节`; return node; }
function setSize(id, value) { $(id).textContent = formatSize(value); $(id).title = `${number(value)} 字节`; }
const statusLabels = {candidates_available:'有更小候选',inspected:'已检测',inventory_only:'仅清点',failed:'检测失败'};
const kindLabels = {image:'图片',vector:'矢量图',video:'视频',audio:'音频',animation:'动效',font:'字体',archive:'压缩包',localization:'本地化',data:'数据文件',unclassified:'未分类'};
function issueLabel(reason) {
  const labels = {alpha_error_exceeds_policy:'Alpha 误差超限',transparency_presence_changed:'透明状态改变',non_finite_decoded_samples:'解码数据异常',dimensions_changed:'尺寸改变',decoded_image_exceeds_max_pixels:'像素数超过分析上限（可用 --max-pixels 调整）',orientation_changed:'方向改变',multiple_frames_not_transcoded:'多帧资源仅检测，未转码',app_icon:'AppIcon 保留原格式',resizing:'拉伸资源保留原格式',below_explicit_input_threshold:'低于指定的输入门槛',source_changed_during_analysis:'分析期间源文件发生变化','non-IDAT chunks changed; candidate rejected':'元数据块变化'};
  if (labels[reason]) return labels[reason];
  if (String(reason).endsWith('_optimization_backend_not_implemented')) return '已纳入清单，暂未提供此类型的压缩分析';
  return String(reason);
}
function perceptualLabel(score) {
  if (score >= 90) return '几乎无差异';
  if (score >= 70) return '轻微差异';
  if (score >= 50) return '可察觉差异';
  return '明显劣化';
}
const candidateLabel = c => `${String(c.format || '').toUpperCase()} · ${c.lossy ? `质量 ${c.quality ?? '—'}` : '无损'}`;
let mode = records.some(r => saving(r) > 0) ? 'candidates' : 'all';
let page = 0, selected = null, chosen = null, background = 'checker';
const pageSize = 50;
let filtered = [];
let operationStates = {}, operationBusy = false, sessionReady = !report.sessionToken, operationMessage = '';
let pendingAction = null;
function preferredCandidate(r) {
  if (!r) return null;
  const state = operationStates[records.indexOf(r)];
  return state?.state === 'applied' ? state.candidate : r.smallest_candidate ?? (r.candidates?.length ? 0 : null);
}
function applicationReason(r, c) {
  if (!c?.valid || !c.artifact) return '请选择通过校验且有体积收益的候选';
  if (r.resource?.conversion_exclusion) return issueLabel(r.resource.conversion_exclusion);
  return '';
}
async function loadOperationStates() {
  if (!report.sessionToken) return;
  try {
    const response = await fetch('/api/state', {headers:{'X-Resopt-Token':report.sessionToken}});
    if (!response.ok) throw new Error('本地服务不可用，请重新启动 resopt serve 并刷新页面');
    operationStates = await response.json(); sessionReady = true; chosen = preferredCandidate(selected);
  } catch(error) { operationMessage = error.message; sessionReady = false; }
  renderDetail();
}
async function performAction(action) {
  operationBusy = true; operationMessage = '正在校验和写入…'; renderDetail();
  try {
    const response = await fetch(`/api/${action.kind}`, {method:'POST',headers:{'Content-Type':'application/json','X-Resopt-Token':report.sessionToken},body:JSON.stringify({resource:action.resource,candidate:action.candidate,approve_lossy:action.approveLossy,plan_token:action.planToken})});
    const result = await response.json();
    if (result.states) operationStates = result.states;
    if (!response.ok) throw new Error(result.error || '操作失败');
    operationMessage = action.kind === 'apply' ? '已优化并保存原图备份。上方对比仍显示分析时的原图。' : '已恢复原图及关联的引用文件。';
  } catch(error) { operationMessage = error.message; }
  finally { operationBusy = false; renderDetail(); }
}
function renderActions(pane, r, c) {
  const block = el('section','apply-panel'); block.setAttribute('aria-label','优化操作');
  if (!report.sessionToken) {
    block.append(el('strong','','在页面应用优化'),el('p','','通过本地服务打开此报告，即可逐张确认应用并恢复原图。'));
    block.append(el('code','','resopt serve <此报告所在目录>')); pane.append(block); return;
  }
  const index = records.indexOf(r), state = operationStates[index];
  const action = el('button','primary','优化这张图片'); action.type='button';
  const reason = applicationReason(r,c);
  action.disabled = operationBusy || !sessionReady || !!reason || (state && state.state !== 'original');
  action.addEventListener('click',async()=>{
    const candidateIndex = chosen;
    operationBusy = true; operationMessage = '正在检查文件引用…'; renderDetail();
    try {
      const response = await fetch('/api/preview', {method:'POST',headers:{'Content-Type':'application/json','X-Resopt-Token':report.sessionToken},body:JSON.stringify({resource:index,candidate:candidateIndex})});
      const plan = await response.json();
      if (!response.ok) throw new Error(plan.error || '无法生成引用迁移计划');
      pendingAction = {kind:'apply',resource:index,candidate:candidateIndex,approveLossy:!!c.lossy,planToken:plan.plan_token};
      $('apply-title').textContent = c.lossy ? '确认有损优化' : '确认无损优化';
      $('apply-confirm').textContent = '确认并应用';
      $('apply-note').textContent = plan.loose_conversion ? '将按报告的忽略规则迁移可识别的静态引用。动态拼接、第三方解码器和项目外引用需要你复核；原图和引用文件均会备份。' : '原文件会备份，可在页面恢复。请先检查原尺寸候选的画质。';
      const references = plan.reference_files || [];
      $('apply-description').textContent = `${plan.source} → ${plan.target}\n${candidateLabel(c)}：${formatSize(r.resource.bytes)} → ${formatSize(c.bytes)}，节省 ${formatSize(c.savings_bytes)}。${c.lossy ? '此操作会有画质损失。' : ''}\n引用文件（${references.length}）：${references.length ? '\n'+references.join('\n') : '未发现需要修改的静态引用'}`;
      $('apply-dialog').showModal(); $('apply-cancel').focus(); operationMessage = '';
    } catch(error) { pendingAction = null; operationMessage = error.message; }
    finally { operationBusy = false; renderDetail(); }
  });
  block.append(action);
  if(state && state.state !== 'original') {
    const restore = el('button','','恢复原图');restore.type='button';restore.disabled=operationBusy || !sessionReady || state.state==='conflict';
    restore.addEventListener('click',()=>{ pendingAction={kind:'restore',resource:index};$('apply-title').textContent='确认恢复原图';$('apply-confirm').textContent='确认恢复';$('apply-note').textContent='如共享引用文件有较新的转换，请先恢复较新的操作。';$('apply-description').textContent=pathText(r)+'\n恢复优化前的文件及资源引用；后续人工修改不会被覆盖。';$('apply-dialog').showModal();$('apply-cancel').focus(); });block.append(restore);
    block.append(el('p','',state.state==='applied'?`已应用：${candidateLabel(r.candidates[state.candidate] || c)} · 可恢复`:state.state==='partial'?'操作未完成，可恢复原图':`文件状态冲突：${state.error || '请先恢复较新的操作'}`));
  } else if(reason) block.append(el('p','',reason));
  if(operationMessage) { const message=el('p','operation-message',operationMessage);message.setAttribute('role','status');block.append(message); }
  pane.append(block);
}
$('apply-cancel').addEventListener('click',()=>{$('apply-dialog').close();pendingAction=null;});
$('apply-dialog').addEventListener('cancel',()=>{pendingAction=null;});
$('apply-confirm').addEventListener('click',()=>{const action=pendingAction;pendingAction=null;$('apply-dialog').close();if(action&&!operationBusy)performAction(action);});
const candidates = records.filter(r => saving(r) > 0).length;
$('total-count').textContent = number(records.length); $('candidate-count').textContent = number(candidates);
setSize('total-savings', report.savings || 0);
$('scope-note').textContent = `总计 ${formatSize(records.reduce((n,r) => n + (r.resource?.bytes || 0), 0))} · 质量 ${(report.options?.qualities || []).join(' / ')}\n候选通过结构及 Alpha 检查，画质仍需审阅。`;
$('mode-candidates').textContent = number(candidates); $('mode-images').textContent = number(records.filter(r => r.resource?.kind === 'image').length); $('mode-all').textContent = number(records.length);
for (const format of [...new Set(records.map(r => r.resource?.format).filter(Boolean))].sort()) { const option = el('option','',format.toUpperCase()); option.value = format; $('format-filter').append(option); }
function filterRecords() {
  const query = $('search').value.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean); const format = $('format-filter').value;
  filtered = records.filter(r => (mode !== 'candidates' || saving(r) > 0) && (mode !== 'images' || r.resource?.kind === 'image') && (format === 'all' || r.resource?.format === format) && query.every(q => `${pathText(r)} ${r.resource?.format} ${r.resource?.kind}`.toLocaleLowerCase().includes(q)));
  const sort = $('sort').value;
  filtered.sort((a,b) => (sort === 'savings' ? saving(b) - saving(a) : sort === 'size' ? (b.resource?.bytes || 0) - (a.resource?.bytes || 0) : basename(pathText(a)).localeCompare(basename(pathText(b)))) || pathText(a).localeCompare(pathText(b)));
  page = 0; selected = filtered[0] || null; chosen = preferredCandidate(selected);
  document.querySelectorAll('.mode').forEach(button => { const active = button.dataset.mode === mode; button.classList.toggle('active',active); button.setAttribute('aria-pressed',String(active)); });
  renderList(); renderDetail();
}
function renderList() {
  const list = $('results'); list.replaceChildren();
  const start = page * pageSize; const items = filtered.slice(start,start+pageSize);
  if (!items.length) { const empty = el('div','empty'); empty.append(el('strong','','没有匹配的资源'),el('span','','试试其他关键词或格式。')); const reset = el('button','','清除筛选'); reset.addEventListener('click',() => { $('search').value=''; $('format-filter').value='all'; mode='all'; filterRecords(); }); empty.append(reset); list.append(empty); }
  for (const r of items) {
    const button = el('button','resource-row'); button.type='button'; button.setAttribute('role','option'); button.setAttribute('aria-selected',String(r===selected)); button.title=pathText(r);
    const identity=el('span','identity'), thumb=el('span','thumb'); const src=assetUrl(r.original_preview);
    if(src){ const img=el('img'); img.src=src; img.alt=''; img.loading='lazy'; thumb.append(img); } else thumb.textContent=String(r.resource?.format || 'FILE').toUpperCase().slice(0,5);
    const copy=el('span','file-copy'); copy.append(el('span','filename',basename(pathText(r))),el('span','file-context',`${String(r.resource?.format || '').toUpperCase()} · ${pathText(r).replace(/[\\/][^\\/]+$/,'')}`)); identity.append(thumb,copy);
    const saved=el('span',saving(r)>0?'number saving':'number status-muted'); saved.append(saving(r)>0?sizeNode(saving(r)):el('span','','—')); if(saving(r)>0) saved.append(el('small','',`−${percent(saving(r)/(r.resource.bytes || 1))}`));
    button.append(identity,sizeNode(r.resource?.bytes,'number'),saved);
    button.addEventListener('click',()=>{selectRecord(r);if(window.matchMedia('(max-width: 760px)').matches)$('inspector').scrollIntoView({block:'start',behavior:window.matchMedia('(prefers-reduced-motion: reduce)').matches?'instant':'smooth'});});
    button.addEventListener('keydown',event=>{ if(!['ArrowUp','ArrowDown'].includes(event.key))return; event.preventDefault(); const next=items.indexOf(r)+(event.key==='ArrowDown'?1:-1); if(items[next]){ selectRecord(items[next]); list.children[next]?.focus(); } });
    list.append(button);
  }
  $('range').textContent=filtered.length?`${number(start+1)}–${number(Math.min(start+pageSize,filtered.length))} / ${number(filtered.length)} 个资源`:'0 个资源';
  $('page-number').textContent=`${filtered.length?page+1:0} / ${Math.ceil(filtered.length/pageSize)}`;
  $('previous').disabled=page===0; $('next').disabled=start+pageSize>=filtered.length;
}
function selectRecord(r) { selected=r; chosen=preferredCandidate(r); const rows=$('results').children; const items=filtered.slice(page*pageSize,(page+1)*pageSize); [...rows].forEach((row,i)=>row.setAttribute('aria-selected',String(items[i]===r))); renderDetail(); }
function drawPreview(r,c,original) {
  const figure=el('figure','preview-frame'); const caption=el('div','preview-label'); caption.append(el('span','',original?'原图':c?candidateLabel(c):'候选'));
  const raw=assetUrl(original?r.original_artifact:c?.artifact); if(raw){ const link=el('a','', '打开原尺寸 ↗'); link.href=raw; link.target='_blank'; link.rel='noopener'; caption.append(link); }
  const canvas=el('div','canvas'); canvas.dataset.background=background; const src=assetUrl(original?r.original_preview:c?.preview);
  if(src){const img=el('img');img.src=src;img.alt=original?`${basename(pathText(r))} 原图预览`:`${candidateLabel(c)} 候选预览`;canvas.append(img);}else canvas.append(el('div','placeholder',original?'未保存原图预览':c?.rejection?issueLabel(c.rejection):'未保存更小的候选'));
  const size=el('figcaption','preview-size');size.append(sizeNode(original?r.resource?.bytes:c?.bytes || null));if(!original&&c?.savings_bytes>0)size.append(el('small','',`节省 ${formatSize(c.savings_bytes)} · ${percent(c.savings_bytes/(r.resource.bytes||1))}`));
  figure.append(caption,canvas,size);return figure;
}
function renderDetail() {
  const pane=$('inspector');pane.replaceChildren(); if(!selected){const empty=el('div','empty');empty.append(el('strong','','选择一个资源'),el('span','','查看体积与画质对比'));pane.append(empty);return;}
  const r=selected, variants=Array.isArray(r.candidates)?r.candidates:[], c=variants[chosen] || null;
  const heading=el('div','detail-heading'),title=el('div');title.append(el('div','eyebrow',kindLabels[r.resource?.kind] || '资源'),el('h1','',basename(pathText(r))));heading.append(title,el('span','tag',statusLabels[r.status] || r.status));pane.append(heading,el('p','detail-path',pathText(r)));
  const facts=el('div','facts');facts.append(el('span','',String(r.resource?.format || '').toUpperCase()));
  if(r.image){facts.append(el('span','',`${number(r.image.width)} × ${number(r.image.height)}`),el('span','',r.image.has_transparent_pixels?'含透明像素':'无透明像素'),el('span','',`${r.image.frames} 帧`));if(r.resource.extension_mismatch)facts.append(el('span','','扩展名与实际格式不一致'));}else facts.append(el('span','',statusLabels[r.status] || r.status));
  pane.append(facts);
  if(r.image){
    const controls=el('div','comparison-controls'); const label=el('label','candidate-label','对比方案');label.htmlFor='candidate-select';const select=el('select');select.id='candidate-select';select.setAttribute('aria-label','对比方案');select.disabled=!variants.length;
    if(!variants.length){const o=el('option','','暂无候选');select.append(o);}else variants.forEach((v,i)=>{const o=el('option','',`${candidateLabel(v)} · ${formatSize(v.bytes || null)}${!v.valid?' · 未通过校验':''}`);o.value=String(i);select.append(o);}); if(c)select.value=String(chosen);label.append(select);select.addEventListener('change',()=>{chosen=Number(select.value);renderDetail();$('candidate-select').focus();});controls.append(label);
    const backgrounds=el('div','backgrounds');backgrounds.setAttribute('role','group');backgrounds.setAttribute('aria-label','预览背景');for(const [value,name] of [['checker','棋盘背景'],['light','白色背景'],['dark','深色背景']]){const button=el('button','swatch');button.dataset.background=value;button.setAttribute('aria-label',name);button.title=name;button.setAttribute('aria-pressed',String(background===value));button.addEventListener('click',()=>{background=value;pane.querySelectorAll('.canvas').forEach(n=>n.dataset.background=value);backgrounds.querySelectorAll('button').forEach(n=>n.setAttribute('aria-pressed',String(n.dataset.background===value)));});backgrounds.append(button);}controls.append(backgrounds);pane.append(controls);
    const comparison=el('div','comparison');comparison.append(drawPreview(r,c,true),drawPreview(r,c,false));pane.append(comparison,el('p','preview-note','缩略图用于初筛；请打开原尺寸候选确认画质。有损质量数值不代表节省比例。'));
  }
  if(r.image) renderActions(pane,r,c);
  if(variants.length){
    const heading=el('div','metrics-title','全部方案');heading.append(el('span','',`${variants.length} 个方案`));pane.append(heading);
    const wrap=el('div','table-scroll'),table=el('table'),head=el('thead'),row=el('tr');for(const [label,title] of [['方案','编码格式与质量'],['体积','悬停查看精确字节数'],['节省','相对原文件的源体积收益'],['感知画质','SSIMULACRA2，取黑、白、灰三种背景下的最低分；100 为完全一致，90 以上通常难以察觉'],['RGB 误差','sRGB 预乘 Alpha 像素的 MAE，0–255 标度；越低越接近'],['Alpha 误差','最大 Alpha 误差，按百分比显示'],['状态','结构与 Alpha 校验结果，不代表视觉验收']]){const th=el('th','',label);th.scope='col';th.title=title;row.append(th);}head.append(row);table.append(head);const body=el('tbody');
    variants.forEach((v,i)=>{const row=el('tr',i===Number(chosen)?'selected':'');const name=el('td'),button=el('button','variant-button',candidateLabel(v));button.addEventListener('click',()=>{chosen=i;renderDetail();});name.append(button);const bytes=el('td');bytes.append(sizeNode(v.bytes || null));const saved=el('td',v.savings_bytes>0?'status-ok':'status-muted',v.savings_bytes>0?formatSize(v.savings_bytes):'—');saved.title=v.savings_bytes?`${number(v.savings_bytes)} 字节`:'没有体积收益';const score=v.difference?.ssimulacra2;const perceptual=el('td',score==null?'status-muted':score>=90?'status-ok':'',score==null?'—':Number(score).toFixed(1));if(score!=null)perceptual.append(el('small','metric-sub',perceptualLabel(score)));const rgb=el('td','',v.difference?Number(v.difference.rgb_mae_255).toFixed(3):'—');if(v.difference){const psnr=`${v.difference.psnr_db==null?'∞':Number(v.difference.psnr_db).toFixed(2)} dB`;rgb.title=`PSNR ${psnr}`;rgb.append(el('small','metric-sub',psnr));}
      const alpha=el('td','',v.difference?`${(v.difference.max_alpha_error*100).toFixed(2)}%`:'—');const state=el('td',v.valid&&v.artifact?'status-ok':'status-muted',v.rejection?issueLabel(v.rejection):v.artifact?'可审阅':'无体积收益');state.title=v.rejection || '画质仍需审阅';row.append(name,bytes,saved,perceptual,rgb,alpha,state);body.append(row);});table.append(body);wrap.append(table);pane.append(wrap);
  }
  if(r.issues?.length)pane.append(el('div','reasons',r.issues.map(issueLabel).join('；')));
  const notes=el('details'),summary=el('summary','','分析口径与限制');notes.append(summary,el('p','',`Alpha 最大误差上限：${((report.options?.max_alpha_error || 0)*100).toFixed(3)}%。仅按当前校验条件选择最小候选，感知画质（SSIMULACRA2）与 RGB/PSNR 指标均不替代视觉检查。`),el('p','','非图片资源目前仅清点，多帧图片不会转成单帧。离线报告仅供审阅；本地服务支持逐张确认应用。不测量编译后的 App 包体。'));pane.append(notes);
  if(!window.matchMedia('(prefers-reduced-motion: reduce)').matches)pane.animate([{opacity:.7,transform:'translateY(2px)'},{opacity:1,transform:'none'}],{duration:140,easing:'ease-out'});
  pane.scrollTop=0;
}
$('search').addEventListener('input',filterRecords);$('format-filter').addEventListener('change',filterRecords);$('sort').addEventListener('change',filterRecords);
document.querySelectorAll('.mode').forEach(button=>button.addEventListener('click',()=>{mode=button.dataset.mode;filterRecords();}));
function changePage(delta){page+=delta;selected=filtered[page*pageSize] || null;chosen=preferredCandidate(selected);renderList();renderDetail();$('results').scrollTop=0;}
$('previous').addEventListener('click',()=>changePage(-1));$('next').addEventListener('click',()=>changePage(1));filterRecords();loadOperationStates();
})();
