pub trait QueryRunner {
    fn find_user(&self, name: &str) -> String;
}
