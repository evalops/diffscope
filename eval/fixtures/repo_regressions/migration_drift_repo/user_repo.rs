pub fn insert_review(db: &Db, review: &Review) {
    db.execute(
        "insert into review_runs (id, status) values ($1, $2)",
        &[&review.id, &review.status],
    );
}
