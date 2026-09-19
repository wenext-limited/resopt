// UI strings. English is the default; Simplified Chinese follows the browser
// language or the header selector. `{0}` placeholders are positional.
const MESSAGES = {
  en: {
    title: 'Resource analysis', live: 'Local session · changes need your confirmation', offline: 'Offline report · read-only',
    theme: 'Theme', languageAuto: 'Auto', themeSystem: 'System', themeLight: 'Light', themeDark: 'Dark', language: 'Language', json: 'View JSON ↗',
    scanning: 'Scanning the project and Git ignore rules…', analyzing: 'Analyzed {0} of {1} resources', cancel: 'Stop analysis',
    cancelling: 'Stopping after the files in progress…', cancelled: 'Analysis was stopped early. Unfinished resources are listed as “Not analyzed”. Run resopt web again to continue; with the cache enabled (the default), finished work is reused.',
    failed: 'Analysis failed: {0}', failedAction: 'Fix the cause shown above, then run resopt web again. No project files were changed.',
    disconnected: 'Lost connection to the local resopt process. Check that it is still running in your terminal, then reload this page.',
    ready: 'Analysis complete', localOnly: 'Files are read from this computer and never uploaded.',
    statResources: 'Resources', statOpportunities: 'Opportunities', statSavings: 'Recommended savings', statWarnings: 'Warnings to review', statApplied: 'Applied savings',
    scope: 'Total {0} · quality levels {1} · minimum score {2}\nSource-file sizes on disk, not build-target membership or app download size.',
    modeCandidates: 'Opportunities', modeWarnings: 'Warnings', modeDuplicates: 'Duplicates', modeApplied: 'Applied', modeImages: 'Images', modeUnsupported: 'No optimizer', modeFailed: 'Failed', modeAll: 'All',
    search: 'Search by name or path…', allFormats: 'All formats', sortSavings: 'Savings ↓', sortSize: 'Original size ↓', sortName: 'Name A–Z', sortScore: 'Lowest score first',
    batch: 'Batch apply…', restoreSelected: 'Restore selected {0}…', restoreAll: 'Restore all…', colResource: 'Resource', colSize: 'Original', colSavings: 'Savings',
    noMatch: 'No matching resources', noMatchHint: 'Try another keyword, format or view.', clearFilters: 'Clear filters', waiting: 'Results appear here as files finish.',
    range: '{0}–{1} of {2}', none: '0 resources', selectedCount: '{0} selected', previous: 'Previous page', next: 'Next page',
    multiSelected: '{0} resources selected', multiSelectedHint: 'Shift-click selects a range; Option/Alt-click toggles individual resources. Batch apply and restore selected in the toolbar are limited to this selection.', batchSelected: 'Batch apply {0} selected…',
    select: 'Select a resource', selectHint: 'Compare size and quality for each candidate.',
    original: 'Original', candidate: 'Candidate', openFull: 'Open full size ↗', compare: 'Compare at full size', compareTitle: 'Full-size comparison', compareHint: 'Pick a mode; sliders also respond to ← → keys. Scroll to pan.',
    compareUnavailable: 'This browser cannot display {0} files; the comparison uses the PNG preview. Open the file in Preview or Safari to inspect it at full size.',
    compareModes: 'Comparison mode', 'compareMode_two-up': '2-up', compareMode_swipe: 'Swipe', compareMode_onion: 'Onion skin', compareMode_difference: 'Difference', compareSwipe: 'Split position', compareOnion: 'Candidate opacity', compareGain: 'Amplify',
    compareDiffSummary: '{0} of pixels differ · largest difference {1}/255 (before amplification)', compareDiffNone: 'No pixel differs: the two pictures are identical', compareDiffUnavailable: 'The difference could not be computed for these pictures',
    close: 'Close', background: 'Preview background', bgChecker: 'Checkerboard', bgLight: 'White', bgDark: 'Dark', compareWith: 'Compare with',
    noCandidates: 'No candidates', notValid: 'warning', failedCheck: 'failed', previewNote: 'Thumbnails are for triage; inspect the full-size candidate before accepting a lossy change. Quality is an encoder parameter, not a savings percentage.',
    lossless: 'lossless', nearLossless: 'near-lossless (q100)', quality: 'quality {0}', allCandidates: 'All candidates', nCandidates: '{0} candidates',
    thCandidate: 'Candidate', thSize: 'Size', thSaved: 'Saved', thScore: 'Perceptual', thRgb: 'RGB error', thAlpha: 'Alpha error', thStatus: 'Status',
    thScoreHint: 'SSIMULACRA2, lowest of black, white and gray backdrops. 100 is identical; 90+ is usually imperceptible.', thRgbHint: 'Mean absolute error of premultiplied sRGB on a 0–255 scale, with PSNR below.', thAlphaHint: 'Largest per-pixel alpha difference.',
    score90: 'near identical', score70: 'minor', score50: 'noticeable', score0: 'degraded',
    statusOk: 'Recommended', statusReview: 'Passes checks', statusNoGain: 'Not smaller', exactBytes: '{0} bytes',
    apply: 'Apply this candidate', restore: 'Restore original', applying: 'Verifying and writing…', checking: 'Checking references…',
    applied: 'Applied: {0}. The original is backed up and can be restored.', operationPartial: 'Incomplete', operationConflict: 'Conflict', partial: 'This operation was interrupted. Restore the original to return to a consistent state.',
    conflict: 'Files changed since this operation: {0}', conflictAction: 'Restore newer operations first, or revert your manual edit, then try again.',
    appliedOk: 'Applied. The original is backed up in the report directory.', restoredOk: 'Restored the original file and its references.',
    offlineApply: 'To apply changes, open this report through the local server:', analysisRunning: 'Changes can be applied when the analysis finishes.',
    confirmLossless: 'Apply lossless optimization', confirmLossy: 'Apply lossy candidate', confirmWarning: 'Warning: review before applying', confirmRestore: 'Restore original',
    confirmApply: 'Confirm and apply', confirmAccept: 'I reviewed the candidate — accept and apply', confirmRestoreButton: 'Restore', cancelButton: 'Cancel',
    dialogChange: '{0} → {1}\n{2}: {3} → {4}, saving {5}.', dialogLossy: 'This change is lossy.', dialogRefs: 'Files with references to update ({0}):', dialogNoRefs: 'No static references need to change.',
    dialogLoose: 'Statically resolvable references are migrated under the same ignore rules as the report. Dynamic names, third-party decoders and references outside the project need your review. Originals and edited files are backed up.',
    dialogBackup: 'The original is backed up and can be restored from this page.', dialogRestore: 'Restores the file and any migrated references. Later manual edits are never overwritten. If a newer operation shares a file, restore that one first.',
    dialogAndroid: 'Android resource “{0}” ({1}) keeps its name, so XML and R.{1}.{0} references stay valid. minSdk {2}. Found {3} XML and {4} code references.', dialogDynamic: '{0} source files call getIdentifier(); names built at runtime cannot be checked.',
    dialogCompiled: 'Measured with AAPT2: compiled resource {0} → {1}. The build re-compresses PNG files, so this — not the source size — is what gets packaged.',
    warn_alpha_error_exceeds_policy: 'Alpha error above the threshold', warn_transparency_presence_changed: 'Transparency was added or removed', warn_quality_below_policy: 'Perceptual score below the minimum',
    warnDetail_alpha_error_exceeds_policy: 'Largest alpha difference {0}. Edges or translucent areas may look different.', warnDetail_transparency_presence_changed: 'The candidate changes whether the image has transparent pixels.', warnDetail_quality_below_policy: 'Score {0} is below the minimum {1}. Visible artifacts are likely.',
    warnRecorded: 'Your approval is recorded with the operation.',
    batchTitle: 'Batch apply', batchIntro: 'Choose what may be applied. Every file is its own restorable operation.', batchLossless: 'Lossless candidates', batchLossy: 'Lossy candidates that pass every check',
    batchCross: 'Allow format changes (renames files and migrates references)', batchAlpha: 'Also accept Alpha warnings', batchQuality: 'Also accept quality warnings', batchMinScore: 'Minimum perceptual score for lossy candidates',
    batchScope: 'Only the {0} resources in the current view', batchSelectedScope: 'Only the {0} selected resources', batchPreview: 'Preview batch', batchNothing: 'No candidates match this policy.',
    batchSummary: '{0} files · saves {1}\n{2} lossy · {3} with accepted warnings · {4} format changes', batchSelectedSummary: '{0} selected · {1} eligible · {2} excluded · saves {3}\n{4} lossy · {5} with accepted warnings · {6} format changes', batchConfirm: 'Apply {0} files', batchRunning: 'Applied {0} of {1}…', batchStop: 'Stop after current file',
    batchExcludedTitle: 'Excluded selected resources ({0})', batchExcludedLossy: 'Requires lossy candidates', batchExcludedLossless: 'Requires lossless candidates', batchExcludedCross: 'Requires format changes', batchExcludedWarning: 'Requires explicit warning approval', batchExcludedScore: 'Below the selected score floor', batchExcludedApplied: 'Already applied or awaiting restore', batchExcludedUnavailable: 'No candidate matches this policy',
    batchDone: 'Applied {0} · failed {1} · skipped {2} · saved {3}', batchFailures: 'Files that were not applied', batchStopped: 'Stopped. Applied files remain applied and can be restored individually or with “Restore all”.',
    restoreSelectedTitle: 'Restore selected changes', restoreSelectedIntro: 'Restores the {0} selected applied or interrupted operations. Unselected operations and files edited afterwards are left untouched.', restoreSelectedConfirm: 'Restore {0} selected',
    restoreAllTitle: 'Restore all applied changes', restoreAllIntro: 'Restores every applied or interrupted operation in this report, newest dependencies first. Files edited afterwards are left untouched and reported.', restoreAllConfirm: 'Restore {0} operations', restoreRunning: 'Restored {0} of {1}…', restoreAllDone: 'Restored {0} · not restored {1}', restoreFailures: 'Files that were not restored', restoreStopped: 'Stopped. Restored files remain original; the remaining operations can be restored later.',
    methodology: 'How results are measured', methodology1: 'Alpha threshold {0}; minimum SSIMULACRA2 score {1}. The recommended candidate is the smallest one that passes every check. Scores guide review; they do not replace looking at the image.',
    methodology2: 'Animated images are never flattened. Savings are source-file bytes, not compiled app size.', footerUnits: 'Sizes use binary units (KiB = 1,024 bytes); hover for exact bytes.', footerScope: 'Source savings only · not app download size',
    dup_identical: 'Identical files', dup_resized: 'Same picture at different sizes', dup_similar: 'Near-duplicate pictures', dupRedundant: '{0} files · {1} beyond the largest copy',
    dupHint: 'Found by comparing decoded pixels, not names. Scale variants of one asset (@2x/@3x, density folders) are not reported. Removing a copy means updating the code that uses it, so nothing is merged automatically.',
    animInfo: '{0} × {1} · {2} fps · {3} frames', animPlay: 'Play', animPause: 'Pause', animFrame: 'Frame {0} of {1}', animStatic: 'Poster frame {0}. Open this report with resopt serve to play the animation.',
    android: 'Android', androidInfo: '{0}/{1} · name “{2}” · qualifiers {3}', notes: 'Notes', perf: 'Analysis took {0} s ({1} workers, {2} cache hits, {3} duplicates reused)',
    capabilityTitle: 'Available on this computer', tool_missing: '{0} not found — {1} Install: {2}',
    kind_image: 'Image', kind_vector: 'Vector', kind_video: 'Video', kind_audio: 'Audio', kind_animation: 'Animation', kind_font: 'Font', kind_archive: 'Archive', kind_localization: 'Localization', kind_data: 'Data', kind_unclassified: 'Other',
    status_candidates_available: 'Smaller candidate', status_inspected: 'No smaller candidate', status_excluded: 'Excluded', status_unsupported: 'No optimizer', status_inventory_only: 'No optimizer', status_failed: 'Analysis failed', status_not_analyzed: 'Not analyzed',
    transparent: 'has transparency', opaque: 'opaque', frames: '{0} frames', mismatch: 'extension does not match content',
    issue_android_nine_patch: 'Nine-patch: stays PNG so AAPT can read its stretch markers; lossless optimization only', issue_android_launcher_icon: 'Launcher icon: keeps its format; lossless optimization only', issue_android_raw_resource: 'res/raw file: read as raw bytes by app code, so the format is kept',
    issue_android_min_sdk_unknown: 'minSdk could not be read from Gradle files; pass --android-min-sdk to enable WebP candidates', issue_app_icon: 'App icon: kept as is', issue_resizing: 'Resizable (sliced) image: kept as is',
    issue_multiple_frames_not_transcoded: 'Animated image: inspected only, never flattened', issue_below_explicit_input_threshold: 'Below the configured input size', issue_source_changed_during_analysis: 'The file changed while it was being analyzed; run the analysis again',
    issue_decoded_image_exceeds_max_pixels: 'Larger than the pixel limit (raise it with --max-pixels)', issue_dimensions_changed: 'Dimensions changed', issue_orientation_changed: 'Orientation changed', issue_no_smaller_candidate: 'No smaller candidate was produced', issue_failed_verification: 'This candidate failed verification and cannot be applied',
    issue_ffprobe_not_installed: 'Install ffmpeg to see codec, duration and bitrate for this file', mediaInfo: '{0} · {1} s · {2} kbit/s',
    issue_webp_candidates_disabled: 'WebP candidates were turned off for this run (--no-webp)',
    issue_preview_unavailable: 'This animation could not be rendered for preview ({0}); optimization is verified on its bytes and is unaffected',
    issue_near_lossless: 'Near-lossless: the encoder’s highest quality. Apple’s HEIC encoder has no lossless mode, so a small share of samples still changes; see the measured error',
    issue_palette: 'Reduced to a palette of {0} colours (lossy); the file stays a PNG',
    issue_backend: 'Listed in the inventory; resopt has no optimizer for this type yet', issue_macos: 'Decoding this format needs Apple ImageIO (macOS)', issue_min_sdk: 'WebP here needs API {1}+, but minSdk is {0}', issue_metadata: 'Not carried into the new file: {0}',
  },
  'zh-CN': {
    title: '资源分析', live: '本地会话 · 修改需逐项确认', offline: '离线报告 · 仅供审阅',
    theme: '主题', languageAuto: '自动', themeSystem: '跟随系统', themeLight: '浅色', themeDark: '深色', language: '语言', json: '查看 JSON ↗',
    scanning: '正在扫描目录与 Git 忽略规则…', analyzing: '已分析 {0} / {1} 个资源', cancel: '停止分析',
    cancelling: '处理完当前文件后停止…', cancelled: '分析已提前停止，未完成的资源标记为“未分析”。再次运行 resopt web 可继续；缓存开启时（默认）会复用已完成的结果。',
    failed: '分析失败：{0}', failedAction: '请处理上述原因后重新运行 resopt web。项目文件未被修改。',
    disconnected: '与本地 resopt 进程的连接已中断。请确认终端中的进程仍在运行，然后刷新页面。',
    ready: '分析完成', localOnly: '文件仅从本机读取，不会上传。',
    statResources: '资源文件', statOpportunities: '可优化', statSavings: '推荐可节省', statWarnings: '待审核警告', statApplied: '已应用节省',
    scope: '总计 {0} · 质量档位 {1} · 最低评分 {2}\n统计的是磁盘上的源文件体积，不代表构建目标归属或 App 下载体积。',
    modeCandidates: '可优化', modeWarnings: '有警告', modeDuplicates: '重复图片', modeApplied: '已应用', modeImages: '图片', modeUnsupported: '暂无优化器', modeFailed: '失败', modeAll: '全部',
    search: '搜索文件名或路径…', allFormats: '全部格式', sortSavings: '节省量 ↓', sortSize: '原始体积 ↓', sortName: '文件名 A–Z', sortScore: '评分从低到高',
    batch: '批量应用…', restoreSelected: '取消应用已选 {0} 个…', restoreAll: '全部恢复…', colResource: '资源', colSize: '原始体积', colSavings: '可节省',
    noMatch: '没有匹配的资源', noMatchHint: '试试其他关键词、格式或视图。', clearFilters: '清除筛选', waiting: '文件分析完成后会陆续显示在这里。',
    range: '{0}–{1} / {2}', none: '0 个资源', selectedCount: '已选 {0} 个', previous: '上一页', next: '下一页',
    multiSelected: '已选 {0} 个资源', multiSelectedHint: '按住 Shift 点击选择连续范围；按住 Option/Alt 点击切换单个资源。工具栏中的“批量应用”和“取消应用已选”仅处理这些已选资源。', batchSelected: '批量应用已选 {0} 个…',
    select: '选择一个资源', selectHint: '对比每个候选的体积与画质。',
    original: '原图', candidate: '候选', openFull: '打开原尺寸 ↗', compare: '原尺寸对比', compareTitle: '原尺寸对比', compareHint: '选择对比方式；滑块也支持 ← → 键。滚动可平移。',
    compareUnavailable: '当前浏览器无法显示 {0} 文件，对比使用 PNG 预览。可用“预览”或 Safari 打开文件查看原尺寸。',
    compareModes: '对比方式', 'compareMode_two-up': '并排', compareMode_swipe: '滑动', compareMode_onion: '洋葱皮', compareMode_difference: '差异', compareSwipe: '分割位置', compareOnion: '候选不透明度', compareGain: '放大',
    compareDiffSummary: '{0} 的像素有差异 · 最大差值 {1}/255（放大前）', compareDiffNone: '没有像素差异：两张图片完全一致', compareDiffUnavailable: '无法计算这两张图片的差异',
    close: '关闭', background: '预览背景', bgChecker: '棋盘格', bgLight: '白色', bgDark: '深色', compareWith: '对比方案',
    noCandidates: '暂无候选', notValid: '警告', failedCheck: '未通过', previewNote: '缩略图用于初筛；接受有损修改前请查看原尺寸候选。质量是编码器参数，不是节省比例。',
    lossless: '无损', nearLossless: '近无损（q100）', quality: '质量 {0}', allCandidates: '全部方案', nCandidates: '{0} 个方案',
    thCandidate: '方案', thSize: '体积', thSaved: '节省', thScore: '感知画质', thRgb: 'RGB 误差', thAlpha: 'Alpha 误差', thStatus: '状态',
    thScoreHint: 'SSIMULACRA2，取黑、白、灰三种背景下的最低分；100 为完全一致，90 以上通常难以察觉。', thRgbHint: '预乘 sRGB 的平均绝对误差（0–255），下方为 PSNR。', thAlphaHint: '单像素最大 Alpha 差异。',
    score90: '几乎无差异', score70: '轻微差异', score50: '可察觉', score0: '明显劣化',
    statusOk: '推荐', statusReview: '通过校验', statusNoGain: '没有更小', exactBytes: '{0} 字节',
    apply: '应用此候选', restore: '恢复原图', applying: '正在校验并写入…', checking: '正在检查引用…',
    applied: '已应用：{0}。原文件已备份，可随时恢复。', operationPartial: '未完成', operationConflict: '有冲突', partial: '此操作曾被中断。请恢复原图以回到一致状态。',
    conflict: '操作后文件又被修改：{0}', conflictAction: '请先恢复较新的操作，或撤销手动修改后重试。',
    appliedOk: '已应用。原文件已备份在报告目录。', restoredOk: '已恢复原文件及其引用。',
    offlineApply: '如需应用修改，请通过本地服务打开此报告：', analysisRunning: '分析完成后即可应用修改。',
    confirmLossless: '应用无损优化', confirmLossy: '应用有损候选', confirmWarning: '警告：请先审核再应用', confirmRestore: '恢复原图',
    confirmApply: '确认并应用', confirmAccept: '我已查看候选 — 接受并应用', confirmRestoreButton: '恢复', cancelButton: '取消',
    dialogChange: '{0} → {1}\n{2}：{3} → {4}，节省 {5}。', dialogLossy: '此修改为有损。', dialogRefs: '需要更新引用的文件（{0}）：', dialogNoRefs: '没有需要修改的静态引用。',
    dialogLoose: '将按报告的忽略规则迁移可静态识别的引用。动态拼接的名称、第三方解码器及项目外引用需要你自行复核。原文件与被修改的文件都会备份。',
    dialogBackup: '原文件会备份，可在本页面恢复。', dialogRestore: '恢复文件及已迁移的引用；之后的手动修改不会被覆盖。如有较新的操作共用同一文件，请先恢复较新的操作。',
    dialogAndroid: 'Android 资源“{0}”（{1}）名称保持不变，XML 与 R.{1}.{0} 引用继续有效。minSdk {2}。发现 {3} 处 XML 引用、{4} 处代码引用。', dialogDynamic: '{0} 个源文件调用了 getIdentifier()；运行时拼接的名称无法静态检查。',
    dialogCompiled: 'AAPT2 实测：编译后资源 {0} → {1}。构建时会重新压缩 PNG，因此实际打包的是这个体积，而不是源文件体积。',
    warn_alpha_error_exceeds_policy: 'Alpha 误差超过阈值', warn_transparency_presence_changed: '透明状态发生变化', warn_quality_below_policy: '感知评分低于最低要求',
    warnDetail_alpha_error_exceeds_policy: '最大 Alpha 差异 {0}，边缘或半透明区域可能有变化。', warnDetail_transparency_presence_changed: '候选改变了图片是否含透明像素。', warnDetail_quality_below_policy: '评分 {0} 低于最低要求 {1}，可能出现可见瑕疵。',
    warnRecorded: '你的确认会随操作一并记录。',
    batchTitle: '批量应用', batchIntro: '选择允许应用的范围。每个文件都是独立且可恢复的操作。', batchLossless: '无损候选', batchLossy: '通过全部校验的有损候选',
    batchCross: '允许更换格式（会重命名文件并迁移引用）', batchAlpha: '同时接受 Alpha 警告', batchQuality: '同时接受画质警告', batchMinScore: '有损候选的最低感知评分',
    batchScope: '仅当前视图中的 {0} 个资源', batchSelectedScope: '仅处理已选的 {0} 个资源', batchPreview: '预览批量操作', batchNothing: '没有符合该策略的候选。',
    batchSummary: '{0} 个文件 · 节省 {1}\n{2} 个有损 · {3} 个接受警告 · {4} 个更换格式', batchSelectedSummary: '已选 {0} 个 · 可应用 {1} 个 · 已排除 {2} 个 · 节省 {3}\n{4} 个有损 · {5} 个接受警告 · {6} 个更换格式', batchConfirm: '应用 {0} 个文件', batchRunning: '已应用 {0} / {1}…', batchStop: '处理完当前文件后停止',
    batchExcludedTitle: '已排除的已选资源（{0}）', batchExcludedLossy: '需要允许有损候选', batchExcludedLossless: '需要允许无损候选', batchExcludedCross: '需要允许更换格式', batchExcludedWarning: '需要明确接受警告', batchExcludedScore: '低于设置的最低评分', batchExcludedApplied: '已经应用或等待恢复', batchExcludedUnavailable: '没有符合当前策略的候选',
    batchDone: '已应用 {0} · 失败 {1} · 跳过 {2} · 节省 {3}', batchFailures: '未应用的文件', batchStopped: '已停止。已应用的文件保持不变，可逐个恢复或使用“全部恢复”。',
    restoreSelectedTitle: '取消应用已选图片', restoreSelectedIntro: '恢复已选的 {0} 个已应用或未完成操作；未选操作及之后被手动修改的文件不会变动。', restoreSelectedConfirm: '取消应用 {0} 个',
    restoreAllTitle: '恢复全部已应用的修改', restoreAllIntro: '恢复此报告中所有已应用或被中断的操作，并按依赖顺序处理。之后被手动修改的文件不会被覆盖，并会列出。', restoreAllConfirm: '恢复 {0} 个操作', restoreRunning: '已恢复 {0} / {1}…', restoreAllDone: '已恢复 {0} · 未恢复 {1}', restoreFailures: '未能恢复的文件', restoreStopped: '已停止。完成恢复的文件保持原始状态，其余操作可稍后继续恢复。',
    methodology: '分析口径', methodology1: 'Alpha 阈值 {0}；SSIMULACRA2 最低评分 {1}。推荐候选是通过全部校验的最小方案。评分用于辅助审核，不能替代目视检查。',
    methodology2: '动图不会被压成单帧。节省量指源文件字节数，不是编译后的 App 体积。', footerUnits: '体积采用二进制单位（1 KiB = 1,024 字节），悬停可查看精确字节数。', footerScope: '仅统计源文件收益 · 不等于 App 下载体积',
    dup_identical: '完全相同的文件', dup_resized: '同一张图片的不同尺寸', dup_similar: '近似重复的图片', dupRedundant: '{0} 个文件 · 除最大的一份外共 {1}',
    dupHint: '通过比较解码后的像素发现，与文件名无关。同一资源的倍率变体（@2x/@3x、密度目录）不会列出。删除副本需要同步修改引用它的代码，因此不会自动合并。',
    animInfo: '{0} × {1} · {2} fps · {3} 帧', animPlay: '播放', animPause: '暂停', animFrame: '第 {0} / {1} 帧', animStatic: '封面为第 {0} 帧。使用 resopt serve 打开此报告即可播放动画。',
    android: 'Android', androidInfo: '{0}/{1} · 名称“{2}” · 限定符 {3}', notes: '说明', perf: '分析耗时 {0} 秒（{1} 个线程，缓存命中 {2}，复用重复文件 {3}）',
    capabilityTitle: '本机可用能力', tool_missing: '未找到 {0} — {1} 安装方式：{2}',
    kind_image: '图片', kind_vector: '矢量图', kind_video: '视频', kind_audio: '音频', kind_animation: '动效', kind_font: '字体', kind_archive: '压缩包', kind_localization: '本地化', kind_data: '数据文件', kind_unclassified: '其他',
    status_candidates_available: '有更小候选', status_inspected: '没有更小的候选', status_excluded: '已排除', status_unsupported: '暂无优化器', status_inventory_only: '暂无优化器', status_failed: '分析失败', status_not_analyzed: '未分析',
    transparent: '含透明像素', opaque: '不透明', frames: '{0} 帧', mismatch: '扩展名与实际格式不一致',
    issue_android_nine_patch: 'Nine-patch：保持 PNG 以便 AAPT 读取拉伸标记，仅做无损优化', issue_android_launcher_icon: '启动图标：保持原格式，仅做无损优化', issue_android_raw_resource: 'res/raw 文件：应用按原始字节读取，保持原格式',
    issue_android_min_sdk_unknown: '无法从 Gradle 文件读取 minSdk；使用 --android-min-sdk 指定后可生成 WebP 候选', issue_app_icon: 'AppIcon：保持原样', issue_resizing: '拉伸（切片）图片：保持原样',
    issue_multiple_frames_not_transcoded: '动图：仅检测，不会压成单帧', issue_below_explicit_input_threshold: '低于指定的输入体积', issue_source_changed_during_analysis: '分析期间文件发生变化，请重新分析',
    issue_decoded_image_exceeds_max_pixels: '超过像素上限（可用 --max-pixels 调整）', issue_dimensions_changed: '尺寸改变', issue_orientation_changed: '方向改变', issue_no_smaller_candidate: '没有生成更小的候选', issue_failed_verification: '该候选未通过校验，无法应用',
    issue_ffprobe_not_installed: '安装 ffmpeg 后可查看此文件的编码、时长与码率', mediaInfo: '{0} · {1} 秒 · {2} kbit/s',
    issue_webp_candidates_disabled: '本次运行已关闭 WebP 候选（--no-webp）',
    issue_preview_unavailable: '无法渲染此动画的预览（{0}）；优化按字节校验，不受影响',
    issue_near_lossless: '近无损：编码器的最高质量。Apple 的 HEIC 编码器没有无损模式，仍有少量采样值发生变化，请参考实测误差',
    issue_palette: '颜色缩减为 {0} 色调色板（有损）；文件仍为 PNG',
    issue_backend: '已纳入清单；resopt 暂无此类型的优化器', issue_macos: '解码此格式需要 Apple ImageIO（macOS）', issue_min_sdk: '此处使用 WebP 需要 API {1}+，但 minSdk 为 {0}', issue_metadata: '不会带入新文件：{0}',
  },
};

// An explicit choice wins; otherwise the first supported language in the
// browser's preference order decides (so "en, zh" stays English).
function pickLocale(stored, browserLanguages) {
  if (stored === 'en' || stored === 'zh-CN') return stored;
  for (const language of browserLanguages || []) {
    const tag = String(language).toLowerCase();
    if (tag.startsWith('zh')) return 'zh-CN';
    if (tag.startsWith('en')) return 'en';
  }
  return 'en';
}

function translate(locale, key, args) {
  const text = (MESSAGES[locale] && MESSAGES[locale][key]) || MESSAGES.en[key] || key;
  return text.replace(/\{(\d+)\}/g, (_, i) => (args && args[i] !== undefined ? String(args[i]) : ''));
}

// Machine-readable issue identifiers become sentences; unknown ones are shown verbatim.
function issueText(locale, reason) {
  const text = String(reason);
  if (MESSAGES.en[`issue_${text}`]) return translate(locale, `issue_${text}`);
  if (MESSAGES.en[`warn_${text}`]) return translate(locale, `warn_${text}`);
  if (text.endsWith('_optimization_backend_not_implemented')) return translate(locale, 'issue_backend');
  if (text.endsWith('_decoding_requires_macos_imageio')) return translate(locale, 'issue_macos');
  const sdk = text.match(/^android_min_sdk_(\d+)_below_webp_requirement_(\d+)$/);
  if (sdk) return translate(locale, 'issue_min_sdk', [sdk[1], sdk[2]]);
  const palette = text.match(/^palette_colors: (\d+)$/);
  if (palette) return translate(locale, 'issue_palette', [palette[1]]);
  const preview = text.match(/^preview_unavailable: (.+)$/);
  if (preview) return translate(locale, 'issue_preview_unavailable', [preview[1]]);
  const metadata = text.match(/^metadata_not_carried_over: (.+)$/);
  if (metadata) return translate(locale, 'issue_metadata', [metadata[1]]);
  return text;
}
