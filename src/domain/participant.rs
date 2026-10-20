use super::{Cents, Guest, GuestId, PlayerId};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ParticipantId {
    Roster(PlayerId),
    Guest(GuestId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Participant {
    Roster(PlayerId),
    Guest(Guest),
}

impl Participant {
    pub fn id(&self) -> ParticipantId {
        match self {
            Self::Roster(id) => ParticipantId::Roster(id.clone()),
            Self::Guest(guest) => ParticipantId::Guest(guest.id().clone()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmountEntry {
    participant_id: ParticipantId,
    amount: Cents,
}

impl AmountEntry {
    pub fn new(participant_id: ParticipantId, amount: Cents) -> Self {
        Self {
            participant_id,
            amount,
        }
    }

    pub fn participant_id(&self) -> &ParticipantId {
        &self.participant_id
    }

    pub fn amount(&self) -> Cents {
        self.amount
    }
}
