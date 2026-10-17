// `sqlx::migrate!` reads the migrations directory when it expands, but Cargo does not know it depends on it: a new migration file would not rebuild
// the crate, and an incremental build could ship without the table the code then asks for.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
