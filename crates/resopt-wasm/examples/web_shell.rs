use maud::{DOCTYPE, html};
fn main() {
    let page = html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8"; meta name="viewport" content="width=device-width,initial-scale=1";
                title { "resopt Web · PNG 优化" }
                link rel="stylesheet" href="../src/style.css";
                script type="module" src="../src/app.ts" {}
            }
            body {
                header {
                    a.brand href="./" aria-label="resopt 首页" { span.mark aria-hidden="true" {i{}i{}i{}i{}} strong {"resopt"} span.badge {"Web"} }
                    div.header-end { span.local {"图片仅在本机处理"} label.sr-only for="theme" {"页面主题"}
                        select id="theme" aria-label="页面主题" { option value="system" {"跟随系统"} option value="light" {"浅色"} option value="dark" {"深色"} }
                    }
                }
                main {
                    section.intro {
                        div { h1 {"PNG 无损优化"} p {"减少文件体积，保留像素与透明细节。原文件不会被改写。"} }
                        button.primary id="choose" {"选择 PNG"}
                        input id="files" type="file" accept="image/png,.png" multiple hidden;
                    }
                    section.stats aria-label="优化概览" {
                        div {span {"图片"} strong id="count" {"0"}}
                        div {span {"原始体积"} strong id="original-total" {"0 B"}}
                        div {span {"已节省"} strong.accent id="saved-total" {"0 B"}}
                        p.limit {"静态 PNG · 每张 ≤16 MiB / 2 MP" br; "大图与 JPEG / HEIC 请使用原生 CLI"}
                    }
                    section.toolbar aria-label="优化设置" {
                        label {"压缩耗时" select id="effort" {option value="1" {"快速"} option value="2" selected {"均衡"} option value="4" {"充分优化"}}}
                        label.check {input id="reductions" type="checkbox" checked; "无损颜色精简"}
                        span.spacer {}
                        button id="clear" disabled {"清空"}
                        button id="download-all" disabled {"下载全部"}
                        button.primary id="start" disabled {"开始优化"}
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
                                h2 {"拖入 PNG，开始比较"}
                                p {"可批量选择图片，逐张检查优化结果。"}
                                button id="empty-choose" {"选择图片"}
                            }
                        }
                        aside id="inspector" aria-label="图片详情" {
                            div.empty {h2 {"原图与结果，一目了然"} p {"选择图片后可查看透明背景、像素验证和体积变化。"}}
                        }
                    }
                    footer {span {"支持严格无损压缩和无损颜色精简；已完成的图片可单独下载。"} span {"无需上传 · 单次最多 500 张"}}
                }
                noscript {"请启用 JavaScript 以运行浏览器内的图片优化。"}
            }
        }
    };
    println!("{}", page.into_string());
}
