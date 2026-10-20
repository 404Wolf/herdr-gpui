//! What clients ask for.

use acp::v1 as wire;
use herdr_chat_protocol::{
    Change, Command, CommandId, MAX_PROMPT_SIZE, OptionValue, PermissionId, Queued, Rejection,
    Status, Turn,
};

use super::{Activity, ConnId, Pending, Session};

impl<R: FnMut(Activity)> Session<R> {
    pub(super) fn command(&mut self, conn: ConnId, id: CommandId, command: Command) {
        if let Some((_, rejected)) = self.handled.iter().find(|(handled, _)| *handled == id) {
            let rejected = *rejected;
            return self.send_to(
                conn,
                herdr_chat_protocol::ServerMessage::Ack { id, rejected },
            );
        }
        let rejected = match command {
            Command::Prompt { text } => self.prompt(id, text),
            Command::Cancel => self.cancel(),
            Command::Answer { option, permission } => self.answer(permission, &option),
            Command::SetMode { id } => self.set_mode(id),
            Command::SetOption { id, value } => self.set_option(id, &value),
            Command::Unqueue { id } => self.unqueue(id),
        }
        .err();
        self.ack(conn, id, rejected);
    }

    fn prompt(&mut self, id: CommandId, text: String) -> Result<(), Rejection> {
        if text.trim().is_empty() {
            return Err(Rejection::EmptyPrompt);
        }
        if text.len() > MAX_PROMPT_SIZE {
            return Err(Rejection::PromptTooLong);
        }
        if !self.transcript.status.accepts_prompts() {
            return Err(Rejection::NotReady);
        }
        if self.turn.is_some() || !self.ready {
            let mut queued = self.transcript.queued.clone();
            queued.push(Queued { id, text });
            self.queue_changed(queued);
            return Ok(());
        }
        self.send_prompt(&text);
        Ok(())
    }

    fn send_prompt(&mut self, text: &str) {
        let Some(session) = self.acp_session.clone() else {
            return;
        };
        let prompt = self.projector.prompt(text);
        self.emit(prompt);
        let request = wire::PromptRequest::new(
            session,
            vec![wire::ContentBlock::Text(wire::TextContent::new(text))],
        );
        if self.request("session/prompt", &request, Pending::Prompt) {
            self.turn = Some(self.next_request);
            self.emit(Change::Turn {
                turn: Some(Turn {
                    started_ms: super::now_ms(),
                }),
            });
            self.status(Status::Working);
        }
    }

    /// Sends the oldest queued prompt once the agent is free.
    pub(super) fn next_queued(&mut self) {
        if self.turn.is_some() || !self.ready {
            return;
        }
        let mut queued = self.transcript.queued.clone();
        if queued.is_empty() {
            return;
        }
        let next = queued.remove(0);
        self.queue_changed(queued);
        self.send_prompt(&next.text);
    }

    fn cancel(&mut self) -> Result<(), Rejection> {
        if self.turn.is_none() && self.transcript.queued.is_empty() {
            return Err(Rejection::NotWorking);
        }
        self.clear_queue();
        self.resolve_all();
        if self.turn.is_some()
            && let Some(session) = self.acp_session.clone()
            && let Err(error) = self
                .outbox
                .notify("session/cancel", &wire::CancelNotification::new(session))
        {
            self.fail(&error.to_string());
            return Err(Rejection::AgentUnavailable);
        }
        Ok(())
    }

    fn answer(&mut self, permission: PermissionId, option: &str) -> Result<(), Rejection> {
        let asked = self
            .asked
            .get(&permission)
            .ok_or(Rejection::UnknownPermission)?;
        if !asked.options.iter().any(|known| known == option) {
            return Err(Rejection::UnknownOption);
        }
        self.resolve(permission, Some(option));
        Ok(())
    }

    fn set_mode(&mut self, mode: String) -> Result<(), Rejection> {
        let session = self.acp_session.clone().ok_or(Rejection::NotReady)?;
        let known = self
            .transcript
            .modes
            .as_ref()
            .is_some_and(|modes| modes.available.iter().any(|known| known.id == mode));
        if !known {
            return Err(Rejection::UnknownMode);
        }
        let request = wire::SetSessionModeRequest::new(session, mode.clone());
        if !self.request("session/set_mode", &request, Pending::SetMode(mode)) {
            return Err(Rejection::AgentUnavailable);
        }
        Ok(())
    }

    fn set_option(&mut self, id: String, value: &OptionValue) -> Result<(), Rejection> {
        let session = self.acp_session.clone().ok_or(Rejection::NotReady)?;
        let known = self
            .transcript
            .options
            .iter()
            .any(|option| option.id == id && option.accepts(value));
        if !known {
            return Err(Rejection::UnknownSetting);
        }
        let request = wire::SetSessionConfigOptionRequest::new(
            session,
            id,
            crate::project::option_value(value),
        );
        if !self.request("session/set_config_option", &request, Pending::SetOption) {
            return Err(Rejection::AgentUnavailable);
        }
        Ok(())
    }

    fn unqueue(&mut self, id: CommandId) -> Result<(), Rejection> {
        let mut queued = self.transcript.queued.clone();
        let index = queued
            .iter()
            .position(|queued| queued.id == id)
            .ok_or(Rejection::UnknownQueued)?;
        queued.remove(index);
        self.emit(Change::Queue { queued });
        Ok(())
    }
}
