use crate::request::Request;
use crate::search::QueryRunner;

pub fn get_profile(_runner: &dyn QueryRunner, _request: &Request) -> String {
    "ok".to_string()
}
