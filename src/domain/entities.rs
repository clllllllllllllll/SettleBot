use super::{DomainError, GameId, GuestId, PlayerId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayName(String);

impl DisplayName {
    pub fn new(value: &str) -> Result<Self, DomainError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(DomainError::RequiredValue("display_name"));
        }
        if value.chars().any(char::is_control) {
            return Err(DomainError::InvalidDisplayName);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RosterStatus {
    Active,
    Inactive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Player {
    id: PlayerId,
    name: DisplayName,
    status: RosterStatus,
}

impl Player {
    pub fn new(id: PlayerId, name: DisplayName, status: RosterStatus) -> Self {
        Self { id, name, status }
    }

    pub fn id(&self) -> &PlayerId {
        &self.id
    }

    pub fn name(&self) -> &DisplayName {
        &self.name
    }

    pub fn status(&self) -> RosterStatus {
        self.status
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Game {
    id: GameId,
    name: DisplayName,
}

impl Game {
    pub fn new(id: GameId, name: DisplayName) -> Self {
        Self { id, name }
    }

    pub fn id(&self) -> &GameId {
        &self.id
    }

    pub fn name(&self) -> &DisplayName {
        &self.name
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Guest {
    id: GuestId,
    name: DisplayName,
}

impl Guest {
    pub fn new(id: GuestId, name: DisplayName) -> Self {
        Self { id, name }
    }

    pub fn id(&self) -> &GuestId {
        &self.id
    }

    pub fn name(&self) -> &DisplayName {
        &self.name
    }
}
