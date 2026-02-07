use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    Heartbeat,
}

#[derive(Debug, Clone, Copy)]
pub struct JobSchedule {
    pub next_due: i64,
}

#[derive(Debug, Default)]
pub struct SchedulerState {
    schedules: HashMap<(JobKind, String), JobSchedule>,
}

impl SchedulerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_due(&mut self, kind: JobKind, key: &str, next_due: Option<i64>) {
        if let Some(ts) = next_due {
            self.schedules.insert((kind, key.to_string()), JobSchedule { next_due: ts });
        } else {
            self.schedules.remove(&(kind, key.to_string()));
        }
    }

    pub fn get_due(&self, kind: JobKind, key: &str) -> Option<i64> {
        self.schedules
            .get(&(kind, key.to_string()))
            .map(|entry| entry.next_due)
    }

    pub fn clear(&mut self, kind: JobKind, key: &str) {
        self.schedules.remove(&(kind, key.to_string()));
    }
}
