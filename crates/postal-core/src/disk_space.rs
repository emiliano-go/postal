use crate::message_ref::MessageRef;
use anyhow::{Result, ensure};
use std::{io, path::Path};

const RESERVE_BYTES: u64 = 32 * 1024 * 1024;

pub fn available(path: &Path) -> io::Result<u64> {
    let existing = path.ancestors().find(|ancestor| ancestor.exists())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no existing disk path"))?;
    fs4::available_space(existing)
}

pub fn check(available: u64, needed: u64) -> Result<()> {
    let required = needed.checked_add(RESERVE_BYTES);
    ensure!(required.is_some_and(|required| available >= required), MessageRef::new("error.upload_disk_space")
        .with_param("required_bytes", serde_json::Number::from(required.unwrap_or(u64::MAX)))
        .with_param("available_bytes", serde_json::Number::from(available)));
    Ok(())
}

pub fn encrypted_len(size: u64) -> u64 {
    (size / 16).saturating_add(1).saturating_mul(16).saturating_add(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_space_is_typed_and_encryption_size_includes_padding_and_mac() {
        assert_eq!(encrypted_len(0), 26);
        assert_eq!(encrypted_len(16), 42);
        let error = check(RESERVE_BYTES + 25, encrypted_len(0)).unwrap_err();
        let message = error.downcast_ref::<MessageRef>().unwrap();
        assert_eq!(message.code, "error.upload_disk_space");
        let params = serde_json::to_value(&message.params).unwrap();
        assert_eq!(params["required_bytes"], RESERVE_BYTES + 26);
        assert_eq!(params["available_bytes"], RESERVE_BYTES + 25);
        assert!(check(u64::MAX, u64::MAX).is_err());
    }
}
