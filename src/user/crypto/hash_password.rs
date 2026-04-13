use hmac::{Hmac, Mac};
use sha2::Sha256;
use bcrypt::{hash, verify, DEFAULT_COST};

type HmacSha256 = Hmac<Sha256>;

fn compute_hmac_pepper(password: &str, pepper: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(pepper.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(password.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

pub fn hash_password(password: &str, pepper: &str) -> Result<String, bcrypt::BcryptError> {
    // 1. HMAC the password with the pepper to collapse it and "key" it
    let peppered_password = compute_hmac_pepper(password, pepper);
    
    // 2. Hash the resulting HMAC bytes with bcrypt (hex encoded)
    let input = hex::encode(peppered_password);
    hash(input, DEFAULT_COST)
}

pub fn verify_password(password: &str, hashed_password: &str, pepper: &str) -> Result<bool, bcrypt::BcryptError> {
    let peppered_password = compute_hmac_pepper(password, pepper);
    let input = hex::encode(peppered_password);
    verify(input, hashed_password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify_with_pepper() {
        let password = "my_secure_password";
        let pepper = "secret_pepper_key";
        let hashed = hash_password(password, pepper).expect("Failed to hash password");
        
        assert!(verify_password(password, &hashed, pepper).expect("Failed to verify password"));
        assert!(!verify_password(password, &hashed, "wrong_pepper").expect("Verify should fail with wrong pepper"));
        assert!(!verify_password("wrong_password", &hashed, pepper).expect("Verify should fail with wrong password"));
    }
}
