def lookup_user(name, db):
    query = f"SELECT * FROM users WHERE name = '{name}'"
    return db.execute(query).fetchone()
