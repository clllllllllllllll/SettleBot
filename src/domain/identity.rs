use super::DomainError;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Identifier(String);

impl Identifier {
    fn new(value: &str, field: &'static str) -> Result<Self, DomainError> {
        if value.trim().is_empty() {
            return Err(DomainError::RequiredValue(field));
        }
        if !value.bytes().all(is_identifier_byte) {
            return Err(DomainError::InvalidIdentifier(field));
        }
        Ok(Self(value.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_identifier_byte(value: u8) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_')
}

macro_rules! identifier_type {
    ($name:ident, $field:literal) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub struct $name(Identifier);

        impl $name {
            pub fn new(value: &str) -> Result<Self, DomainError> {
                Identifier::new(value, $field).map(Self)
            }

            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }
    };
}

identifier_type!(PlayerId, "player_id");
identifier_type!(GameId, "game_id");
identifier_type!(SessionId, "session_id");

/// A guest's local identifier is meaningful only within its owning session.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GuestId {
    session_id: SessionId,
    local_id: Identifier,
}

impl GuestId {
    pub fn new(session_id: SessionId, local_id: &str) -> Result<Self, DomainError> {
        Ok(Self {
            session_id,
            local_id: Identifier::new(local_id, "guest_id")?,
        })
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn as_str(&self) -> &str {
        self.local_id.as_str()
    }
}
