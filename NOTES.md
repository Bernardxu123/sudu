# 素读工程笔记

本文件记录不能从代码直接推导的取舍与外部约束；源码保持零注释。

## iced 0.14

- 应用入口：`iced::application(|| (State, Task), update, view).title(...).theme(...).subscription(...).window(...).default_font(...).run()`。参考 rmdv `src/main.rs`。
- `widget::Id` 只能标识滚动容器自身；要跳到某个块，需要 `Operation` 读到该块容器的 `layout.bounds()`。
- `Operation<T>: Send`，必须实现 `traverse`（调用 `operate(self)`），否则遍历不进入子节点。
- 容器 bounds 与 Scrollable 的 `content_bounds` 在同一坐标系；`bounds.y - content_bounds.y` 是正文内绝对 y。参考 rmdv `src/app/scroll_tasks.rs` 的 `ScrollBlockToTop`。
- 程序化滚动：`iced::widget::operation::scroll_to(id, scrollable::AbsoluteOffset { x, y })`、`snap_to`、`scroll_by`。
- `Padding` 只有 `From<u16>`、`From<[u16;2]>`、`From<f32>`、`From<[f32;2]>`；四边不同时用结构体字面量。
- `container::Style` 字段：`text_color`、`background`、`border`、`shadow`、`snap`。
- `Lazy` 的公开入口是 `iced::widget::lazy(dependency, view)`（模块私有，helpers 再导出）；`Dependency: Hash`，`view: Fn(&Dependency) -> Element<'static, ...>`。
- `Lazy` 会把 `operate` 转发到缓存子树，因此块 Id 在惰性正文里仍可被 Operation 找到。
- Scrollable 的 widget state 按树位置索引；包裹/解开正文（查找条、侧栏切换）会把滚动位置重置到顶部。rmdv 在 `src/app/mod.rs` 的 `restore_body_scroll` 里记录了这一点。

## iced_selection 0.5

- crates.io 0.5.0 的 `text::Style` 只有 `color` 与 `selection`；没有 `link_hover`。
- crates.io 0.5.0 的选择状态不公开，无法从外部清除；跨块选择需要 rmdv 的 vendored 补丁（`vendor/iced_selection/VENDORED.md`，MIT）。阶段②先测 crates.io 版本的多选区行为，必要时改用 vendored path 依赖。
- 中文输入法下 Ctrl+C 可能报告非拉丁 `key`；rmdv 在 `src/selectable.rs` 的 `LatinKeys` 里把带 command 修饰的按键事件改写为拉丁键名，然后再交给选择组件。此问题在中文 Windows 上必须复测。

## 字体

- 不捆绑字体。启动只检查 `%WINDIR%\Fonts` 与 `%LOCALAPPDATA%\Microsoft\Windows\Fonts` 的候选文件是否存在，再映射到英文家族名。
- 正文与 UI 统一无衬线：`Microsoft YaHei UI` → `Segoe UI`；代码：`Consolas` → `Cascadia Mono`。用户 2026-11-19 明确喜欢无衬线，纸/米黄的思源宋体/SimSun 衬线链已移除。
- cosmic-text/fontdb 由 iced 内部加载系统字体；我们不再引入 fontdb，避免重复扫描（阶段②实测启动时间）。
- 若家族名不命中，cosmic-text 会静默回退；必须用截图验证，而不是只看代码。

## 构建

- `iced` 关闭默认特性后需要显式 `web-colors`，否则半透明叠加与抗锯齿边缘偏色。
- 默认后端 `backend-tiny`（tiny-skia）；`backend-wgpu` 用于对比。
- 图片编解码瘦身杠杆：`iced/image-without-codecs` + 显式 `image`（png/jpeg/webp/gif/bmp），先用 `cargo tree -i image` 确认版本统一。

## 探测

- `SUDU_PROBE=<文件路径>`：仅在设置该环境变量时订阅窗口事件；`RedrawRequested`、`Opened`、`Focused` 任一先到就写 `startup_ms` 并停用。tiny-skia 下 `RedrawRequested` 不一定出现，`Opened` 是可靠回退。
- `SUDU_TIMING=<文件路径>`：正文元素构建耗时。
- `SUDU_THEME=0..3`：启动主题。`SUDU_NO_OPEN=1`：链接点击不打开系统浏览器（测试用）。

## 中文 Markdown 脏数据

- Typora/Obsidian 会把空格与中文编码进路径：`assets/%E6%9E%B6%E6%9E%84%20%E5%9B%BE.png`；读取本地图片和站内链接前必须 percent-decode。
- 相对路径基于当前 md 所在目录，不是进程工作目录。
- UTF-8 BOM 出现在不少 Windows 工具保存的文件里；读取后 strip。
- YAML frontmatter 用 `Options::ENABLE_YAML_STYLE_METADATA_BLOCKS` 解析后跳过，避免渲染成横线加标题。

## 工具链（2026-11-19 实测）

- MSVC 路线未走完：VS Build Tools 从 Microsoft CDN 下载约 0.25 MB/s，完整 C++ 工作负载预计数小时，已中止。改用 WinLibs MinGW-w64（UCRT，GCC 16.2）+ `stable-x86_64-pc-windows-gnu`；`rust-toolchain.toml` 固定 GNU。
- GNU ld 在 Windows 上无法处理含中文的路径：项目在 `D:\AI\杂\素读`，链接 proc-macro 时报 “cannot find ...rlib”。解决：`.cargo/config.toml` 把 `target-dir` 指到 ASCII 路径 `D:/AI/sudu-target`，并显式指定 WinLibs 的 `gcc.exe` 作为 linker。
- 若以后装好 MSVC，把 `rust-toolchain.toml` 改回 msvc 即可；代码与 target 无关，`target-dir` 配置可保留。

## 阶段② Gate 数据（release，tiny-skia vs wgpu）

- tiny-skia：exe 5.86 MB；37 块样文启动中位 84 ms（窗口句柄 45 ms）、WorkingSet 26.7 MB；1 MB / 3763 段落：首个事件 437 ms、WorkingSet 232.8 MB。
- wgpu：exe 10.23 MB；样文首个事件约 925 ms、WorkingSet 182.8 MB（首次运行更高）。当时按启动/内存优先选 tiny-skia；后续对比 Moeka 后改成流畅度优先，默认后端已切到 wgpu（见下文「滚动与流畅度」）。
- 选区/复制：中英混排在 crates.io `iced_selection 0.5.0` 上通过；Ctrl+C 复制文本正确；选中第二段后第一段高亮立即清除，不需要 vendored patch。
- 链接：span 的 `link` + `on_link_click` 生效；单击前光标必须先移动到链接上（产生 hover），正常使用无感。
- 坐标：`CollectOffsets` 在懒渲染正文里收到 37 个块；`scroll_to` 跳转到第 4 块成功；跳转/滚动后 `renders` 保持 1，Lazy 未重建。
- 主题：四套都能渲染；`纸/米黄` 当时命中已安装的 `NotoSerifSC-VF.ttf`（家族名 `Noto Serif SC`），`白/夜` 命中 `Microsoft YaHei UI`；代码 `Consolas`。后续按用户偏好已全部改为无衬线（见上文）。
- 已知问题：1 MB 文档 WorkingSet 233 MB（tiny-skia），需要后续 virt 分块；正文宽度已改为按窗口与侧栏计算后居中。
- 表格：pulldown-cmark 的表头是 `TableHead` + 直接 `TableCell`，没有 `TableRow`；必须自己把表头开成 `TableRow { header: true }`，否则表头会被当成普通段落。
- 阴影：iced 0.14 tiny-skia 在 `stack` 覆盖层里给 container 设置 `Shadow` 会逐帧累积 alpha，菜单/查找框会慢慢变黑。已移除所有容器阴影，只保留 1px 边框；证据 `no-shadow-1.png` / `no-shadow-2.png`（像素 245,246,248 不变）。

## 滚动与流畅度（2026-11-19 两次修正）

- 第一次：iced 0.14 Scrollable 对滚轮 `Lines` 直接 `-movement * 60.0`，每格 60 px 无动画；加了 16 ms 缓出，但对比 Moeka 仍“不跟手”。
- Moeka（GPUI）的做法：`Cargo.toml` 用 `gpui 0.2.2`，README 明确 GPU / 120Hz / on-demand rendering；`src/editor/input.rs:4553` 的 `on_scroll` 直接 `scroll_offset -= delta_y`（`pixel_delta(line_height)`），没有动画；`src/editor/list_viewport.rs` 只渲染 visible/render/preload 窗口并在上下留 spacer。
- 素读对应改成：`SmoothScroll` 仍拦截 `WheelScrolled`，但立即 `scroll_to`；`Lines` 步长 = 正文字号 × 行高（约 31 px），`Pixels` 1:1；删除 16 ms 动画订阅和 `AnimTick`。
- 后端：默认 `backend-wgpu`（GPU，接近 Moeka 的路线）；`backend-tiny` 保留为轻量构建。
- 实测：wgpu 11.57 MB / 首事件 968 ms / WorkingSet 184 MB / 快速滚轮 CPU 0.03 核；tiny-skia 6.93 MB / 137 ms / 26.8 MB / 0.56 核。两条 exe 都在 `dist\`。
- 图标：`assets/icons` 使用 iconly 的 `Search_528.svg`（Light-Outline）与 `Setting_530.svg`，侧栏/编辑/保存/最小化/最大化/还原/关闭为同风格线性 SVG；iced 开启 `svg` feature，用 `svg::Style { color }` 按主题着色。

## 渲染态原位编辑（2026-11-20）

- 目标：点铅笔/Ctrl+E 后页面保持渲染，不整页切源码；点击哪一块，只把这一块原位变成编辑区，字体/字号/行高/宽度与阅读视图一致。
- 源码范围：`Block.span` 记录可编辑范围。标题去掉 `# `、列表去掉 `- `/`1. `、代码块去掉围栏，引用/段落保留内容；保存只替换这段，标记与前缀不动。`doc.rs` 的 `block_span` 负责裁剪。
- 显示转换：`live.rs` 用 pulldown-cmark 的 offset 事件收集 Strong/Emphasis/Strikethrough/Link/Image/Code 的标记范围，`conceal` 生成“去掉标记的显示文本 + display→source offset map”；粗体/斜体/链接/行内代码用自定义 `MarkHighlighter` 着色。表格行只隐藏首尾管道，内部管道保留，避免在行尾输入把表格写坏。
- 编辑同步：`text_editor::Content` 持有显示文本；每次 `Action::is_edit()` 用前后文本 diff，经 map 换算成源码范围再 `replace_range`。显示重建后若与编辑器文本不一致（例如刚输入成对标记），重置 Content；普通中文/英文输入不会重置。
- 块切换：`switch_block` 先把 `block_edits` 合并回 `doc.source` 并 `doc::parse`，再按原块源码位置找到新块打开。这样切走后其他块立即显示最新结果；旧实现从 `doc` 原始 runs 渲染，会把已编辑的列表项“变回去”。
- 保存：`apply_block_edits` 按原 span 逆序替换，写盘后重新解析、回阅读并恢复 `edit_ratio`；无路径弹保存对话框；未保存切换文件/关闭窗口拦截。
- 限制（v1）：一次只编辑一块，不能跨块连续选择；链接只能改显示文字；标题级别/表格增删列没有 UI；删除整块格式化文本可能留下空标记（已处理“整块清空”情况）；编辑模式每次输入重建整篇非活动块，超大文档性能待测。
- 为什么不做 Moeka 式单缓冲：Moeka 的 Edit 模式是自写 GPUI 文本引擎（buffer + LineDecoration + display_text/buffer_to_display/display_to_buffer + 光标映射）。iced 0.14 的 `text_editor` 不暴露光标/选区范围，也只在 Highlighter 里支持颜色和字体，不支持 conceal；要完全复刻必须自写编辑器。当前用“每块一个 text_editor + 标记隐藏”逼近，风险与代码量可控。

## 保存与完整性标签（2026-11-19）

- 现象：`dist\sudu.exe` 编辑后 Ctrl+S 报 `保存失败：拒绝访问。 (os error 5)`；同一份代码从 `D:\AI\sudu-target\debug\sudu.exe` 启动则保存正常。
- 根因：`D:\AI\杂` 目录带显式 `Mandatory Label\Low Mandatory Level:(OI)(CI)(NW)`，`dist` 与其中的 exe 继承 Low。Windows 会按映像的完整性标签启动进程，Low 进程不能写 Medium 文件（no-write-up）。
- 证据：token 查询 `dist\sudu-diag.exe` 进程为 `S-1-16-4096`（Low），`D:\AI\sudu-target\debug\sudu.exe` 为 `S-1-16-12288`（High）；`icacls dist\sudu-diag.exe /setintegritylevel Medium` 后进程变为 `S-1-16-8192`（Medium），Ctrl+S 成功写入 `D:\AI\sudu-target\save-test.md`（内容 `MEDIUMOK`）。
- 解决：以后用 `tools\package.ps1` 复制 release 并设置 Medium；`dist\sudu.exe` 与 `dist\sudu-tiny.exe` 已设置。不要直接 `cp` 到 `dist`，复制会继承 Low 标签。
- `D:\AI\杂` 本身的标签未改，避免影响目录内其他工具；如果以后要彻底恢复该工作区的普通权限，需要单独确认后再处理。

## 应用图标

- 用户提供黑书本白底 JPG；`tools/make_icon.py` 用 Pillow 裁内容、30%→12% padding、白色圆角 tile（1024，radius 230），输出 `assets/icon-256.png` 与多尺寸 `assets/icon.ico`。
- 窗口图标：`image::load_from_memory` 解码 PNG → `iced::window::icon::from_rgba`；taskbar 显示。
- exe 资源：`winresource` 直接跑 `windres` 会因中文源路径报 `Bad file descriptor`（`-I D:\AI\杂\素读`）；改为 `build.rs` 把 `icon.ico`/`icon.rc` 复制到 `OUT_DIR` 的 ASCII staging 目录，cwd 那里执行 `windres`，输出 `icon.o`，再用 `cargo:rustc-link-arg` 链接。
- 文件关联的 `DefaultIcon` 指向 `dist\sudu.exe`，exe 内嵌图标后 Explorer / 打开方式可见。

## v0.2.0 升级纪要（2026-11-28）

- **统一文档引擎（`doc_widget.rs` + `editor.rs`）**：移除 `iced_selection` 与块级编辑隔离层 `live.rs`，改为直接在 `DocWidget` 内基于 `cosmic_text::Buffer` 与 `Paragraph` 管理整篇阅读态与连续 WYSIWYG 编辑态，实现跨块/跨表单元格连续选区复制、方向键按视觉行跨块移动、`InputMethod::Enabled` 就地 IME 合成、工具栏格式与表格单元格编辑（`Tab`/增删行列/对齐）。
- **视口虚拟化**：`DocWidgetState` 维护全篇估算高度与 `prefix_ys`，仅在 `[local_top - 900, local_bottom + 900]` 区间实例化 `Paragraph`/表格/图像，超出 `2600px` 回收布局对象仅保留实测高度，编辑单块通过 `edited_block` 增量重排。
- **表格与图像（`table.rs`、`images.rs`）**：补齐 GFM（行内代码管道/转义/空单元格/列数容错/`<br>`）与原生 HTML `<table>`（含 `colspan`/`rowspan`）解析、独立水平滚动条、居中/右对齐水平偏移校准；`ImageStore` 缓存解码本地光栅图（PNG/JPEG/WebP/GIF/BMP）与 SVG。
- **会话持久化与滚动稳定（`session.rs`、`main.rs`、`app.rs`）**：新增 `session.json` 保存上次文件、分文件阅读比例、主题、字号、侧栏及窗口坐标尺寸；在 `window::Settings` 中显式设置 `exit_on_close_request: false`，确保 OS `WM_CLOSE` 能触发 `WindowCloseRequested` 完成落盘与未保存保护；将 `find_bar` 固定包裹在常量 `stack` 内，开关查找/`Esc` 不再重置 `Scrollable`；`SUDU_SESSION` / `SUDU_NO_SESSION=1` 供测试隔离。
- **截图脚本（`tools/shot.ps1`）**：改用 `EnumWindows` 选取进程最大可见窗口（避开 winit `50x50` 隐藏辅助窗），并在后台会话 `CopyFromScreen` 失败时回退到 `PrintWindow(hwnd, hdc, 2)`。





