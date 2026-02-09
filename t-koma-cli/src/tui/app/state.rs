use t_koma_core::KnowledgeResultInfo;
use t_koma_db::{Ghost, JobLog, JobLogSummary, SessionInfo};

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
    SetOperatorAccessLevel,
    SetOperatorRateLimits,
    KnowledgeSearch,
}

#[derive(Debug, Default)]
pub(super) struct PromptState {
    pub(super) kind: Option<PromptKind>,
    pub(super) buffer: String,
    pub(super) target_ghost: Option<String>,
    pub(super) target_operator_id: Option<String>,
}

/// Current content sub-view for drill-down navigation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) enum ContentView {
    #[default]
    List,
    GhostSessions {
        ghost_id: String,
        ghost_name: String,
    },
    SessionMessages {
        ghost_name: String,
        session_id: String,
    },
    JobDetail {
        job_id: String,
    },
    KnowledgeDetail {
        note_id: String,
    },
}

/// Selection modal for choosing from a list (e.g. access level).
#[derive(Debug, Clone)]
pub(super) struct SelectionModal {
    pub(super) title: String,
    pub(super) items: Vec<SelectionItem>,
    pub(super) selected_idx: usize,
    pub(super) on_select: SelectionAction,
    pub(super) context: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct SelectionItem {
    pub(super) label: String,
    pub(super) value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectionAction {
    SetAccessLevel,
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

#[derive(Debug, Clone)]
pub(super) struct GhostRow {
    pub(super) ghost: Ghost,
    pub(super) heartbeat: Option<String>,
}

/// View state for the job viewer.
#[derive(Debug, Default)]
pub(super) struct JobViewState {
    pub(super) summaries: Vec<JobLogSummary>,
    pub(super) detail: Option<JobLog>,
}

/// View state for session drill-down.
#[derive(Debug, Default)]
pub(super) struct SessionViewState {
    pub(super) sessions: Vec<SessionInfo>,
    pub(super) messages: Vec<t_koma_db::Message>,
    pub(super) scroll: u16,
}

/// View state for the knowledge browser.
#[derive(Debug, Default)]
pub(super) struct KnowledgeViewState {
    pub(super) notes: Vec<KnowledgeResultInfo>,
    pub(super) detail_title: Option<String>,
    pub(super) detail_body: Option<String>,
    pub(super) scroll: u16,
}
