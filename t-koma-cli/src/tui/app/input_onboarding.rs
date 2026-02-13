use crossterm::event::{KeyCode, KeyEvent};

use super::{
    TuiApp,
    onboarding::{OnboardingState, OnboardingStep},
};

impl TuiApp {
    pub(super) async fn handle_onboarding_key(&mut self, key: KeyEvent) {
        let Some(ob) = &self.onboarding else {
            return;
        };

        match key.code {
            KeyCode::Esc => {
                self.onboarding = None;
                self.status = "Setup cancelled".to_string();
            }
            KeyCode::Backspace => match ob.step {
                OnboardingStep::Welcome => {}
                OnboardingStep::EnterApiKey
                | OnboardingStep::ConfigureModel
                | OnboardingStep::EmbeddingsApiKey => {
                    let ob = self.onboarding.as_mut().unwrap();
                    if ob.input_buffer.is_empty() {
                        ob.go_back();
                    } else {
                        ob.input_buffer.pop();
                    }
                }
                _ => {
                    self.onboarding.as_mut().unwrap().go_back();
                }
            },
            KeyCode::Up | KeyCode::Char('k') => {
                handle_selection_up(&mut self.onboarding);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                handle_selection_down(&mut self.onboarding);
            }
            KeyCode::Char(c) => {
                handle_char_input(&mut self.onboarding, c);
            }
            KeyCode::Enter => {
                self.confirm_onboarding_step().await;
            }
            _ => {}
        }
    }

    async fn confirm_onboarding_step(&mut self) {
        let Some(ob) = &self.onboarding else {
            return;
        };

        match ob.step.clone() {
            OnboardingStep::Welcome => {
                self.onboarding.as_mut().unwrap().advance();
            }
            OnboardingStep::ChooseProvider => {
                let choices = OnboardingState::provider_choices();
                let ob = self.onboarding.as_mut().unwrap();
                if let Some((_, provider)) = choices.get(ob.selection_idx) {
                    ob.provider = Some(*provider);
                    ob.advance();
                }
            }
            OnboardingStep::EnterApiKey => {
                let ob = self.onboarding.as_mut().unwrap();
                let key = ob.input_buffer.trim().to_string();
                if key.is_empty() {
                    self.status = "API key cannot be empty".to_string();
                    return;
                }
                ob.api_key = Some(key);
                ob.advance();
            }
            OnboardingStep::ConfigureModel => {
                let ob = self.onboarding.as_mut().unwrap();
                let input = ob.input_buffer.trim().to_string();
                if !input.is_empty() {
                    if let Some((alias, model)) = input.split_once(',') {
                        ob.model_alias = alias.trim().to_string();
                        ob.model_id = model.trim().to_string();
                    } else {
                        self.status = "Format: alias,model-id".to_string();
                        return;
                    }
                }
                ob.advance();
            }
            OnboardingStep::EmbeddingsChoice => {
                let choices = OnboardingState::embedding_choices();
                let ob = self.onboarding.as_mut().unwrap();
                if let Some((_, choice)) = choices.get(ob.selection_idx) {
                    ob.embedding_provider = *choice;
                    ob.advance();
                }
            }
            OnboardingStep::EmbeddingsApiKey => {
                let ob = self.onboarding.as_mut().unwrap();
                let key = ob.input_buffer.trim().to_string();
                if key.is_empty() {
                    self.status = "API key cannot be empty".to_string();
                    return;
                }
                ob.openai_embedding_key = Some(key);
                ob.advance();
            }
            OnboardingStep::DiscordChoice => {
                let ob = self.onboarding.as_mut().unwrap();
                ob.discord_enabled = ob.selection_idx == 0;
                ob.advance();
            }
            OnboardingStep::Summary => {
                self.apply_onboarding().await;
            }
        }
    }

    async fn apply_onboarding(&mut self) {
        let ob = self.onboarding.take().unwrap();
        match ob.apply(&mut self.settings) {
            Ok(msg) => {
                self.status = msg;
                self.refresh_settings_toml();
                self.settings_dirty = false;
                self.disk_toml = self.settings_toml.clone();
            }
            Err(err) => {
                self.status = format!("Setup failed: {err}");
            }
        }
    }
}

fn handle_selection_up(onboarding: &mut Option<OnboardingState>) {
    let Some(ob) = onboarding else { return };
    match ob.step {
        OnboardingStep::ChooseProvider
        | OnboardingStep::EmbeddingsChoice
        | OnboardingStep::DiscordChoice => {
            if ob.selection_idx > 0 {
                ob.selection_idx -= 1;
            }
        }
        _ => {}
    }
}

fn handle_selection_down(onboarding: &mut Option<OnboardingState>) {
    let Some(ob) = onboarding else { return };
    let max = match ob.step {
        OnboardingStep::ChooseProvider => OnboardingState::provider_choices().len(),
        OnboardingStep::EmbeddingsChoice => OnboardingState::embedding_choices().len(),
        OnboardingStep::DiscordChoice => 2,
        _ => return,
    };
    if ob.selection_idx + 1 < max {
        ob.selection_idx += 1;
    }
}

fn handle_char_input(onboarding: &mut Option<OnboardingState>, c: char) {
    let Some(ob) = onboarding else { return };
    match ob.step {
        OnboardingStep::EnterApiKey
        | OnboardingStep::ConfigureModel
        | OnboardingStep::EmbeddingsApiKey => {
            ob.input_buffer.push(c);
        }
        _ => {}
    }
}
