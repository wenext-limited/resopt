# resopt

面向 Apple 项目的资源优化工具，同时提供 Rust 库和命令行接口，支持先审阅、再应用。

`resopt` 从 `.xcassets` 中发现被引用的资源文件，生成优化候选，验证解码后的像素和元数据，
然后按审阅过的结果替换文件，并保留可恢复的原件。资源目录由 `xcassets` 解析，
PNG 优化使用内嵌的 Oxipng。Numi 继续独立负责资源代码生成。

## 首版功能

- 递归发现资源目录，包括 Swift 包内的 `.xcassets`。
- 对静态 PNG 重新压缩，保留文件名、解码后的像素数据、位深、颜色类型、隔行扫描设置、
  调色板和所有非 IDAT 数据块。
- 提供便于人工阅读的报告和 JSON 输出。
- 通过 TOML 策略设置优化力度和最低体积收益。
- 保存优化计划，包含源文件与资源目录元数据的哈希、候选文件和原件。
- 应用与恢复前检查整个批次，逐文件原子替换，并提供锁和操作日志。

**报告中的节省量是源文件字节数。** 当前版本不测量编译后的 `Assets.car`、IPA、
App Store 下载体积、解码速度或内存占用。Xcode 可能重新压缩资源目录中的输入文件，
因此源文件缩小不一定意味着最终 App 同比例缩小。

## 从源码安装

在本仓库根目录执行：

```sh
cargo install --path . --locked
resopt doctor
```

构建需要 Rust 1.88+ 和 C 编译器，后者用于编译内嵌的 libdeflate 依赖。
当前 PNG 工作流不要求另外安装 Oxipng、sips 或 FFmpeg 可执行文件。
`doctor` 会检查未来后端可能使用的媒体工具，但不会安装它们，也不会调用它们进行优化。

## 使用流程

```sh
# 只读扫描，不编码，也不修改项目。
resopt scan /path/to/project
resopt scan /path/to/project --json > inventory.json

# 生成候选文件；输出目录必须尚不存在，其父目录必须已经存在。
resopt plan /path/to/project --out /tmp/resopt-review

# 查看按收益排序的报告，以及 plan.json、originals/ 和 candidates/。
# 执行 apply 表示明确同意写入已经审阅的修改。
resopt apply /tmp/resopt-review

# 对已经与候选结果一致的文件，重复 apply 不会再次修改。
# 如需保留恢复能力，请保留整个审阅目录。
resopt restore /tmp/resopt-review
```

所有命令都支持 `--json`。报告写入标准输出，面向人工阅读的诊断和错误写入标准错误。
JSON 格式的扫描与计划报告会把诊断信息放在文档中。
命令成功返回 `0`，操作失败返回 `1`，命令行用法错误返回 `2`。
扫描或生成计划成功时，仍可能有文件被跳过：请查看 `diagnostics` 和 `skipped`，
不要把命令成功理解为所有资源都已处理。

### 优化策略

```sh
resopt plan /path/to/project --policy resopt.example.toml --out /tmp/resopt-review
```

```toml
png_level = 2
min_input_bytes = 51200
min_savings_bytes = 1024
min_savings_percent = 1.0
```

默认值与上例一致。仅在传入 `--policy` 时加载指定的策略文件。
`png_level` 控制优化力度，不代表视觉质量。候选文件必须比原件小，
并且同时达到最低节省字节数与最低节省百分比。
未知配置字段会报错，因此类似 `quality = 75` 的有损压缩设置不会被静默忽略或错误应用。
当前版本尚未实现有损压缩。

### 与 Numi 组合使用

如果工作流需要重新生成资源访问代码，可在项目根目录执行：

```sh
resopt apply /tmp/resopt-review && numi generate --workspace
```

当前 PNG 后端保留文件名和资源名，因此重新生成的访问代码通常不会变化。
`resopt` 不依赖 Numi，也不会隐式运行代码生成器。

## 安全约束

生成计划时，优化过程在内存中进行，只向新建的计划目录写入文件。
每个入选候选都必须通过独立的 PNG 解码，并逐字节比较解码后的像素数据。
所有非 IDAT 数据块及其相对于首个 IDAT 的顺序必须一致。
这会主动拒绝一些会重写元数据的有效优化。
完全透明像素的 RGB 值也会保留；位深、调色板和颜色类型的缩减均已禁用。

应用前会先检查所有条目，核对当前资源是否仍符合处理条件、`Contents.json` 哈希、
源文件哈希、候选与原件哈希、文件大小、元数据和解码后的像素数据。
每个文件在写入前还会再次检查。
当前源文件可以与原件或计划中的候选结果一致，从而支持重复应用和部分完成后的恢复。
恢复操作遵循同样的规则；如果用户后来修改了文件，或资源目录元数据已经变化，就会拒绝覆盖。

每次替换都在源文件同目录创建临时文件，再通过原子重命名完成，并保留普通文件权限。
**整个多文件批次并非一个原子事务。** 原件会在应用前保存，
`journal.jsonl` 会记录每次替换的开始和完成。
如果 I/O 错误中断了批次，请保留审阅目录并执行 `restore`。
即使最后一条日志尚未写入，恢复操作也能通过哈希识别已经完成的部分。
文件系统时间戳、扩展属性和硬链接关系不会保留。

应用与恢复时会临时创建计划目录下的 `.lock` 和项目根目录下的 `.resopt.lock`。
进程异常终止后，应先确认它已不再运行，再删除遗留锁文件。
操作期间不要并发编辑源文件、修改符号链接，或对根目录存在交集的项目同时执行优化。
该工具面向本地开发工作流，不提供抵御恶意并发文件系统修改的安全沙箱。
计划记录项目根目录的绝对路径；项目移动后需要重新生成计划。

## 扫描范围与排除规则

- 仅统计资源目录明确引用的资源变体文件。尚不扫描目录外的独立资源文件，
  也不根据磁盘发现结果推断资源属于哪个构建目标。
- 跳过 `.git`、`.worktrees`、`.worktree`、`.build`、`.swiftpm`、`.resopt`、
  `target`、`build`、`DerivedData`、`Pods`、`Carthage` 和 `node_modules` 目录。
- AppIcon、带有 `resizing` 元数据的资源及非 PNG 资源会列入报告，但不参与优化。
- APNG、格式错误的 PNG 和验证失败的候选会被跳过，并记录原因。
  生成计划时，单张 PNG 的输入上限为 64 MiB，解码数据上限为 256 MiB。
- 包含符号链接或不可读条目的资源目录会被跳过。
  引用文件名中的路径穿越、候选与原件路径中的符号链接，以及缺失文件都会被拒绝。
- 不支持的资源目录节点类型会产生诊断信息。
  当前不删除未使用资源、不改写源码、不推断最低系统版本，也不转换文件扩展名。

## 作为 Rust 库使用

```rust,no_run
use resopt::{Policy, create_plan, apply, restore};

let plan = create_plan("/path/to/project", "/tmp/resopt-review", Policy::default())?;
println!("{} 个候选，可节省 {} 字节源文件", plan.candidates.len(), plan.savings_bytes());
// 审阅已保存的计划后再应用：
let report = apply("/tmp/resopt-review")?;
restore("/tmp/resopt-review")?;
# Ok::<(), anyhow::Error>(())
```

计划格式带有版本号（`schema_version = 1`）。`read_plan` 验证其结构；
应用与恢复时还会验证保存的文件和当前项目状态。
请把整个计划目录作为一个整体保存，不要只复制 `plan.json`。

## 开发与验证

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --doc
```

测试覆盖实际编码与解码往返、16 位像素数据、透明像素中的 RGB 值、元数据、跳过规则、
Unicode 路径、过期计划、损坏的候选，以及内容不安全但哈希已被重新计算的候选。
还覆盖符号链接、锁、部分应用、恢复和 CLI JSON 输出。
CI 已配置 Linux、macOS 和 Windows；本地验证通过不代表远端 CI 已通过。

## 后续支持

后续计划支持 JPEG 优化、经批准的有损图片候选、HEIC 转换、普通音视频优化，
以及能够识别构建目标的资源发现。
SVGA、VAP 等特殊动效容器需要各自明确的格式约束。
编译后资源目录的体积测量应作为独立、按需启用的验证阶段。

## 许可证

MIT。各依赖保留自己的许可证。首版未引入 Imagequant 或 GPL 依赖。
