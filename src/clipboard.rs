use std::error::Error;

use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

pub fn copy_text(text: &str) -> Result<(), Box<dyn Error>> {
    autoreleasepool(|_| {
        let text = NSString::from_str(text);
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        // AppKit exports an immutable, process-lifetime string for this type.
        let plain_text_type = unsafe { NSPasteboardTypeString };
        if pasteboard.setString_forType(&text, plain_text_type) {
            Ok(())
        } else {
            Err(
                std::io::Error::other("macOS could not copy the selected text to the clipboard")
                    .into(),
            )
        }
    })
}
