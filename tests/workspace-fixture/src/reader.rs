pub struct Document {
    pub path: String,
    pub content: String,
}

impl Document {
    pub fn line_count(&self) -> usize {
        self.content.lines().count()
    }
}
