use crate::build::compute_source_fingerprint;
use crate::snapshot::SourceFingerprint;

pub fn fingerprint_is_fresh(saved: &SourceFingerprint, source_path: &str) -> Result<bool, String> {
    let current = compute_source_fingerprint(source_path)?;
    Ok(saved == &current)
}
