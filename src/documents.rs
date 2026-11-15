use std::{
    error::Error,
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::{self, Read, Write as _},
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;

pub struct SourceDocument {
    pub path: PathBuf,
    pub title: String,
    pub markdown: String,
    pub line_count: usize,
}

struct Topic {
    folder: &'static str,
    slug: &'static str,
    title: &'static str,
    subtitle: &'static str,
    question: &'static str,
    principle: &'static str,
    vocabulary: [&'static str; 4],
}

const TOPICS: [Topic; 12] = [
    Topic {
        folder: "01-foundations",
        slug: "spatial-reading",
        title: "空间阅读与记忆",
        subtitle: "A notebook for thinking in two dimensions",
        question: "当文件不再藏在标签页里，我们如何记住它们的位置？",
        principle: "把相互关联的内容放在一起，让距离表达上下文。",
        vocabulary: ["空间记忆", "上下文", "阅读路径", "关联线索"],
    },
    Topic {
        folder: "01-foundations",
        slug: "camera-coordinates",
        title: "相机与坐标系统",
        subtitle: "World space stays still while the camera moves",
        question: "鼠标下的内容，能否在缩放时保持原位？",
        principle: "世界坐标决定文档布局，屏幕坐标只决定最后的显示位置。",
        vocabulary: ["世界坐标", "屏幕坐标", "缩放中心", "逆变换"],
    },
    Topic {
        folder: "02-rendering",
        slug: "text-layout",
        title: "文字排版实验",
        subtitle: "Typography is geometry with meaning",
        question: "中文、英文与代码混排时，怎样让每一行都自然？",
        principle: "先完成字形选择与断行，再把稳定的布局交给渲染器。",
        vocabulary: ["字形整形", "基线", "断行", "字体回退"],
    },
    Topic {
        folder: "02-rendering",
        slug: "gpu-batching",
        title: "GPU 绘制流水线",
        subtitle: "Small commands, large scenes",
        question: "怎样减少每一帧的重复工作，同时保留细腻的文字？",
        principle: "复用稳定的数据，让相机变化尽可能不触发文档重排。",
        vocabulary: ["批处理", "字形缓存", "纹理上传", "帧预算"],
    },
    Topic {
        folder: "03-documents",
        slug: "markdown-blocks",
        title: "Markdown 文档结构",
        subtitle: "From source text to a readable page",
        question: "一个普通文本文件，如何成为可自由摆放的阅读单元？",
        principle: "保留内容的语义，把标题、段落和代码块分别布局。",
        vocabulary: ["语义块", "内联样式", "标题层级", "源文件"],
    },
    Topic {
        folder: "03-documents",
        slug: "source-navigation",
        title: "代码导航设计",
        subtitle: "Keep the surrounding explanation visible",
        question: "阅读函数实现时，如何同时看见调用者和设计说明？",
        principle: "让代码与解释拥有共同的空间，而不是互相替换。",
        vocabulary: ["调用关系", "源码片段", "批注", "定位"],
    },
    Topic {
        folder: "04-experiments",
        slug: "cache-lifetime",
        title: "缓存的生命周期",
        subtitle: "Remember expensive work, forget it deliberately",
        question: "缓存何时有效，什么时候应该主动释放？",
        principle: "每一种缓存都需要明确的失效条件与容量上限。",
        vocabulary: ["内容版本", "失效条件", "容量预算", "淘汰策略"],
    },
    Topic {
        folder: "04-experiments",
        slug: "viewport-culling",
        title: "可见范围裁剪",
        subtitle: "The canvas is huge; the screen is finite",
        question: "屏幕之外的数万行文字，需要参与当前帧吗？",
        principle: "先筛选可见文档，再筛选文档内的可见内容。",
        vocabulary: ["包围盒", "可见行", "空间索引", "细节层级"],
    },
    Topic {
        folder: "05-interaction",
        slug: "reading-gestures",
        title: "阅读手势笔记",
        subtitle: "Interaction should follow the reader’s attention",
        question: "什么样的拖动和缩放，能让人忘记工具的存在？",
        principle: "输入需要连续、可预期，并且尊重当前的阅读焦点。",
        vocabulary: ["中键拖拽", "滚轮缩放", "焦点", "连续输入"],
    },
    Topic {
        folder: "05-interaction",
        slug: "selection-anchors",
        title: "选择与文本锚点",
        subtitle: "One layout for pixels, selection, and annotations",
        question: "文件换行以后，批注还能指向原来的句子吗？",
        principle: "保存文本范围，通过同一份排版结果计算显示位置。",
        vocabulary: ["字符范围", "命中测试", "选区矩形", "文本身份"],
    },
    Topic {
        folder: "06-field-notes",
        slug: "performance-journal",
        title: "性能观察日志",
        subtitle: "Measure the pause, not just the average",
        question: "第一次看到一个文件时，为什么可能比再次看到它更慢？",
        principle: "把首次加载、持续移动与静止阅读分开测量。",
        vocabulary: ["冷缓存", "慢帧", "内存", "交互延迟"],
    },
    Topic {
        folder: "06-field-notes",
        slug: "design-notebook",
        title: "设计工作手记",
        subtitle: "Make room for unfinished ideas",
        question: "一个用来思考的工具，应该留下多少自由空间？",
        principle: "保持内容清晰、操作简单，让组织方式可以逐渐生长。",
        vocabulary: ["留白", "层次", "可读性", "渐进组织"],
    },
];

/// Materialize original sample files once. Existing files belong to the user.
pub fn ensure_demo_documents(dir: &Path, count: usize) -> Result<(), Box<dyn Error>> {
    if count == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "样例数量必须大于 0").into());
    }
    fs::create_dir_all(dir).map_err(|err| contextual(dir, "无法创建样例目录", err))?;
    for index in 0..count {
        let topic = &TOPICS[index % TOPICS.len()];
        let volume = index / TOPICS.len() + 1;
        let folder = dir.join(topic.folder);
        fs::create_dir_all(&folder).map_err(|err| contextual(&folder, "无法创建主题目录", err))?;
        let path = folder.join(format!("{:03}-{}-{:02}.md", index + 1, topic.slug, volume));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(sample_document(topic, index, volume).as_bytes())
                    .map_err(|err| contextual(&path, "无法写入样例文件", err))?;
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(contextual(&path, "无法创建样例文件", err).into()),
        }
    }
    Ok(())
}

/// Read Markdown files only; never follow links outside the requested tree.
pub fn load_documents(dir: &Path) -> Result<Vec<SourceDocument>, Box<dyn Error>> {
    if !dir.is_dir() && !dir.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Markdown 文件或目录不存在：{}", dir.display()),
        )
        .into());
    }
    let mut pending = if dir.is_dir() {
        vec![dir.to_owned()]
    } else {
        Vec::new()
    };
    let mut paths = if dir.is_file() {
        if !is_markdown(dir) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("请选择 .md 或 .markdown 文件：{}", dir.display()),
            )
            .into());
        }
        vec![dir.to_owned()]
    } else {
        Vec::new()
    };
    while let Some(folder) = pending.pop() {
        for entry in
            fs::read_dir(&folder).map_err(|err| contextual(&folder, "无法读取目录", err))?
        {
            let entry = entry.map_err(|err| contextual(&folder, "无法读取目录条目", err))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|err| contextual(&path, "无法读取文件类型", err))?;
            if kind.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            if kind.is_dir() {
                if !matches!(name.as_ref(), "target" | "node_modules" | "dist" | "build") {
                    pending.push(path);
                }
            } else if kind.is_file() && is_markdown(&path) {
                paths.push(path);
            }
        }
    }
    paths.sort();
    let mut total = 0usize;
    let mut documents = Vec::with_capacity(paths.len());
    for path in paths {
        let file =
            fs::File::open(&path).map_err(|err| contextual(&path, "无法打开 Markdown", err))?;
        let length = file
            .metadata()
            .map_err(|err| contextual(&path, "无法读取文件大小", err))?
            .len();
        if length > MAX_FILE_BYTES {
            return Err(size_error(&path).into());
        }
        let mut bytes = Vec::with_capacity(length as usize);
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| contextual(&path, "无法读取 Markdown", err))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(size_error(&path).into());
        }
        total = total.saturating_add(bytes.len());
        if total > MAX_TOTAL_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Markdown 总内容超过 128 MiB，请选择更小的子目录：{}",
                    dir.display()
                ),
            )
            .into());
        }
        let markdown = String::from_utf8(bytes).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "文件不是 UTF-8，请转换编码后重试：{}（{err}）",
                    path.display()
                ),
            )
        })?;
        let markdown = markdown
            .trim_start_matches('\u{feff}')
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let title = document_title(&markdown).unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        let line_count = markdown.lines().count();
        documents.push(SourceDocument {
            path,
            title,
            markdown,
            line_count,
        });
    }
    if documents.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("没有找到 .md 或 .markdown 文件：{}", dir.display()),
        )
        .into());
    }
    Ok(documents)
}

fn contextual(path: &Path, action: &str, err: io::Error) -> io::Error {
    io::Error::new(err.kind(), format!("{action}：{}（{err}）", path.display()))
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

fn size_error(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("单个 Markdown 超过 2 MiB，请拆分后重试：{}", path.display()),
    )
}

fn document_title(markdown: &str) -> Option<String> {
    let mut fence: Option<char> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker = trimmed.chars().next().unwrap();
            if fence == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker);
            }
            continue;
        }
        if fence.is_none()
            && let Some(title) = trimmed.strip_prefix("# ")
        {
            let title = title.trim().trim_end_matches('#').trim();
            if !title.is_empty() {
                return Some(title.to_owned());
            }
        }
    }
    None
}

fn sample_document(topic: &Topic, index: usize, volume: usize) -> String {
    let [first, second, third, fourth] = topic.vocabulary;
    let mut text = format!(
        "# {} · {:02}\n\n*{}*\n\n这是一份展开在画布上的独立 Markdown 文件。第 **{}** 组笔记记录了关于 **{}** 的思考，包含正文、代码、表格与实验记录。\n\n> {}\n>\n> {}\n\n## 从一个问题开始\n\n{}\n\n阅读不是单纯地从上到下扫描文字。我们会跳到另一个文件，回来看一段解释，然后沿着某个线索继续前进。画布为这些临时的联系提供了位置。\n\n这一页把 **{}** 放在中心，也保留 *{}* 的细节；`{}` 是观察时使用的标签，而不是已经完成的产品能力。\n\n### 本页关注\n\n- **{}**：先确认数据来自哪里，再决定如何展示。\n- **{}**：保持输入与结果之间的关系稳定。\n- **{}**：用一个小实验观察边界情况。\n  - 将内容放到屏幕边缘。\n  - 连续缩放，再回到原来的位置。\n- **{}**：记录结果，给下一轮实验留下依据。\n\n---\n\n",
        topic.title,
        volume,
        topic.subtitle,
        volume,
        topic.title,
        topic.question,
        topic.principle,
        topic.principle,
        first,
        second,
        topic.slug,
        first,
        second,
        third,
        fourth,
    );
    if index.is_multiple_of(3) {
        text.push_str("## 一段 Rust 草图\n\n下面的代码说明数据流，不依赖画布中的真实坐标。类型让约束明确，也让后续重构更容易。\n\n```rust\n#[derive(Clone, Copy, Debug)]\nstruct Viewport {\n    x: f64,\n    y: f64,\n    scale: f64,\n}\n\nimpl Viewport {\n    fn world_at(&self, screen: (f64, f64)) -> (f64, f64) {\n        ((screen.0 - self.x) / self.scale,\n         (screen.1 - self.y) / self.scale)\n    }\n\n    fn zoom_at(&mut self, pointer: (f64, f64), factor: f64) {\n        let world = self.world_at(pointer);\n        self.scale = (self.scale * factor).clamp(0.02, 8.0);\n        self.x = pointer.0 - world.0 * self.scale;\n        self.y = pointer.1 - world.1 * self.scale;\n    }\n}\n```\n\n在这个例子里，`world_at` 描述了屏幕到世界的转换。改变缩放以后，重新计算平移量，指针下的内容就能留在同一个屏幕位置。\n\n");
    } else if index % 3 == 1 {
        text.push_str("## 一段 TypeScript 草图\n\n将内容变化与相机变化分开，会让缓存的边界更清楚。这段代码只是用于解释的文档示例。\n\n```typescript\ntype DocumentKey = string;\n\ninterface LayoutEntry {\n  revision: number;\n  width: number;\n  height: number;\n  blocks: readonly TextBlock[];\n}\n\nconst layouts = new Map<DocumentKey, LayoutEntry>();\n\nfunction needsLayout(\n  previous: LayoutEntry | undefined,\n  revision: number,\n  width: number,\n): boolean {\n  return !previous\n    || previous.revision !== revision\n    || previous.width !== width;\n}\n\n// Camera movement does not change a document's width.\nconst reuse = !needsLayout(layouts.get('notes'), 7, 680);\nconsole.log({ reuse });\n```\n\n比较值应当来自文档自身。窗口移动或相机缩放不一定改变文档宽度，因此不必自动成为排版缓存的失效条件。\n\n");
    } else {
        text.push_str("## 一份配置草图\n\n可复现的实验应当记录输入、预期和资源预算。下面的 JSON 是说明性配置，不代表程序当前的设置接口。\n\n```json\n{\n  \"experiment\": \"spatial-reading\",\n  \"documents\": {\n    \"format\": \"markdown\",\n    \"encoding\": \"UTF-8\",\n    \"wrap\": true\n  },\n  \"camera\": {\n    \"zoomAnchor\": \"pointer\",\n    \"panButton\": \"middle\"\n  },\n  \"observations\": [\n    \"first appearance\",\n    \"continuous movement\",\n    \"return to reading scale\"\n  ],\n  \"notes\": \"Keep the layout stable.\"\n}\n```\n\n配置只是实验的起点。实际结论仍然来自运行记录，以及相同输入条件下的对照。\n\n");
    }
    let _ = writeln!(
        text,
        "## 观察矩阵\n\n| 观察对象 | 需要保持的性质 | 检查方式 |\n| :--- | :--- | :--- |\n| {first} | 位置稳定 | 缩放前后对照 |\n| {second} | 内容完整 | 阅读长段落 |\n| {third} | 关系清楚 | 查看邻近文档 |\n| {fourth} | 结果可复现 | 保存实验条件 |\n"
    );
    text.push_str("这里的表格没有虚构性能测量。它只列出观察目标。真正的帧耗时、内存占用和文字质量，需要在具体设备上检查。\n\n## 阅读与实验清单\n\n- [x] 把原始想法写成独立文档。\n- [x] 加入代码和足够长的说明文字。\n- [x] 使用中文与 English 混排检查字体回退。\n- [ ] 比较首次出现与再次出现时的停顿。\n- [ ] 记录一段连续缩放的帧耗时。\n- [ ] 检查超长文档在屏幕边缘的裁剪。\n\n### 一个值得保留的细节\n\n**强调**需要真正改变字重，*斜体*需要保留语气，~~被否定的旧想法~~仍然可以作为背景信息存在。行内的 `document.layout(width)` 则应当与普通叙述区分开。\n\n链接也属于阅读内容，例如 [Rust 官方文档](https://www.rust-lang.org/learn) 与 [Skia 项目](https://skia.org/)。这份 demo 以展示文字为主，链接的外观不等于已经提供浏览器行为。\n\n");
    text.push_str("## 对照记录的方法\n\n### 开始以前\n\n确认当前窗口大小与显示缩放比例。把需要比较的两个文件放到相近的位置，先记录它们的标题和内容长度。\n\n- 输入内容：同一份 Markdown。\n- 阅读尺度：正文可以自然阅读。\n- 移动方式：中键拖动，然后使用滚轮缩放。\n\n### 操作过程中\n\n观察文字首次出现、跨越远景细节阈值，以及返回原来位置这三个时刻。不要把它们合并成一个主观分数。\n\n- 是否发生明显停顿？\n- 是否出现缺字或字体变化？\n- 文档之间的相对位置是否保持？\n\n### 操作结束以后\n\n记下具体的触发步骤。如果问题只出现一次，可以先重新打开同一批文件，再重复相同操作。\n\n");
    let section_count = 2 + index % 3;
    for section in 0..section_count {
        let label = ["第一次观察", "改变一个条件", "回到上下文", "下一次实验"][section];
        let _ = writeln!(
            text,
            "## {} · {}\n\n{}。围绕这个原则，先只观察一个变量：**{}**。如果同时改变字体、宽度和内容，就很难知道差异来自哪里。\n\n在第 {} 轮记录中，我们把相关的文件放在附近，留出足够的空白。远处可以识别文档的位置，靠近后再阅读完整段落。布局本身不因为视野变化而重新组织。\n\n> 观察提示：不要只问“看起来快不快”，也要问“停顿发生在什么时候”。\n\n1. 确认当前内容与上一轮相同。\n2. 缓慢靠近，再连续缩远。\n3. 检查文字清晰度和相对位置。\n4. 记录异常，保持描述具体。\n",
            section + 1,
            label,
            topic.principle,
            topic.vocabulary[(section + volume) % 4],
            volume + section
        );
    }
    let _ = writeln!(
        text,
        "---\n\n### 留给下一页\n\n{} 不是一个孤立问题。把这份笔记与旁边的文档一起阅读，可能会发现新的联系。\n\n*文件编号 {:03} · 卷 {} · 原创示例内容，供渲染与阅读实验使用。*",
        topic.title,
        index + 1,
        volume
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "infinity-markdown-documents-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn samples_have_rich_content_and_existing_files_survive() {
        let dir = TempDir::new();
        ensure_demo_documents(&dir.0, 12).unwrap();
        let docs = load_documents(&dir.0).unwrap();
        assert_eq!(docs.len(), 12);
        assert!(docs.iter().all(|doc| doc.line_count >= 120));
        assert!(docs.iter().all(|doc| doc.markdown.contains("```")));
        assert!(docs.iter().all(|doc| doc.markdown.contains("| 观察对象 |")));
        fs::write(&docs[0].path, "# User changes\n\nKeep me.\n").unwrap();
        ensure_demo_documents(&dir.0, 12).unwrap();
        assert_eq!(load_documents(&dir.0).unwrap()[0].title, "User changes");
    }

    #[test]
    fn loading_is_sorted_recursive_and_skips_build_and_hidden_directories() {
        let dir = TempDir::new();
        for folder in ["nested", ".hidden", "node_modules", "target"] {
            fs::create_dir(dir.0.join(folder)).unwrap();
            fs::write(dir.0.join(folder).join("note.md"), "# Nested").unwrap();
        }
        fs::write(dir.0.join("z.MARKDOWN"), "\u{feff}# Last\r\n\r\nText").unwrap();
        fs::write(
            dir.0.join("a.md"),
            "```markdown\n# Not the title\n```\n# First\n",
        )
        .unwrap();
        fs::write(dir.0.join("not-markdown.txt"), "# Ignore").unwrap();
        let docs = load_documents(&dir.0).unwrap();
        assert_eq!(
            docs.iter().map(|d| d.title.as_str()).collect::<Vec<_>>(),
            ["First", "Nested", "Last"]
        );
        assert!(!docs[2].markdown.contains('\r'));
    }

    #[test]
    fn invalid_utf8_and_oversized_files_report_the_path() {
        let dir = TempDir::new();
        let path = dir.0.join("broken.md");
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        let error = load_documents(&dir.0).err().unwrap().to_string();
        assert!(error.contains("UTF-8") && error.contains("broken.md"));
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        let error = load_documents(&dir.0).err().unwrap().to_string();
        assert!(error.contains("2 MiB") && error.contains("broken.md"));
    }

    #[test]
    fn an_explicit_markdown_file_can_be_opened_by_itself() {
        let dir = TempDir::new();
        let path = dir.0.join("single.md");
        fs::write(&path, "# One file\n").unwrap();
        let docs = load_documents(&path).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].title, "One file");
        let unsupported = dir.0.join("plain.txt");
        fs::write(&unsupported, "hello").unwrap();
        assert!(load_documents(&unsupported).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_not_followed() {
        let dir = TempDir::new();
        let outside = TempDir::new();
        fs::write(outside.0.join("secret.md"), "# Outside").unwrap();
        fs::write(dir.0.join("real.md"), "# Inside").unwrap();
        std::os::unix::fs::symlink(&outside.0, dir.0.join("linked-directory")).unwrap();
        std::os::unix::fs::symlink(outside.0.join("secret.md"), dir.0.join("linked.md")).unwrap();
        let docs = load_documents(&dir.0).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].title, "Inside");
    }
}
