use tracing::error;

use crate::content;
use t_koma_core::{GatewayMessage, GatewayMessageKind};

pub fn from_content(id: &str, interface: Option<&str>, vars: &[(&str, &str)]) -> GatewayMessage {
    match content::gateway_message(id, interface, vars) {
        Ok(message) => message,
        Err(err) => {
            error!("Message render failed for {}: {}", id, err);
            GatewayMessage::text_only(
                id,
                GatewayMessageKind::Error,
                format!("[missing message: {}]", id),
            )
        }
    }
}

pub fn text(kind: GatewayMessageKind, text: impl Into<String>) -> GatewayMessage {
    let id = format!("gw_{}", uuid::Uuid::new_v4());
    GatewayMessage::text_only(id, kind, text)
}
