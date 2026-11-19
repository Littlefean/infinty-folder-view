# Markdown Atlas

一个使用 **Rust + Skia/SkParagraph + Metal** 的原生无限画布阅读工具。导入文件夹后，目录展开成嵌套框，Markdown、代码和纯文本展开成卡片；支持自动排版、碰撞推挤拖动、文字选择与复制。

当前只实现 macOS Metal 后端。画布与文字均由原生 Skia 绘制，没有 WebView 或 DOM。

## 启动

需要 macOS、支持 Metal 的 GPU、Rust 工具链和 Xcode Command Line Tools。第一次编译需要联网下载 Rust 依赖及匹配的 Skia 预编译库；若没有匹配的预编译库，依赖可能转为从源码构建。

```sh
cd /Users/fittencode/Desktop/Projects/infinity-markdown-skia
cargo run --release
```

也可以双击项目中的 `Launch.command`。启动器会使用已有 release 二进制；没有时才编译。修改源码后，应先运行 `cargo build --release` 更新二进制。

首次启动会在项目的 `demo-markdown/` 中生成 **72 份真实 Markdown 文件**，按 12 个主题、6 个目录组织，每份包含较长的中英文正文、代码和其他样式。生成器不会覆盖已有文件，因此可以直接修改样例再重新启动。

为正常比较交互表现，请使用 release 构建。首次出现新字体或内容时仍可能发生缓存准备；本项目不预设或承诺特定帧率。

## 操作

| 操作 | 效果 |
| --- | --- |
| 中键按住拖动 | 平移视野 |
| 空格 + 左键拖动 | 平移视野，适用于没有中键的设备 |
| 在正文中左键拖动 | 选择文字 |
| 双击 / 三击正文 | 选择词 / 段落，继续拖动可按对应单位扩展 |
| `Shift` + 点击或拖动 | 从已有选区的起点扩展选择 |
| `⌘A` | 选择当前卡片的全部正文，先点击该卡片内的文字 |
| `⌘C` | 复制选中的纯文本 |
| 拖动文件 / 文件夹标题栏 | 移动卡片或整棵目录子树；碰到同层节点时推开它们 |
| 单击文件封面 / 标题 | 选中文件，原位置显示完整正文；双击平滑定位到阅读比例 |
| 单击 / 双击目录封面或标题 | 单击展开一层，双击递归展开全部内容，保持相机和布局不变 |
| 在画布或展开目录内部的空白处左键拖动 | 框选节点；展开目录需要完全框住，文件和封面相交即可 |
| `Shift` + 点选节点 / 框选 | 点选增减节点，框选追加；拖动选中节点的标题可整体移动 |
| 拖动远景封面 | 移动对应文件或目录；超过 4 像素才开始移动 |
| 点击画布空白处 | 清除文字与节点选择 |
| `⌘O` / 顶部「导入」 | 通过原生选择器打开一个文件或文件夹，替换当前画布 |
| `A` / 顶部「自动排版」 | 选中文件夹时整理其直接子项；否则重新整理整个目录树 |
| 鼠标滚轮 / 触控板滚动 | 围绕鼠标位置平滑缩放，松手后逐帧逼近目标 |
| `F` / 顶部「全览」 | 平滑过渡到全部已排版内容的概览 |
| `1` | 围绕窗口中心平滑恢复 100% 比例 |
| `R` | 平滑回到初始阅读视角并清除错误提示 |
| `L` | 切换远景细节：智能概览 / 完整文字 |
| 方向键 | 平滑平移，连续按键累积目标位置 |
| 点击右下角小地图 | 平滑移动到对应位置 |
| 拖入文本文件或目录 | 加载这批内容，替换当前画布 |
| `Esc` | 清除选区、停止相机动画和拖动状态、清除错误提示 |
| `⌘Q` | 退出 |

缩放范围为 1.5%–400%。智能细节参考 `infinity-code-view`：文件低于约 57% 时，显示居中、自适应大小的完整文件名封面；名称始终保持一行，高于宽的封面将名称顺时针旋转 90°，按框内空间缩放并保留内边距，不换行、不截断。目录按屏幕尺寸与层级逐层显示大标题封面，沿用相同的名称适配规则，放大后展开子节点。封面替换正文但不改变节点尺寸或位置，边框随缩放补偿以保持可见。按 `L` 可以切换到完整内容。

选中文件后，即使在远景也会在原卡片中显示正文，并保持祖先目录展开；取消选中后恢复按缩放决定的封面。目录单击只展开一层，子文件仍按各自缩放比例展示；双击目录递归展开全部内容。只有点击目录才会展开，框选目录不会自动展开。框选只命中当前可见层级，不会选到封面后隐藏的子项。目录内部空白支持框选，拖动目录标题或封面则移动整个目录。

缩放参考 Project Graph 的“当前值逐帧逼近目标值”：滚轮累积目标比例，当前视口随时间连续追近。采用 `alpha = 1 - exp(-13.984 × dt)`，相当于其默认 `0.11` 逼近系数在 120Hz 下的速度，约 214ms 消除 95% 的差距。按实际时间计算，因此 60Hz 与 120Hz 的动画时长相同。每帧同时更新比例与偏移，使最后一次滚轮事件的鼠标锚点保持不动；小于收尾阈值时精确到达目标并停止重绘。

中键 / 空格拖拽仍然直接跟手，按下时打断尚未结束的缩放；这与 Project Graph 的拖拽路径一致。此处的“漂移”指缩放逐步减速逼近，不给中键拖拽额外添加松手惯性。

目录层级与导入时的文件结构一致，包括空目录。Markdown 卡片宽 720，代码和纯文本卡片宽 1024，高度按实际排版结果计算。初次加载会逐层自动排布；点击文件夹标题或内部空白选中目录，再按 `A` 只整理该目录的直接子项，保留子目录内部的摆放。点击画布空白取消节点选择后按 `A`，会重新排布整棵树。

拖动时保持原有父子关系：文件夹带着其全部子项移动，同层节点发生碰撞时沿实际接触边推挤，贴边滑动不会把相邻文件块甩到角落；快速拖动会连续检测碰撞链，父框随内容重新计算边界。拖动只改变画布位置，不移动磁盘文件。支持 Shift 多选和框选后整体移动，父子节点同时选中时子项只移动一次。当前不支持卡片调宽、撤销或布局保存，重新导入和退出会丢失手动布局。

## 选择与复制

正文、标题、代码和表格中的文字都可以选择。单击确定起点，按住左键拖出选区；双击按词选择，三击按段落选择。代码按行排版，因此三击选择当前代码行；表格中三击选择当前单元格。`Shift` + 点击或拖动可以沿用原来的起点扩展选区。拖选到画布边缘时，视野会自动平移，便于选择屏幕外的后续内容。

选择范围限定在同一张卡片内。平移、缩放时保留选区，并按文字布局重新绘制高亮；正文被封面替代时，不能直接在隐藏正文上开始文字选区；先单击文件封面即可显示正文。点选正文也会选中该文件，因此缩小时仍保留可见内容。也可以按 `L` 切换为完整文字。

`⌘C` 复制的是展示内容的纯文本，不包含自动绘制的代码行号或语言标签。Markdown 卡片不包含格式标记，普通段落之间用两个换行分隔，表格单元格用 Tab 分隔、表格行用换行分隔；代码与纯文本文件保留字面内容、缩进、空行及末尾换行。导入时去掉 UTF-8 BOM，并把 CRLF / CR 换行统一为 LF。复制后状态栏显示结果。`⌘A` 只选择当前卡片的正文，不包含卡片标题栏中的文件名和统计信息。

用专门的样例体验英文比例字体、中英混排、emoji 与组合字符、粗体、行内代码、跨段落、代码块和表格：

```sh
cargo run --release -- --dir tests/selection-fixture.md
```

首段为 `Select this exact phrase.`，可以拖选这句话，按 `⌘C` 后在文本编辑器中粘贴检查。当前不支持文本编辑、无障碍文本接口或跨卡片选区。

## 加载自己的文件

```sh
# 递归加载项目目录中的 Markdown、代码和纯文本
cargo run --release -- --dir "/path/to/project"

# 也可以只打开一个文件
cargo run --release -- --dir "/path/to/README.md"

# 增加默认样例数量
cargo run --release -- --demo-count 200
```

也可以在运行中按 `⌘O`、点击顶部「导入」，或把文件 / 文件夹拖入窗口。文件读取在后台进行，随后逐文件排版；导入成功后替换当前画布。

`.md`、`.markdown` 使用 Markdown 富文本展示；Rust、JavaScript / TypeScript、Python、C / C++、Java、Go、JSON、CSS、HTML、Shell 等常见代码扩展名按语言高亮，不支持的语言回退到普通代码样式。其他 UTF-8 文本按纯文本显示，保留字面的 Markdown 标记。扩展名不区分大小写。

目录扫描遵守 `.gitignore`（包括嵌套规则），保留未被忽略的隐藏文件与空目录；不跟随符号链接。跳过 `.git`、`.hg`、`.svn`、`target`、`node_modules`、`dist`、`build`、`vendor`、`__pycache__`、`coverage`、`.cache`、`.next`、`.nuxt`、`.venv` 和 `venv` 目录。

单文件限制 2 MiB，总文本限制 128 MiB，最多导入 2000 份文本、扫描约 20000 个目录条目。文件夹中遇到二进制、非 UTF-8、超大或无法读取的文件会跳过并提示；达到总量上限时保留已读取的内容并提示。单独打开无效文件则报错。当前跳过的文件不生成占位卡。加载过程只读取指定文件，不改写它们。

`--demo-count` 接受 1–2000；它只补充缺失的样例，不删除旧样例。如果曾生成 200 份，之后使用默认参数仍会读取这 200 份。`--dir` 会关闭样例生成。

可通过 `MARKDOWN_ATLAS_DEMO_DIR` 改变默认样例存放位置。示例：

```sh
MARKDOWN_ATLAS_DEMO_DIR="/tmp/markdown-atlas-demo" ./Launch.command --demo-count 96
```

## Markdown 展示范围

已实现标题层级、正文、粗体、斜体、删除线、行内代码、链接样式、分隔线、引用、列表、任务清单和基础表格。围栏代码块使用 Syntect 语法高亮；中文正文、等宽代码和 emoji 使用系统字体回退。

目前是只读阅读工具，尚未实现文件编辑、批注、可点击链接、图片资源加载、搜索、连线或文件监视。原始 HTML 不作为网页执行，复杂 Markdown 扩展也不保证与浏览器渲染完全一致。

## 渲染结构

内容处理分为文件读取、目录树建立、文本排版、空间布局和 Metal 绘制：

- `src/documents.rs`：生成示例文件，扫描目录与忽略规则，识别文本类型并限制输入大小。
- `src/markdown.rs`：用 pulldown-cmark 解析 Markdown，用 Syntect 高亮代码，逐行展示纯文本，用 SkParagraph 缓存排版结果。
- `src/layout.rs`：递归目录布局、父框边界、子树移动和同层碰撞推挤。
- `src/camera.rs`：世界坐标、屏幕坐标、鼠标锚定缩放、时间相关的相机缓动与可见范围计算。
- `src/scene.rs`：嵌套框与文件卡片、可见层级裁剪、文字选择、点选与框选、目录展开、多节点拖动和小地图。
- `src/presentation.rs`：缩放细节阈值、边框补偿、按字体实际度量适配并缓存的大标题封面。
- `src/renderer.rs`：winit 原生窗口、CAMetalLayer、Skia Metal surface 和帧提交。
- `src/main.rs`：输入、按需重绘、原生导入入口、后台读取、文件拖放和自动运行检查。
- `src/file_picker.rs`：macOS 原生文件 / 文件夹选择器。
- `src/animation_check.rs`：单次缩放输入后的续帧、收敛、锚点稳定与空闲检查。
- `src/clipboard.rs`：通过 macOS NSPasteboard 复制选中的纯文本。

每次重绘最多排版一份待加载文档，让初始加载期间的窗口可以逐步显示内容。每份文档排版后保留 SkParagraph；平移和缩放复用它们，不按相机比例重新断行。按目录树裁剪不在视野中的分支，代码再按行跳过不可见内容。拖动和自动排版只更新空间几何，不重新排版文字。

Skia 的 GPU 资源缓存上限设为 256 MiB；这不是整个应用的内存上限。文本、语法高亮数据和全部文档布局仍保存在内存中。当前没有按需文件读取、后台排版或布局缓存淘汰，所以极大输入的加载时间与内存仍需要进一步优化。

相机动画期间，每次绘制后通过 winit `request_redraw()` 继续请求下一帧，Metal 开启垂直同步；取得 drawable 后立即按实际时间更新相机，再用 Skia 绘制。无需依赖下一次滚轮事件，也不以固定 16ms 定时器限制高刷新率显示器。静止时回到等待事件；窗口完全遮挡时暂停，恢复后重置动画计时。界面里的 CPU 提交时间包含 drawable 获取、绘制与提交，不能直接当作 GPU 耗时或显示器实际 FPS。连续动画机制已经具备，复杂文字场景是否能稳定 60fps 仍取决于单帧绘制成本。

## 验证

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check

# 打开真实 Metal 窗口，自动平移缩放并保存截图和计时
cargo run --release -- --smoke-test work/smoke

# 只发送一次缩放目标，验证无新输入时仍逐帧运动，收敛后停帧
cargo run --release -- --animation-test work/animation
```

自动运行检查会完成 180 个呈现帧，写出三张截图和 `report.txt`，然后退出。窗口需要处于可呈现状态，不能在完全遮挡或最小化时评估运行结果。报告区分 CPU 绘制/提交耗时与事件/呈现间隔，并排除截图帧；它不是 GPU 时间戳测量，也不是完整的交互性能基准。

动画检查与普通 smoke test 分开运行：前者不强制连续重绘，依赖真实动画调度，写出逐帧 `frames.csv` 和 `report.txt`。它检查每帧鼠标锚点、最终目标、停止输入后的连续运动，以及收敛后的空闲重绘次数。10 秒看门狗会检测续帧遗漏；窗口遮挡或人工操作会影响结果。

单元测试覆盖相机的鼠标锚定与坐标转换、60/120Hz 下的动画时间一致性、滚轮累积与反向输入、换锚点、动画取消与收敛、Markdown 解析边界、嵌套目录与忽略规则、文件加载约束及示例文件保护。布局测试覆盖父框包含、子树移动、碰撞链、快速拖动与局部排版。选择相关测试覆盖 SkParagraph 命中与选区矩形、UTF-16 与 UTF-8 索引转换、emoji 和组合字符边界、跨段落/代码/表格复制、源码字面内容与换行、反向拖选、缩放后的选区、边缘自动平移与高亮缓存。

## 打包成可双击应用

```sh
cargo build --release
./scripts/package-app.sh
open "dist/Markdown Atlas.app"
```

打包脚本使用已有 release 二进制；没有时才构建。结果位于 `dist/Markdown Atlas.app`，以本地 ad-hoc 方式签名，包含本机架构的可执行文件。它不是通用架构或经过公证的分发包。

应用内部不运行 Cargo。首次启动会在 `~/Library/Application Support/Markdown Atlas/demo-markdown/` 生成样例，后续保留修改。项目启动器与 `.app` 使用各自的默认样例目录。

## 上游参考与许可

Metal surface 的接入参考了 rust-skia 官方的 [metal-window 示例](https://github.com/rust-skia/rust-skia/tree/master/skia-safe/examples/metal-window)（MIT）。本 demo 的文档加载、示例内容、Markdown 处理、画布场景与相机交互围绕本实验实现。

相关文档：[rust-skia](https://github.com/rust-skia/rust-skia)、[SkParagraph](https://github.com/google/skia/tree/main/modules/skparagraph)、[winit](https://docs.rs/winit/)、[pulldown-cmark](https://docs.rs/pulldown-cmark/)、[Syntect](https://docs.rs/syntect/)。依赖库保留各自的许可证。

rust-skia 上游许可声明：

```text
MIT License

Copyright (c) 2019 LongYinan & Armin Sander
Copyright (c) 2019 rust-skia Contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
