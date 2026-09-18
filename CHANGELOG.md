# 更新记录

## Unreleased

- Use xcassets 0.3 rendition indexing and filename-editing APIs; remove duplicate catalog traversal and metadata-editing logic.

- Show completed savings while project analysis is still running; reuse quality-scoring reference data and byte-identical image results.
- Add opt-in native WebP candidates with `--webp`, including transparency checks and reversible same-format updates.
- Keep Alpha-warning candidates for review, download, and explicitly approved application; retain source-integrity and structural checks.
- Protect Android Nine-patch/mipmap resources and block unsupported Android cross-format application.
- Rewrite the README in English for developers using the tool; move contributor details to the development guide.

## 0.5.0 · 2026-09-18

- 新增 `resopt web <项目目录>`：本地扫描、浏览器进度与审核页面，无需上传或安装 Bun；macOS 启用 JPEG/HEIC，Linux/Windows 支持 PNG 无损分析。
- 在线站点切换为纯 WASM；原生 Bun 调试服务强制 loopback，拒绝远程编码配置。

- 源码中的 macOS Bun 调试页面支持目录拖拽、嵌套 `.gitignore` 和候选 ZIP 导出，仅允许本机访问。正式项目入口为 `resopt web`，在线站点不接受上传。

- 新增浏览器版：复用 Rust 核心，通过 WASM 在本机无损优化静态 PNG、校验像素并计算 SSIMULACRA2；支持明暗主题、暂停恢复及 ZIP 下载。
- 使用 Bun + TypeScript + Web Worker 构建网页，Maud 生成 HTML；新增独立浏览器构建与 WASM 回归测试 CI。

- `analyze` 为每个候选计算 SSIMULACRA2 感知画质分数（黑、白、灰背景下的最低分），写入
  `difference.ssimulacra2` 并显示在 HTML 报告中；旧的 `analysis.json` 仍可读取。
- 解码像素上限由约 419 万提高到默认 16,777,216，并新增 `--max-pixels`（最高 67,108,864）；
  超限原因改为 `decoded_image_exceeds_max_pixels`。大图的感知评分按条带计算以限制内存。
- 新增可选的无损 PNG 缩减：策略字段 `reductions` 与 `analyze --png-reductions`。
  校验逐行比较展开后的 RGBA16 样本，其余块必须逐字节一致；不更小或无法校验时退回严格候选。
- `analyze` 的无损 PNG 候选不再固定使用默认策略，新增 `--png-level`。
- 最低 Rust 版本提高到 1.89（`fast-ssim2` 的要求）。

## 0.2.0

- crates.io 包名为 `resopt-cli`，命令名和 Rust 库名保留 `resopt`。

- 扩展 `scan` 到资源目录内外，按文件头识别图片实际编码；`--catalog-only` 保留旧版输出。
- 新增 `analyze`，通过 macOS ImageIO 解码 PNG、HEIC、JPEG、WebP 等图片，检测实际透明像素。
- 不透明图片比较 JPEG 与 HEIC；透明图片比较 HEIC，PNG 另提供严格无损候选。
- 默认按 75、85、95 三档试算，生成 JSON、中文 HTML、原图与候选预览，报告 RGB/Alpha 误差。
- 默认纳入小图片；多帧文件不转成单帧，AppIcon 和已识别的拉伸资源只检测。
- 保留原有严格无损 PNG 的 `plan/apply/restore`；JPEG/HEIC 跨格式替换尚未接入自动应用。

验证：30 项本地测试、文档示例、Clippy、打包后编译通过；Rust 1.88 的 `cargo check --all-targets` 通过。
