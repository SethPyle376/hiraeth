# Hiraeth

## Development

### SQLX Compile Time Checking
- Prefer `sqlx::query!` and `sqlx::query_as!` macros for compile-time checking of SQL queries.
- Use `cargo run -p xtask -- prepare-db` to prepare the dev db for compile-time checking.
- Use `DATABASE_URL=sqlite://.local/db.sqlite cargo sqlx prepare --workspace` to prepare the SQLX macros for compile-time checking.
