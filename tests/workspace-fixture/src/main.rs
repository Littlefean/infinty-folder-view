mod reader;

fn main() {
    // 中文与 emoji 也应该能选择、复制：你好 🌍
    let documents = ["README.md", "src/main.rs"];
    for path in documents {
        println!("Reading: {path}");
    }
}
