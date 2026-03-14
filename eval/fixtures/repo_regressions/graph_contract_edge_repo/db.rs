use crate::search::QueryRunner;

pub struct PostgresQueryRunner;

impl QueryRunner for PostgresQueryRunner {
    fn find_user(&self, name: &str) -> String {
        format!("SELECT * FROM users WHERE name = '{}'", name)
    }
}
