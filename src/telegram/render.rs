use super::{
    flow::{Choice, Flow, Stage},
    Error, Result,
};
use crate::domain::Cents;
use serde::{Deserialize, Serialize};
const MAX_CALLBACK_BYTES: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Reply {
    pub method: String,
    pub chat_id: i64,
    pub text: String,
    pub reply_markup: Keyboard,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Keyboard {
    pub inline_keyboard: Vec<Vec<Button>>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Button {
    pub text: String,
    pub callback_data: String,
}

impl Reply {
    pub fn text(chat: i64, text: String) -> Self {
        Self {
            method: "sendMessage".into(),
            chat_id: chat,
            text,
            reply_markup: Keyboard::default(),
        }
    }

    fn button(&mut self, flow: &Flow, label: String, action: &str) -> Result<()> {
        let callback_data = format!("{}:{}:{action}", flow.id, flow.revision);
        if callback_data.len() > MAX_CALLBACK_BYTES {
            return Err(Error::Invalid);
        }
        self.reply_markup.inline_keyboard.push(vec![Button {
            text: label,
            callback_data,
        }]);
        Ok(())
    }
}

pub(super) fn render(flow: &Flow) -> Result<Reply> {
    let mut reply = Reply::text(flow.chat, format!("Session {}\n", flow.id));
    match flow.stage {
        Stage::Game => games(&mut reply, flow)?,
        Stage::Date => {
            reply.text += &format!(
                "Enter the played date in Asia/Singapore:\n/step {} {} YYYY-MM-DD",
                flow.id, flow.revision
            )
        }
        Stage::Participants => participants(&mut reply, flow)?,
        Stage::Amounts => amounts(&mut reply, flow)?,
        Stage::Review => review(&mut reply, flow)?,
        Stage::Confirmed => reply.text += "Saved. Start another with /session.",
    }
    Ok(reply)
}

fn games(reply: &mut Reply, flow: &Flow) -> Result<()> {
    reply.text += "Choose a game:";
    for (index, game) in flow.games.iter().enumerate() {
        reply.button(flow, game.name.clone(), &format!("g:{index}"))?;
    }
    Ok(())
}

fn participants(reply: &mut Reply, flow: &Flow) -> Result<()> {
    reply.text += "Select roster players (tap again to remove):";
    for (index, player) in flow.roster.iter().enumerate() {
        reply.button(flow, roster_label(flow, player), &format!("p:{index}"))?;
    }
    for selection in &flow.selected {
        reply.text += &format!("\nSelected: {}", selection.label());
    }
    reply.text += &format!(
        "\nAdd a one-time guest:\n/guest {} {} Guest Name",
        flow.id, flow.revision
    );
    reply.button(flow, "Enter amounts".into(), "done")
}

fn roster_label(flow: &Flow, player: &Choice) -> String {
    let selected = flow
        .selected
        .iter()
        .any(|p| !p.guest && p.choice.id == player.id);
    let marker = if selected { "✓" } else { "+" };
    format!("{marker} {} [{}]", player.name, player.id)
}

fn amounts(reply: &mut Reply, flow: &Flow) -> Result<()> {
    let next = flow
        .selected
        .iter()
        .find(|p| p.cents.is_none())
        .ok_or(Error::Invalid)?;
    reply.text += &format!(
        "Enter signed SGD for {} (for example +12.50 or -12.50):\n/step {} {} +0.00",
        next.label(),
        flow.id,
        flow.revision
    );
    Ok(())
}

fn review(reply: &mut Reply, flow: &Flow) -> Result<()> {
    let game = &flow.game.as_ref().ok_or(Error::Invalid)?.name;
    let date = flow.date.as_deref().ok_or(Error::Invalid)?;
    reply.text += &format!("Review: {game}\nPlayed: {date} (Asia/Singapore)");
    for participant in &flow.selected {
        reply.text += &format!(
            "\n{}: {}",
            participant.label(),
            Cents::new(participant.cents.ok_or(Error::Invalid)?)
        );
    }
    match flow.total() {
        Ok(total) => review_total(reply, flow, total)?,
        Err(_) => reply.text += "\nTotal exceeds the supported cent range. Re-enter amounts.",
    }
    reply.button(flow, "Re-enter amounts".into(), "edit")
}

fn review_total(reply: &mut Reply, flow: &Flow, total: Cents) -> Result<()> {
    reply.text += &format!("\nTotal: {total}");
    if total == Cents::ZERO {
        return reply.button(flow, "Confirm session".into(), "confirm");
    }
    reply.text += "\nAmounts must total S$0.00 before confirmation.";
    Ok(())
}
