use tracing::{Event, Subscriber};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

use crate::state::{LogEntry, emit_global_log};

pub struct GatewayLogBridge;

impl<S> Layer<S> for GatewayLogBridge
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let metadata = event.metadata();
        let level = metadata.level().to_string();
        let target = metadata.target().to_string();
        let message = if visitor.message.is_empty() {
            String::new()
        } else {
            visitor.message
        };

        // Suppress trace events that are explicitly tagged as chat I/O because
        // those are emitted separately as structured `OperatorMessage` /
        // `GhostMessage` log entries.
        if visitor.event_kind.as_deref() == Some("chat_io") {
            return;
        }

        emit_global_log(LogEntry::Trace {
            level,
            target,
            message,
        });
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    event_kind: Option<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value).trim_matches('"').to_string();
        }
        if field.name() == "event_kind" {
            self.event_kind = Some(format!("{:?}", value).trim_matches('"').to_string());
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
        if field.name() == "event_kind" {
            self.event_kind = Some(value.to_string());
        }
    }
}
