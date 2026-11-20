# 素读（sudu）

一个安静、轻量的中文 Markdown 阅读器。Rust + iced 0.14，原生窗口，无 Electron、无浏览器壳。

- 阅读优先：默认只读；点右上角铅笔（Ctrl+E）进入渲染态编辑，页面保持排版，点击哪一块就原位编辑哪一块；Ctrl+S 保存。
- 中文优先：四套主题、系统字体、中文标点与行高按阅读调校。
- 大纲、文档内搜索、设置菜单、自定义标题栏。
- 两种构建：GPU（wgpu，流畅优先）与软件渲染（tiny-skia，轻量优先）。

## 现在能做什么

- 打开 `.md` / `.markdown` / `.mdx`，或用参数直接打开：`sudu.exe D:\path\doc.md`。
- 渲染态原位编辑：右上角铅笔或 Ctrl+E 进入；页面不切成源码页，点击段落、标题、列表项、引用、代码块或表格行，该块在原位变成同版式的编辑区。Markdown 标记在该块内隐藏，粗体/斜体/链接/行内代码按主题样式显示；Ctrl+S 保存整篇（无路径时弹保存对话框）。未保存修改时切换文件/关闭窗口会被拦截。
- 启动时显示内置「快速入门.md」。
- 左侧大纲：只列标题、按层级缩进、点击跳转，滚动时当前章节高亮；不显示文件树。
- 文档内搜索：`Ctrl+F`，显示 `n / m`，`Enter` / `Shift+Enter` / `F3` 跳转，命中高亮。
- 设置菜单：主题（白 / 纸 / 夜 / 米黄）、大纲侧栏、查找、打开文件、打开文件夹、放大/缩小字号、重读文件、关于。
- 自定义标题栏：拖动移动、双击最大化/还原、四边与四角缩放、最小化/最大化/关闭。
- 打开文件夹后用快速打开列表选文件（不建文件树）。
- 选中/复制、站内相对链接、`#anchor` 跳转、外部链接交给系统浏览器。
- Ctrl+T 循环主题；Ctrl+= / Ctrl+- 调整字号；Ctrl+R 重读文件；Ctrl+E 编辑；Ctrl+S 保存。

## 快捷键

| 操作 | 快捷键 |
|---|---|
| 打开文件 | Ctrl+O |
| 打开文件夹（快速打开） | Ctrl+Shift+O |
| 大纲侧栏 | Ctrl+B |
| 文档内查找 | Ctrl+F |
| 进入编辑 / 保存并返回 | Ctrl+E / Ctrl+S |
| 下一个 / 上一个命中 | Enter / Shift+Enter，或 F3 / Shift+F3 |
| 切换主题 | Ctrl+T |
| 放大 / 缩小字号 | Ctrl+= / Ctrl+- |
| 重读文件 | Ctrl+R |
| 关闭浮层 | Esc |

## 构建

工具链是 **WinLibs MinGW-w64 (UCRT) + Rust `stable-x86_64-pc-windows-gnu`**。MSVC 官方 CDN 实测约 0.25 MB/s，安装时间不可接受，因此改用 GNU；如果以后装好 MSVC，把 `rust-toolchain.toml` 改回 msvc 即可。

GNU ld 不认中文路径，`.cargo/config.toml` 把 `target-dir` 固定到 ASCII 路径 `D:/AI/sudu-target`，并显式指定 WinLibs 的 `gcc.exe`。构建时把 WinLibs 的 `bin` 放进 PATH（`cc` 构建脚本需要）。

```powershell
$mingw = "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
$env:Path = "$mingw;$env:USERPROFILE\.cargo\bin;$env:Path"
Set-Location D:\AI\杂\素读

cargo build --release                              # 默认 GPU（wgpu）
cargo build --release --no-default-features --features backend-tiny   # 轻量（tiny-skia）
```

产物在 `D:\AI\sudu-target\release\sudu.exe`；本仓库的 `dist\` 里放了两种构建。

`D:\AI\杂` 目录带 Low 完整性标签，直接复制进 `dist\` 的 exe 会继承该标签，运行时无法写入普通文件（保存报“拒绝访问”）。打包时用脚本复制并设置 Medium：

```powershell
pwsh tools\package.ps1 -Source D:\AI\sudu-target\release\sudu.exe -Dest D:\AI\杂\素读\dist\sudu.exe
```

`dist\sudu.exe` 与 `dist\sudu-tiny.exe` 已按此设置。

## 实测（release，本机 Windows 11 / 32 GB）

| 后端 | exe | 首事件中位 | 空闲 WorkingSet | 快速滚轮 CPU |
|---|---:|---:|---:|---:|
| **wgpu（默认）** | 11.57 MB | 968 ms | 184 MB | 0.03 核 |
| tiny-skia | 6.93 MB | 137 ms | 26.8 MB | 0.56 核 |

滚动改成和 Moeka 一样的“即时滚动”：滚轮每行 = 正文字号 × 行高（约 31 px），立即 `scroll_to`，不做 200 ms 缓出；触控板像素增量 1:1。

## 已支持 / 未支持

已支持：标题、段落、粗体/斜体/删除线/行内代码、链接、围栏代码块、引用、有序/无序/任务列表、表格（行式排版）、分隔线、frontmatter 跳过、BOM、中文路径 percent-decode；源码编辑与保存。

未支持（v1 明确不做或待做）：

- 跨块的连续选择/拖拽、一次编辑多个块；当前一次只把一块变成编辑区。
- 表格增删行列、列宽调整；表格按行原位编辑，首尾管道隐藏，内部管道保留。
- 链接只能改显示文字，不能改 URL；标题不能直接改级别；没有 B/I 工具栏或快捷键。
- Mermaid、LaTeX 公式、脚注、原始 HTML 渲染。
- 图片目前显示 `[图片：alt]` 占位，不加载本地图片。
- 1 MB 文档仍会一次性构建全部块（tiny-skia 实测约 233 MB WorkingSet）；虚拟列表待做。
- 会话恢复、单实例、文件关联、自动更新。

## 验证

- `cargo test`：10 个测试通过（标题 slug、链接 percent-decode、搜索、引用/表格类型、内容宽度、块源码范围、行内标记隐藏/样式、链接目标隐藏、显示到源码的编辑映射、引用前缀与表格外框）。
- 选区：中文拖选 + Ctrl+C 复制正确；跨段选中会清除上一段高亮。
- 大纲跳转、主题切换、设置菜单、查找、窗口拖动/缩放/最大化均手测通过。
- 渲染态编辑：点铅笔后页面排版不变；标题/段落/列表/引用/代码块/表格行逐块点击进入编辑，Markdown 标记隐藏；中文输入、追加文字、切换块后保留、Ctrl+S 落盘均实测；表格首尾管道隐藏后单元格编辑不会破坏表格。
- 滚动即时性：删除动画后滚轮事件当帧生效；wgpu 滚动 CPU 0.03 核。
- 未验证：跨块选择/拖拽、表格增删列、链接 URL 编辑、150% DPI、干净机器字体差异、1 MB 文档编辑性能。

## 设计参考

- [rmdv](https://github.com/minchenlee/rmdv)（MIT）：主题组织、事件→元素、锚点滚动、CollectOffsets 模式。
- [Moeka](https://github.com/MoekaProject/moeka)（GPL-3.0-or-later）：只借鉴思路，未复制代码——GPUI 的 GPU/120Hz、`on_scroll` 立即应用行高步长、可变高度虚拟列表（待做）。
- 图标来自 `D:\AI\杂\iconly-icons` 的 Light-Outline Search / Setting，其余窗口控件为同风格自绘 SVG。
