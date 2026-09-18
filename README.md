# resopt

面向 Apple 项目的资源检测与优化工具，提供 Rust 库和命令行接口。

`scan` 清点资源目录内外的资源，识别图片的实际编码；`analyze` 真正解码图片，检查透明像素，
试算不同格式与质量的候选，并生成带预览的报告。现有 `plan / apply / restore` 提供严格无损 PNG 的可恢复修改流程。
Numi 独立负责资源代码生成，`resopt` 不依赖它。

## GitHub 二进制与自动发布

推送与 `Cargo.toml` 版本一致的 `v<版本>` tag，会先执行三平台 CI，再编译并发布：

- macOS Apple Silicon：`aarch64-apple-darwin`（macOS 13+）。
- macOS Intel：`x86_64-apple-darwin`（macOS 13+）。
- Linux x86_64：`x86_64-unknown-linux-gnu`（Ubuntu 22.04 构建）。
- Windows x86_64：`x86_64-pc-windows-msvc`。

[GitHub Releases](https://github.com/wenext-limited/resopt/releases) 提供压缩包和 `SHA256SUMS`。
解压后将 `resopt`（Windows 为 `resopt.exe`）放到 PATH 即可；下载二进制无需安装 Rust。
JPEG／HEIC 编码仍仅支持 macOS；其它平台可清点资源并执行严格无损 PNG 流程。

发布前更新并提交 `Cargo.toml`、`Cargo.lock` 中的版本，确认 CI 通过，再创建并推送 tag：

```sh
# 示例：仅在 Cargo.toml 已更新为 0.3.0 时使用这个版本号
 git tag -a v0.3.0 -m 'resopt 0.3.0'
 git push origin v0.3.0
```

版本不匹配或任意测试／构建失败时不会发布。`v0.3.0-rc.1` 这类 tag 会标记为预发布。
工作流使用 GitHub 自动提供的 `GITHUB_TOKEN`；无需额外发布密钥，**不会自动发布 crates.io**。
重跑不会覆盖已存在的 GitHub Release；上传失败若留下草稿，应先检查草稿再重试。

## 浏览器版（WASM）

浏览器版以静态页面运行：选择 PNG → 本机优化 → 原图对比 → 下载图片或 ZIP 报告。
图片不上传到服务端，也不会覆盖原文件。支持透明 PNG、严格无损与无损颜色精简、
逐像素校验、SSIMULACRA2、暂停／继续、深浅主题，以及重复文件名的安全导出。

首版只处理手动选择的静态 PNG，每张限制 16 MiB / 2,097,152 像素，单批最多 500 张，
优化结果缓存上限 128 MiB。JPEG／HEIC、大图、项目扫描、Git 忽略规则和引用迁移仍使用原生 CLI。
网页不会把选中的图片当成一个完整 Xcode 项目，也不会自动更改项目引用。

### 本地开发

以下命令在 Git 仓库源码根目录执行，需要 Rust 1.89+、带 WebAssembly 后端的 Clang，以及 Bun 1.3.14。

```sh
rustup target add wasm32-unknown-unknown
rustup component add llvm-tools-preview
cargo install wasm-bindgen-cli --version 0.2.128 --locked
cd web
bun install --frozen-lockfile
bun run dev
```

默认打开 `http://127.0.0.1:8432/`；可用 `PORT=8433 bun run dev` 指定端口。
Bun 提供 HTML／TypeScript／CSS 热更新，图片优化在独立 Web Worker 中运行。
修改 Worker、Rust 或 Maud HTML 后，请重启 `bun run dev`，重新生成引擎和页面。

### 静态构建与验证

```sh
cd web
bun run build
bun run typecheck
bun test
bun run preview
```

`web/dist/` 可部署到任意静态 HTTP(S) 站点；其中的 Worker 与 WASM 文件须保持同源。
部署后无需 Bun 或业务后端，也无需跨源隔离头：当前 WASM 引擎为单线程，取消任务通过终止 Worker 实现。
不要用 `file://` 打开构建产物。开发／预览服务只监听本机，预览服务不接受上传或写入请求。
CI 会生成可下载的 `browser-site` 静态站点 artifact，不会自动公开部署网站。

### 代码组织

- `src/portable.rs`：原生与 WASM 共用的内存 PNG 优化和 sRGB RGBA 评分入口。
- 原生能力由默认启用的 `native` Cargo feature 控制；`--no-default-features` 不引入
  文件扫描、HTTP 服务、Git 子进程或 Apple 编码后端。
- `crates/resopt-wasm`：薄的 `wasm-bindgen` 接口，以及 Maud 页面生成示例。
- `web/`：Bun 构建、TypeScript 交互和 Worker；浏览器不运行 Bun。

构建脚本会使用 Rust `llvm-tools-preview` 中的 `llvm-ar`，避免 macOS 原生归档器漏掉
WASM 的 C 符号；也可通过 `AR_wasm32_unknown_unknown`、`WASM_BINDGEN` 和
`CARGO_TARGET_DIR` 指定已有工具。PNG 的 OxiPNG 后端在 WASM 上启用 `freestanding`，
关闭原生线程与 `std::time::Instant` 超时，原生 CLI 保留原有设置。

## macOS 网页服务与项目批量分析

`/native.html` 支持拖入整个项目或选择目录，递归收集 PNG / JPEG / HEIC，默认遵守各级
`.gitignore`。浏览器本地筛选，只在点击「上传并分析」后逐张上传图片，不上传源码。
支持最小候选（可能有损）、PNG 无损、JPEG、HEIC，以及 75 / 85 / 95 质量档位。
列表按实际收益排序，汇总全批次节省；可暂停在当前图片结束后继续，或下载保留目录结构的
候选 ZIP 和 `report.json`。重复目标文件名会编号，避免跨格式候选互相覆盖。

限制：最多读取 50,000 个文件、分析 5,000 张图片；每张 16 MiB / 4,194,304 像素；
结果缓存 128 MiB，超限暂停。忽略规则来自所选目录内的 `.gitignore`，不读取目录外的
全局 Git 配置或 `.git/info/exclude`，也不会读取 Git 索引来区分已跟踪文件。请选择项目根目录。
仅处理静态图片；JPEG 遇到透明像素会拒绝，HEIC 仍须通过 Alpha 误差校验。

**下载结果是候选，不会改写本地项目或迁移引用。** 跨格式应用仍需原生 CLI 的审核流程，
不要直接把 ZIP 覆盖到工程中。网页的 PNG 浏览器模式继续完全离线处理。

在 macOS 上构建并运行（Bun 原生进程，无需 Docker）：

```sh
cargo build --locked --release --bin resopt
cd web
bun install --frozen-lockfile
bun run build
HOST=127.0.0.1 PORT=8432 RESOPT_BIN="$(pwd)/../target/release/resopt" bun run host
```

需要内网访问时，把 `HOST` 改成服务器的内网 IP；`PUBLIC_ORIGIN` 可显式指定访问源
（例如 `http://10.86.10.42:8432`）。此服务面向可信内网，不带账户系统。
服务只提供站点文件及单图转换接口，不暴露已有项目目录或原生 `apply`。
转换通过独立临时目录调用 resopt，结束后删除临时文件；单次最多处理一张，超时 90 秒。
后台运行建议使用 macOS LaunchAgent 管理 Bun，并设置 `SITE_DIR`、`RESOPT_BIN` 的绝对路径。
原生集成验证：`RESOPT_TEST_BIN=/absolute/path/to/resopt bun test tests/native-integration.test.ts`。

## 安装

crates.io 包名为 **`resopt-cli`**，安装后的命令仍是 **`resopt`**：

```sh
cargo install resopt-cli --locked
resopt doctor
```

从源码安装时，在仓库根目录执行：

```sh
cargo install --path . --locked
resopt doctor
```

构建需要 Rust 1.89+ 和 C 编译器（用于 libdeflate）。严格无损 PNG 后端内嵌 Oxipng。
JPEG／HEIC 分析目前使用 **macOS ImageIO 与 CoreGraphics**，不需要额外安装 sips、FFmpeg 或 Swift 工具链。
其他平台可以清点资源，并使用原有的 PNG 计划与应用流程。

部分受限沙箱会阻止系统 HEIC 编码器。正式分析前会实际试编码 JPEG 和 HEIC；不可用时会明确报错，
不会把“没有成功编码”伪装成“没有优化空间”。`--probe-only` 可用于不编码的检测。

## 全资源扫描

```sh
resopt scan /path/to/project
resopt scan /path/to/project --json > inventory.json
```

默认遵守根目录、父目录和嵌套 `.gitignore`、`.git/info/exclude` 及 Git 全局忽略规则。
在 Git 可用且存在索引时，已跟踪的文件仍会纳入扫描（包括强制添加的文件）；普通目录也支持 `.gitignore`。
隐藏文件不会仅因名称以点开头就被过滤。

```sh
# 显式包含被 Git 忽略的资源；固定的 VCS／构建缓存排除仍然生效
resopt scan /path/to/project --include-ignored
resopt analyze /path/to/project --out /tmp/resopt-all --include-ignored
resopt plan /path/to/project --out /tmp/resopt-plan --include-ignored
```

扫描范围包括：

- `.xcassets` 引用的资源、目录元数据、未引用文件和暂不支持的资源节点中的文件。
- 目录外的图片、音视频、SVGA/VAP 等动效、字体、压缩包、数据文件、本地化文件和未分类文件。
- Pods、Carthage、node_modules 中的资源文件。

图片依据文件头识别 PNG、JPEG、HEIC/HEIF、WebP、GIF、TIFF、BMP、AVIF 等格式，
并报告扩展名与编码不一致的情况。例如，扩展名为 `.png` 的 WebP 会进入 WebP 分析路径。
无法从文件头识别的格式会先按扩展名分类，后续解码失败会单独报告。

**文件清点不等于构建目标分析。** 清单可能包含未被 App 打包的文件，压缩包内部也不会自动展开。
已知源码、构建配置和工具文件会排除；VCS、构建缓存、嵌套 worktree、工具配置目录不会遍历。
报告列出实际排除的目录以及源码／工具文件的排除数量。符号链接不跟随，不可读文件会产生诊断。

如需兼容旧版的“仅列出资源目录引用文件”结果：

```sh
resopt scan /path/to/project --catalog-only --json
```

## 图片分析：JPEG 与 HEIC

```sh
# 默认试算所有尺寸的图片，不再设置 50 KiB 门槛。
resopt analyze /path/to/project --out /tmp/resopt-analysis

# 显式指定质量档位与并发数。
resopt analyze /path/to/project --out /tmp/resopt-analysis-2 \
  --qualities 75,85,95 --jobs 2

# 只解码检查格式、尺寸、帧数和透明像素，不进行编码。
resopt analyze /path/to/project --out /tmp/resopt-probe --probe-only

# 要求候选 Alpha 与原图完全一致。
resopt analyze /path/to/project --out /tmp/resopt-exact-alpha --max-alpha-error 0

# 无损 PNG 候选使用更高的优化力度，并允许无损的颜色类型／位深／调色板缩减。
resopt analyze /path/to/project --out /tmp/resopt-reduced --png-level 4 --png-reductions

# 调整可分析的最大像素数（默认 16,777,216，上限 67,108,864）。
resopt analyze /path/to/project --out /tmp/resopt-large --max-pixels 33554432 --jobs 1
```

输出目录必须尚不存在，父目录必须存在，且必须位于待扫描项目之外。
默认 `min_input_bytes = 0`，因此小图片也会尝试；如有需要，可显式传入 `--min-input-bytes`。

### 格式选择

| 实际像素状态 | 比较的格式 |
| --- | --- |
| 没有透明像素，包括“带 Alpha 通道但 Alpha 全满” | JPEG、HEIC；原格式为 PNG 时另比较严格无损 PNG |
| 有透明或半透明像素 | HEIC；原格式为 PNG 时另比较严格无损 PNG；不生成 JPEG 候选 |
| 已经是 HEIC | 同样解码检查透明度，并按上述规则重新试算 |

默认 JPEG／HEIC 质量档位为 **75、85、95**。数值是编码器质量参数，既不是体积节省比例，
也不是可以跨格式直接比较的视觉质量分数。JPEG／HEIC 候选明确标为有损。

每个候选都会重新解码，验证尺寸、方向、帧数和透明状态，并计算：

- 感知画质分数 SSIMULACRA2：把预乘 Alpha 像素分别合成到黑、白、中灰背景上，在线性光下评分，
  取三者中的最低分（不透明图片只评一次）。100 表示完全一致，90 以上通常难以察觉，
  70–90 为轻微差异，50–70 为可察觉差异，低于 50 为明显劣化。超过约 200 万像素的图片按条带计算以限制内存，
  分数与整图计算相比可能有零点几分的偏差。
- 在统一 sRGB 预乘 Alpha 像素上的 RGB 平均绝对误差（MAE，按 0–255 标度显示）。
- 同一像素空间上的 PSNR；像素相同时显示无穷大。
- 单个像素最大的 Alpha 误差（0–1 标度）。

这些指标帮助筛选，不替代视觉审阅。完全透明像素的隐藏 RGB 不影响此处的有损画质比较。
HEIC 有损编码可能让 Alpha 相差一个 8 位量化级；默认上限为 `1/255 + 0.000001`，
允许这一级量化及浮点计算误差，同时拒绝整体透明状态的变化。
`--max-alpha-error 0` 要求 Alpha 精确一致；该设置不会改变原有严格无损 PNG 的像素保留约束。

### 报告与候选文件

输出目录包含：

- `analysis.json`：全部资源、检测状态、问题、每个格式／质量的实际体积和误差。
- `report.html`：可离线查看的中文报告，含原图与候选缩略图；点击可打开原尺寸文件。
  报告支持搜索、原格式筛选、排序和分页，采用 KiB／MiB 自动单位，并可切换方案与透明背景。
- `originals/`、`candidates/`、`previews/`：有体积收益的有效候选及对应原图和预览。

较大、超出 Alpha 上限或编码失败的候选仍在 JSON／HTML 中记录原因，但不会被推荐为可采用结果。
`smallest_candidate` 只表示通过结构与 Alpha 检查后体积最小的候选，**不表示画质已经验收**。
总节省量按每个文件仅取一个最小候选计算；有损候选可能采用不同质量档位。

**分析不会修改项目。** 报告不是 `apply` 可执行的计划。
传统 `plan/apply/restore` 命令仍面向严格无损 PNG。分析报告的逐张应用与跨格式替换通过下述 `serve` 流程执行。

### 刷新已有报告界面

```sh
resopt report /tmp/resopt-analysis
```

读取同目录的 `analysis.json` 并更新 `report.html`，不重新编码，不改动测量数据或候选文件。
HTML 内嵌交互所需的数据与脚本，无需网络或额外前端构建步骤。

### 在报告页面优化图片

```sh
resopt serve /tmp/resopt-analysis
# 可选固定端口，默认自动分配空闲端口
resopt serve /tmp/resopt-analysis --port 8417
```

打开终端输出的 `http://127.0.0.1:<端口>/` 地址，选择图片与候选，点击「优化这张图片」，
核对格式、质量和节省体积后确认。JPEG／HEIC 需要明确确认有损优化；页面也提供「恢复原图」。
**直接打开静态 HTML 仍只供审阅**，写入由本地 Rust 服务完成。按 Ctrl-C 停止服务。

- 支持无损 PNG、JPEG／HEIC 候选。Asset Catalog 跨格式替换会更新所有匹配的
  `Contents.json` rendition 文件名，保留其它字段；恢复时还原原始 JSON 字节。
- 散落图片也支持 PNG／JPEG／HEIC 之间的有效候选转换。点击优化会先预览新文件名和引用文件清单，
  确认后将新图片、文件引用和旧图移除一并记录到可恢复事务；预览后文件若有变化，会拒绝应用并要求重新审阅。
- 自动迁移可解析的静态引用：完整文件名、项目路径／相对路径、常见 Swift `UIImage(named:)`、
  `Image(...)`、`Bundle.url(forResource:withExtension:)`，以及 Xcode `PBXFileReference` 的路径和文件类型；
  还支持 JSON／HTML 等文本中的带引号路径、CSS `url(...)`、Markdown 图片链接、XML plist 字符串、
  Interface Builder 图片属性和简单 YAML 路径值。忽略规则同样用于引用扫描。
- 重名或多倍率图片导致解析歧义时不会猜测；需要将引用明确化，或成组处理倍率资源。
  动态拼接、项目外引用、二进制 plist、编码后的路径及第三方解码器兼容性仍需人工复核。
  这是可审阅的静态引用迁移，不是对运行时所有引用的证明。
- AppIcon、拉伸图片和多帧图片保留现有限制。候选会再次解码或严格验证 PNG，检查源文件哈希、尺寸、方向和 Alpha。
- 服务启动时固定候选哈希，拒绝运行期间被替换的候选；服务重新启动后会重新验证所选候选。
- 原始字节、候选和操作记录保存在报告目录的 `operations/` 中，**需要恢复时请保留整个报告目录**。
  操作中断后可重启服务恢复；对共享引用文件（包括 `Contents.json`）的多次转换须按后做先恢复的顺序操作。
  恢复拒绝覆盖后续人工修改。每个文件原子替换，跨文件操作通过持久记录恢复，并非单个原子事务。
- 服务只监听 `127.0.0.1`，写入接口检查 Host、Origin 和随机会话令牌，不接受客户端传入的文件路径。
- 概览体积与图片对比保留分析时的快照；已应用状态单独展示，需要新的整体统计时重新运行 `analyze`。

页面支持「跟随系统／浅色／深色」主题并记住选择，图片的棋盘／白色／深色预览背景独立设置。
页面结构由 Maud 在 Rust 中生成，CSS 和浏览器交互脚本编译进 CLI；不需要 WebAssembly 或前端构建工具。

### 分析边界

- 每张图片输入上限为 64 MiB；解码像素数默认上限为 16,777,216（4096×4096），可用 `--max-pixels` 调整，
  最高 67,108,864。超限会以 `decoded_image_exceeds_max_pixels` 明确报告。
  分析大图时每个并发任务的峰值内存约为每像素 150 字节（实测 2048×2732 约 0.9 GiB，另加 Oxipng 的开销），
  内存紧张时请降低 `--jobs` 或 `--max-pixels`。
- 多帧图片记录帧数和首帧检测信息，但不会转成单帧 JPEG／HEIC。
- AppIcon、已识别的 `resizing` 资源会解码检查，但不会生成格式转换候选。
- 音视频、动效、字体、压缩包、矢量图、数据文件等纳入清单，状态为 `inventory_only`，
  原因注明对应优化后端尚未实现；不会重新编码这些类型或修改容器内部内容。
- JPEG／HEIC 检查不承诺原始元数据的字节级保留；严格无损 PNG 则保留非 IDAT 块及其顺序。
- 有错误、跳过项或不支持的类型时，报告会保留它们。命令成功不代表每个文件都已优化。

## 严格无损 PNG：计划、应用和恢复

```sh
resopt plan /path/to/project --out /tmp/resopt-review
resopt apply /tmp/resopt-review
resopt restore /tmp/resopt-review
```

这个既有流程仍只处理资源目录引用的静态 PNG，文件名和资源名不变。
规划时优化发生在内存中，只写入新的计划目录；每个候选经过独立 PNG 解码，
逐字节比较像素数据，并检查全部非 IDAT 块及其相对于首个 IDAT 的顺序。
完全透明像素的隐藏 RGB 也保留，位深、颜色类型、调色板和隔行扫描设置不变。

默认策略与 `resopt.example.toml` 一致：

```toml
png_level = 2
min_input_bytes = 51200
min_savings_bytes = 1024
min_savings_percent = 1.0
```

```sh
resopt plan /path/to/project --policy resopt.example.toml --out /tmp/resopt-review-2
```

`png_level` 是优化力度而非视觉质量；文件必须变小并同时达到两个收益门槛。

可选的 `reductions = true` 允许无损的位深、颜色类型、灰度和调色板缩减。这类缩减会改写 IHDR／PLTE／tRNS，
因此校验方式改为：其余所有块及顺序必须逐字节一致，尺寸与隔行设置不变，
并把两个文件逐行展开为 RGBA16 后比较每个样本——完全透明像素的隐藏 RGB 同样必须一致。
缩减结果无法通过校验或并不更小时，自动退回严格模式的候选。默认关闭；
关闭时计划文件与旧版本完全兼容，开启后的计划会被旧版本 resopt 拒绝。
只有传入 `--policy` 才加载配置；未知字段会报错。这个策略与 `analyze --qualities` 的有损试算相互独立。

应用前会检查整个批次的源文件、资源目录和候选哈希，逐文件写入前还会再次核对。
每个替换使用同目录临时文件与原子重命名，但整个批次不是单个原子事务。
原件预先保存，`journal.jsonl` 记录操作；保留整个计划目录即可在部分完成后恢复。
重复应用或恢复不会重复修改已符合目标的文件，后续用户编辑和过期目录元数据会阻止覆盖。
普通文件权限会保留，时间戳、扩展属性和硬链接关系不会保留。

操作期间不要并发编辑同一资源树。`.lock` 与项目根目录的 `.resopt.lock` 用于避免并发写入；
异常退出后，确认进程不再运行再清理遗留锁。移动项目后需要重新生成计划。

## 与 Numi 配合

在目标项目根目录按需运行：

```sh
resopt apply /tmp/resopt-review && numi generate --workspace
```

当前无损 PNG 后端保留资源名，因此生成的访问代码通常不变；不会隐式执行 Numi。

## Rust API

通过 `cargo add resopt-cli` 添加依赖，Rust 库名仍为 `resopt`。

```rust,no_run
use resopt::{AnalysisOptions, Policy, analyze, create_plan, inventory};

let resources = inventory("/path/to/project")?;
let analysis = analyze("/path/to/project", "/tmp/resopt-analysis", AnalysisOptions::default())?;
let lossless_plan = create_plan("/path/to/project", "/tmp/resopt-review", Policy::default())?;
# Ok::<(), anyhow::Error>(())
```

原有 `scan()` Rust API 仍返回旧版资源目录引用清单；全资源入口为 `inventory()`。
命令均支持 `--json`，标准输出用于报告，标准错误用于进度与错误。
成功返回 0，操作失败返回 1，用法错误返回 2；消费报告时请检查状态与诊断字段。

## 验证与体积口径

```sh
node --test tests/report-ui.cjs
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --doc
```

报告脚本的语法、单位格式与资源 URL 检查使用 Node.js 内置测试工具（仅开发验证需要 Node.js）。
macOS 图片编码测试需要能够访问系统 HEIC 编码器；不应在阻止编码服务的沙箱中运行。
原有跨平台 PNG 测试继续保留；新增测试覆盖实际透明像素、透明 HEIC、已有 HEIC、错误扩展名、
资源目录外文件、小图片、损坏图片、候选路由及报告生成。

**所有收益均为源文件字节数。** 不证明 `Assets.car`、IPA、App Store 下载体积、解码速度或内存用量会改善。
资源是否进入目标 App，以及编译后的体积差异，需要单独验证。

## 许可证

MIT。各依赖保留其许可证。未引入 Imagequant 或 GPL 依赖。
