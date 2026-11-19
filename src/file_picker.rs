use std::path::PathBuf;

use objc2::{MainThreadMarker, rc::autoreleasepool};
use objc2_app_kit::NSOpenPanel;
use objc2_foundation::NSString;

/// AppKit's native panel runs on the same main thread as the winit window.
pub fn choose_path() -> Option<PathBuf> {
    let mtm = MainThreadMarker::new()?;
    autoreleasepool(|_| {
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(true);
        panel.setAllowsMultipleSelection(false);
        panel.setTitle(Some(&NSString::from_str("导入文件夹或文本文件")));
        panel.setPrompt(Some(&NSString::from_str("导入")));
        panel.setMessage(Some(&NSString::from_str(
            "将所选文件夹展开为嵌套画布，替换当前内容。",
        )));
        if panel.runModal() != 1 {
            return None;
        }
        panel
            .URL()?
            .path()
            .map(|path| PathBuf::from(path.to_string()))
    })
}
