pub struct Request {
    name: String,
}

impl Request {
    pub fn name(&self) -> &str {
        &self.name
    }
}
