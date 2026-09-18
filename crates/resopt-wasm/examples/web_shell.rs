use maud::{DOCTYPE, html};
fn main() {
    let native = std::env::args().any(|arg| arg == "--native");
    let page = html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8"; meta name="viewport" content="width=device-width,initial-scale=1";
                title { @if native { "resopt · 项目图片优化" } @else { "resopt Web · PNG 优化" } }
                link rel="stylesheet" href="../src/style.css";
                @if native { script type="module" src="../src/native.ts" {} } @else { script type="module" src="../src/app.ts" {} }
            }
            body {
                header {
                    a.brand href="./" aria-label="resopt 首页" { span.mark aria-hidden="true" {i{}i{}i{}i{}} strong {"resopt"} span.badge {"Web"} }
                    div.header-end { span.local { @if native { "图片仅在本机 macOS 处理" } @else { "图片仅在本机处理" } } a id="native-link" href="./native.html" hidden[!native] { @if native {"项目批量模式"} @else {"项目 / JPEG / HEIC"} } label.sr-only for="theme" {"页面主题"}
                        select id="theme" aria-label="页面主题" { option value="system" {"跟随系统"} option value="light" {"浅色"} option value="dark" {"深色"} }
                    }
                }
                main {
                    section.intro {
                        div { h1 { @if native {"项目图片优化"} @else {"PNG 无损优化"} } p { @if native {"拖入整个项目，先看整体收益，再查看需要关注的图片。"} @else {"减少文件体积，保留像素与透明细节。原文件不会被改写。"} } }
                        @if native { button.primary id="choose-project" {"选择项目目录"} input id="project" type="file" webkitdirectory multiple hidden; }
                        button id="choose" { @if native {"选择图片"} @else {"选择 PNG"} }
                        input id="files" type="file" accept=(if native {".png,.jpg,.jpeg,.heic,.heif"} else {"image/png,.png"}) multiple hidden;
                    }
                    section.stats aria-label="优化概览" {
                        div {span {"图片"} strong id="count" {"0"}}
                        div {span {"原始体积"} strong id="original-total" {"0 B"}}
                        div {span {"已节省"} strong.accent id="saved-total" {"0 B"}}
                        p.limit { @if native {"PNG / JPEG / HEIC · 每张 ≤16 MiB / 4 MP" br; "默认遵守 .gitignore · 原项目不改写"} @else {"静态 PNG · 每张 ≤16 MiB / 2 MP" br; "大图与 JPEG / HEIC 请使用原生 CLI"} }
                    }
                    section.toolbar aria-label="优化设置" {
                        @if native { label {"输出格式" select id="format" {option value="auto" {"最小候选（含有损）"} option value="png" {"PNG 无损"} option value="heic" {"HEIC（支持透明）"} option value="jpeg" {"JPEG（仅不透明）"}}} label {"编码质量" select id="quality" {option value="75" {"75"} option value="85" selected {"85"} option value="95" {"95"}}} } @else { label {"压缩耗时" select id="effort" {option value="1" {"快速"} option value="2" selected {"均衡"} option value="4" {"充分优化"}}}
                        label.check {input id="reductions" type="checkbox" checked; "无损颜色精简"} }
                        span.spacer {}
                        button id="clear" disabled {"清空"}
                        button id="download-all" disabled {"下载全部"}
                        button.primary id="start" disabled { @if native {"上传并分析"} @else {"开始优化"} }
                        button id="pause" hidden {"暂停"}
                    }
                    div.progress aria-hidden="true" { div id="progress" {} }
                    p.status id="status" role="status" aria-live="polite" {"正在准备图像引擎…"}
                    div.workspace id="drop-area" {
                        section.list-pane aria-label="图片列表" {
                            label.sr-only for="search" {"搜索图片"}
                            input id="search" type="search" placeholder="搜索文件名…";
                            div.list-head {span {"图片"} span {"原始体积"} span {"状态 / 节省"}}
                            div id="results" role="listbox" aria-label="图片列表" {}
                            div.empty id="empty" {
                                div.drop-symbol aria-hidden="true" {"↓"}
                                h2 { @if native {"拖入整个项目或资源目录"} @else {"拖入 PNG，开始比较"} }
                                p {"可批量选择图片，逐张检查优化结果。"}
                                button id="empty-choose" {"选择图片"}
                            }
                        }
                        aside id="inspector" aria-label="图片详情" {
                            div.empty {h2 {"原图与结果，一目了然"} p {"选择图片后可查看透明背景、像素验证和体积变化。"}}
                        }
                    }
                    footer { @if native {span {"图片只传给本机进程，不离开设备。项目扫描与引用迁移请使用 resopt web。"} a href="./" {"PNG 浏览器无损模式 →"}} @else {span {"支持严格无损压缩和无损颜色精简；已完成的图片可单独下载。"} span {"无需上传 · 单次最多 500 张"}} }
                }
                p { "整项目分析与 macOS JPEG / HEIC：在项目目录运行 " code { "resopt web ." } "，打开终端显示的本机地址。" }
                noscript {"请启用 JavaScript 以运行浏览器内的图片优化。"}
            }
        }
    };
    println!("{}", page.into_string());
}
