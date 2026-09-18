function size(value) {
  if (value < 1024) return `${value} B`;
  return `${(value / (value < 1048576 ? 1024 : 1048576)).toFixed(2)} ${value < 1048576 ? 'KiB' : 'MiB'}`;
}
async function poll() {
  try {
    const response = await fetch('/api/progress', {cache: 'no-store'});
    if (!response.ok) throw Error('connection');
    const p = await response.json();
    if (p.done) { location.reload(); return; }
    if (p.error) { document.getElementById('progress').textContent = '分析失败：' + p.error; return; }
    document.getElementById('progress').textContent = p.total ? `已分析 ${p.completed} / ${p.total} 个资源` : '正在扫描目录与 Git 忽略规则…';
    document.getElementById('candidate-count').textContent = String(p.candidate_count || 0);
    document.getElementById('saved').textContent = size(p.savings_bytes || 0);
    const results = document.getElementById('partial-results');
    results.replaceChildren();
    for (const item of p.top || []) {
      const row = document.createElement('li');
      row.textContent = `${item.path} · ${size(item.original_bytes)} · 可节省 ${size(item.savings_bytes)}`;
      results.append(row);
    }
    setTimeout(poll, 700);
  } catch {
    document.getElementById('progress').textContent = '本地服务连接中断，请确认终端进程仍在运行。';
    setTimeout(poll, 2000);
  }
}
poll();
