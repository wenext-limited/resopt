# 更新记录

## 0.2.0

- crates.io 包名为 `resopt-cli`，命令名和 Rust 库名保留 `resopt`。

- 扩展 `scan` 到资源目录内外，按文件头识别图片实际编码；`--catalog-only` 保留旧版输出。
- 新增 `analyze`，通过 macOS ImageIO 解码 PNG、HEIC、JPEG、WebP 等图片，检测实际透明像素。
- 不透明图片比较 JPEG 与 HEIC；透明图片比较 HEIC，PNG 另提供严格无损候选。
- 默认按 75、85、95 三档试算，生成 JSON、中文 HTML、原图与候选预览，报告 RGB/Alpha 误差。
- 默认纳入小图片；多帧文件不转成单帧，AppIcon 和已识别的拉伸资源只检测。
- 保留原有严格无损 PNG 的 `plan/apply/restore`；JPEG/HEIC 跨格式替换尚未接入自动应用。

验证：30 项本地测试、文档示例、Clippy、打包后编译通过；Rust 1.88 的 `cargo check --all-targets` 通过。
