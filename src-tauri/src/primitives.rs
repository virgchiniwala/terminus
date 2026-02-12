use crate::schema::PrimitiveId;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PrimitiveGuardError {
    #[error("This action isn't allowed in Terminus yet.")]
    NotAllowed,
}

#[derive(Debug, Clone)]
pub struct PrimitiveGuard {
    allowlist: Vec<PrimitiveId>,
}

impl PrimitiveGuard {
    pub fn new(allowlist: Vec<PrimitiveId>) -> Self {
        Self { allowlist }
    }

    pub fn validate(&self, primitive: PrimitiveId) -> Result<(), PrimitiveGuardError> {
        if self.allowlist.contains(&primitive) {
            Ok(())
        } else {
            Err(PrimitiveGuardError::NotAllowed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PrimitiveGuard, PrimitiveGuardError};
    use crate::schema::PrimitiveId;

    #[test]
    fn allows_known_primitive_when_in_allowlist() {
        let guard = PrimitiveGuard::new(vec![PrimitiveId::ReadWeb, PrimitiveId::WriteEmailDraft]);
        assert_eq!(guard.validate(PrimitiveId::ReadWeb), Ok(()));
    }

    #[test]
    fn denies_primitive_not_in_allowlist_with_human_message() {
        let guard = PrimitiveGuard::new(vec![PrimitiveId::ReadWeb]);
        let result = guard.validate(PrimitiveId::SendEmail);
        assert_eq!(result, Err(PrimitiveGuardError::NotAllowed));
        assert_eq!(
            result.expect_err("expected deny").to_string(),
            "This action isn't allowed in Terminus yet."
        );
    }
}
