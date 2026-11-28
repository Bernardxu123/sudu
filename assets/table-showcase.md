# 表格渲染压测集（Table Showcase）

本文件用于全面验证 `sudu` 在各类标准、复杂及脏数据表格场景下的解析准确性、布局自适应、横向滚动、长文本断行、主题适配与交互体验。

## 1. 基础 3×3 表格与左 / 中 / 右对齐

| 左对齐列 | 居中对齐列 | 右对齐列 |
| :--- | :---: | ---: |
| 项目 Alpha | 已发布 | 1,024 |
| 项目 Beta | 内测中 | 256 |
| 项目 Gamma | 规划中 | 64 |

## 2. 中英文混排、数字与 Emoji 混合表格

| 模块 Module | 状态 Status | 耗时 (ms) | 说明 Description |
| :--- | :---: | ---: | :--- |
| 解析器 Parser | ✅ 稳定 | 12.4 | 支持中文标点、English tokens 与数字 `2026` 混排 |
| 布局器 Layout | 🚀 优化 | 4.8 | 自适应列宽测算：CJK 全角与半角字符混合对齐 |
| 主题 Themes | 🎨 四套 | 0.9 | 白 / 纸 / 夜 / 米黄全主题网格线与表头区分 |
| 交互 Interaction | 🔍 可搜 | 1.2 | 支持选中复制、`Ctrl+F` 搜索高亮与 `Shift+滚轮` 横移 |

## 3. 超宽多列表格（10 列，触发独立横向滚动容器）

| ID | 服务名称 | 环境 | 协议 | 监听地址 | P50 延迟 | P99 延迟 | QPS | 健康状态 | 负责人与备注 |
| ---: | :--- | :---: | :---: | :--- | ---: | ---: | ---: | :---: | :--- |
| 101 | auth-gateway | prod | HTTPS | `10.24.18.101:8443` | 3.2 ms | 18.5 ms | 12,480 | 🟢 正常 | 张伟 · 核心鉴权入口，支持 JWT 与 mTLS |
| 102 | markdown-renderer | prod | gRPC | `10.24.18.102:9090` | 1.8 ms | 7.4 ms | 28,900 | 🟢 正常 | 李娜 · 负责 GFM AST 与增量块渲染 |
| 103 | search-indexer | staging | HTTP/2 | `10.24.19.45:8080` | 6.5 ms | 41.0 ms | 3,120 | 🟡 观察 | 王强 · 全文分词索引重建中 |
| 104 | asset-cdn-proxy | prod | QUIC | `10.24.20.11:443` | 0.9 ms | 4.1 ms | 54,200 | 🟢 正常 | 赵敏 · 静态资源边缘缓存节点 |

## 4. 超长段落、长 URL 与长无空格字符串

| 类别 | 目标 / 示例 | 详细说明（长文本自动折行检查） |
| :--- | :--- | :--- |
| 长段落 | 中文排版测试 | 阅读器在面对单元格内大段说明文字时，不应把前两列挤压成无法阅读的竖排单字，也不应让整张表格撑破居中正文容器；应当在分配合理列宽后让长文本自然折行，同时保持同一行其他单元格对齐稳定。 |
| 长 URL | https://docs.example.com/v2/organizations/sudu-reader/repositories/markdown-table-rendering-engine/issues/987654321?filter=overflow_wrap_word_or_glyph&theme=paper | 当单元格内包含极长且无空格的 URL 时，文本组件必须使用 `WordOrGlyph` 策略在单元格内部断行，而不能溢出边界遮挡右侧单元格。 |
| 长无空格串 | `SHA256_9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08_VERYLONGCONTINUOUSTOKEN` | 连续无空格哈希值与标识符同样应在单元格边界内平滑折行。 |

## 5. 单元格行内元素与特殊字符

| 语法特性 | 源码写法示例 | 预期显示效果 |
| :--- | :--- | :--- |
| 行内代码含竖线 | `` `a | b` `` 与 `` `x \| y` `` | 完整保留为单列行内代码：`a | b` 与 `x \| y`，不误切列 |
| 表头含代码竖线 | — | 下方独立测试表头含 `` `foo | bar` `` 的场景 |
| 转义竖线 | `A \| B \\| C` | 显示竖线 A \| B，而 `\\|` 正确切列 |
| 富文本混排 | `**粗体** *斜体* ~~删除线~~` | **粗体**、*斜体*、~~删除线~~ 与 `行内代码` |
| 链接与 `<br>` 换行 | `<br>` + `[示例链接](...)` | 第一行文字<br>第二行包含 **加粗** 与 [外部链接](https://example.com)<br />第三行收尾 |

| `foo | bar` | 多反引号 ``x | y`` | 备注 |
| :--- | :--- | :--- |
| 表头含代码竖线 | 同样不能误判列数 | 正常解析为 3 列表格 |

## 6. 缺少首尾管道、空单元格、缺失列与多余列容错

无外框 | 居中列 | 右对齐列
:--- | :---: | ---:
首行正常 | 中间 | 10
| | 第二列非空（首列为空） | 20
| 仅一列（缺失后两列） |
| | |
| 一 | 二 | 三 | 多出第四列 | 多出第五列

## 7. 块级边界：紧邻标题、列表、代码块前后（无空行）

### 7.1 紧邻标题前后
| 紧邻前 | 状态 |
| --- | --- |
| 在二级标题正下方 | 正常 |
#### 紧邻表格正下方的四级标题

### 7.2 紧邻列表前后
- 紧邻表格上方的无序列表项
| 列表后表格 | 说明 |
| --- | --- |
| 紧贴列表项 | 不应被吸进列表 |
- 紧邻表格下方的无序列表项

### 7.3 紧邻代码块前后
```rust
let before_table = "紧邻代码块下方";
```
| 代码块后表格 | 结果 |
| --- | --- |
| 紧贴围栏 | 正常分块 |
```rust
let after_table = "紧邻表格下方";
```

## 8. 原生 HTML 表格（含 `colspan` / `rowspan`）

<table>
  <thead>
    <tr>
      <th rowspan="2" align="center">分类</th>
      <th colspan="2" align="center">核心指标</th>
      <th rowspan="2" align="left">备注说明</th>
    </tr>
    <tr>
      <th align="right">wgpu</th>
      <th align="right">tiny-skia</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td rowspan="2" align="center"><b>桌面渲染</b></td>
      <td align="right">11.6 MB</td>
      <td align="right">7.0 MB</td>
      <td>支持 <code>colspan</code> / <code>rowspan</code> 合并单元格与网格避让</td>
    </tr>
    <tr>
      <td align="right">184 MB</td>
      <td align="right">27 MB</td>
      <td>单元格支持 <a href="https://example.com">链接</a>、<i>斜体</i> 与<br>多行 <b>&lt;br&gt;</b> 换行</td>
    </tr>
    <tr>
      <td align="center">总结</td>
      <td colspan="3" align="left">跨 3 列的底部汇总单元格（<code>colspan="3"</code>）</td>
    </tr>
  </tbody>
</table>
