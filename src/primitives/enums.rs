use crate::primitives::TryLockError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShouldBlock {
    Ok,
    Block,
}

impl ShouldBlock {
    pub fn to_result<E>(self) -> Result<(), TryLockError<E>> {
        match self {
            Self::Ok => Ok(()),
            Self::Block => Err(TryLockError::WouldBlock),
        }
    }
}
