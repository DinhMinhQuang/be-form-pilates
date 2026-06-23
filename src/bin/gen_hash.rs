use argon2::{Argon2, PasswordHasher, password_hash::{SaltString, rand_core::OsRng}};

fn hash(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .unwrap()
        .to_string()
}

fn main() {
    println!("admin123: {}", hash("admin123"));
    println!("trainer123: {}", hash("trainer123"));
}
