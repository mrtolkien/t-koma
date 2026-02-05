#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OperatorView {
    All,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptKind {
    AddOperator,
    AddModel,
    SetDefaultModel,
    NewGhost,
    DeleteGhostConfirmOne,
    DeleteGhostConfirmTwo,
    GateSearch,
}

#[derive(Debug, Default)]
pub(super) struct PromptState {
    pub(super) kind: Option<PromptKind>,
    pub(super) buffer: String,
    pub(super) target_ghost: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct GateRow {
    pub(super) time: String,
    pub(super) level: String,
    pub(super) source: String,
    pub(super) core: String,
    pub(super) message: String,
}

#[derive(Debug)]
pub(super) enum GateEvent {
    Status(bool),
    Log(GateRow),
}

#[derive(Debug, Default, Clone)]
pub(super) struct Metrics {
    pub(super) operator_count: usize,
    pub(super) ghost_count: usize,
    pub(super) recent_message_count: i64,
}
