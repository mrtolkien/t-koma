use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Timelike, Utc};
use cron::Schedule;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::time::{Instant, interval_at};
use tracing::{info, warn};

use crate::scheduler::JobKind;
use crate::state::{AppState, LogEntry};
use crate::tools::ToolManager;
use t_koma_core::{CronPreToolCall, ParsedCronJobFile, parse_cron_job_markdown};
use t_koma_db::{
    ContentBlock, Ghost, GhostRepository, JobKind as DbJobKind, JobLog, JobLogRepository,
    MessageRole, Session, SessionRepository,
};

#[derive(Clone)]
struct CronScheduledJob {
    key: String,
    name: String,
    schedule: Schedule,
    schedule_raw: String,
    prompt: String,
    pre_tools: Vec<CronPreToolCall>,
    carry_last_output: bool,
    model_aliases_json: Option<String>,
    ghost: Ghost,
    state_file: PathBuf,
}

struct CronRuntime {
    watcher: RecommendedWatcher,
    watched_dirs: HashSet<PathBuf>,
    jobs: HashMap<String, CronScheduledJob>,
}

fn utc_minute(now: DateTime<Utc>) -> DateTime<Utc> {
    now.with_second(0)
        .and_then(|dt| dt.with_nanosecond(0))
        .unwrap_or(now)
}

fn parse_schedule(expr: &str) -> Result<Schedule, String> {
    Schedule::from_str(&format!("0 {}", expr.trim())).map_err(|e| e.to_string())
}

fn next_due_at_or_after(schedule: &Schedule, now: DateTime<Utc>) -> Option<i64> {
    let now_min = utc_minute(now);
    let prev = now_min - chrono::Duration::minutes(1);
    schedule
        .after(&prev)
        .next()
        .map(|dt| utc_minute(dt).timestamp())
}

fn next_due_after(schedule: &Schedule, base_unix: i64) -> Option<i64> {
    let base = DateTime::from_timestamp(base_unix, 0)?;
    schedule
        .after(&base)
        .next()
        .map(|dt| utc_minute(dt).timestamp())
}

fn sanitize_key(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn cron_state_file(workspace_root: &Path, key: &str) -> PathBuf {
    workspace_root
        .join("cron")
        .join(".state")
        .join(format!("{}.last.md", sanitize_key(key)))
}

fn build_job_from_file(
    ghost: &Ghost,
    workspace_root: &Path,
    file_path: &Path,
    parsed: ParsedCronJobFile,
) -> Result<CronScheduledJob, String> {
    let schedule = parse_schedule(&parsed.schedule)?;
    let rel = file_path
        .strip_prefix(workspace_root)
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| file_path.display().to_string());
    let key = format!("{}:{}", ghost.id, rel);
    Ok(CronScheduledJob {
        key: key.clone(),
        name: parsed.name,
        schedule,
        schedule_raw: parsed.schedule,
        prompt: parsed.prompt,
        pre_tools: parsed.pre_tools,
        carry_last_output: parsed.carry_last_output,
        model_aliases_json: parsed.model_aliases.map(|m| m.to_json()),
        ghost: ghost.clone(),
        state_file: cron_state_file(workspace_root, &key),
    })
}

fn collect_cron_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_cron_files(&path, out);
            continue;
        }
        if path.extension().and_then(|v| v.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

fn load_jobs_from_workspace(ghost: &Ghost, workspace_root: &Path) -> Vec<CronScheduledJob> {
    let cron_root = workspace_root.join("cron");
    let mut files = Vec::new();
    collect_cron_files(&cron_root, &mut files);

    let mut jobs = Vec::new();
    for file in files {
        let raw = match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(err) => {
                warn!("cron: failed to read {}: {err}", file.display());
                continue;
            }
        };
        let parsed = match parse_cron_job_markdown(&file, &raw) {
            Ok(p) => p,
            Err(err) => {
                warn!("cron: invalid file {}: {err}", file.display());
                continue;
            }
        };
        if !parsed.enabled {
            continue;
        }
        match build_job_from_file(ghost, workspace_root, &file, parsed) {
            Ok(job) => jobs.push(job),
            Err(err) => warn!("cron: invalid schedule in {}: {err}", file.display()),
        }
    }
    jobs
}

async fn resolve_target_session(
    state: &AppState,
    ghost_id: &str,
    operator_id: &str,
) -> Option<Session> {
    if let Ok(Some(active)) =
        SessionRepository::get_active(state.koma_db.pool(), ghost_id, operator_id).await
    {
        return Some(active);
    }

    let infos = SessionRepository::list(state.koma_db.pool(), ghost_id, operator_id)
        .await
        .ok()?;
    let latest = infos.first()?;
    SessionRepository::get_by_id(state.koma_db.pool(), &latest.id)
        .await
        .ok()
        .flatten()
}

fn format_pre_tool_results(results: &[(CronPreToolCall, String)]) -> String {
    if results.is_empty() {
        return "(none)".to_string();
    }
    results
        .iter()
        .enumerate()
        .map(|(idx, (call, output))| {
            format!(
                "### [{}] {}({})\n{}",
                idx + 1,
                call.name,
                call.input,
                output
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

async fn read_previous_output(job: &CronScheduledJob) -> String {
    if !job.carry_last_output {
        return "(disabled by configuration)".to_string();
    }
    match tokio::fs::read_to_string(&job.state_file).await {
        Ok(s) if !s.trim().is_empty() => s,
        _ => "(none)".to_string(),
    }
}

fn build_cron_prompt(
    job: &CronScheduledJob,
    previous_output: &str,
    pre_tool_results: &[(CronPreToolCall, String)],
) -> String {
    let pre_tools = format_pre_tool_results(pre_tool_results);
    crate::content::prompt_text(
        crate::content::ids::PROMPT_CRON,
        None,
        &[
            ("job_name", job.name.as_str()),
            ("schedule", job.schedule_raw.as_str()),
            ("previous_output", previous_output),
            ("pre_tool_results", pre_tools.as_str()),
            ("job_prompt", job.prompt.as_str()),
        ],
    )
    .unwrap_or_else(|_| {
        format!(
            "CRON job: {}\nSchedule: {}\nPrevious output:\n{}\n\nPre-tool results:\n{}\n\nTask:\n{}",
            job.name, job.schedule_raw, previous_output, pre_tools, job.prompt
        )
    })
}

async fn write_previous_output(job: &CronScheduledJob, value: &str) {
    if !job.carry_last_output {
        let _ = tokio::fs::remove_file(&job.state_file).await;
        return;
    }
    if let Some(parent) = job.state_file.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(&job.state_file, value).await;
}

async fn run_single_cron_job(state: &Arc<AppState>, job: &CronScheduledJob) {
    let Some(session) =
        resolve_target_session(state, &job.ghost.id, &job.ghost.owner_operator_id).await
    else {
        warn!(
            "cron: no target session for ghost={} job={}",
            job.ghost.name, job.name
        );
        return;
    };

    let chat_key = format!("{}:{}:{}", session.operator_id, job.ghost.name, session.id);
    if state.is_chat_in_flight(&chat_key).await {
        return;
    }

    let model = state
        .resolve_model_for_ghost_with_override_json(&job.ghost, job.model_aliases_json.as_deref());
    // TODO(cron-tools-policy): Review CRON tool policy. Consider per-CRON allowlists,
    // explicit exclusions, or named profiles (e.g. read-only/shared/coding) instead of
    // a single global set.
    let cron_tm = ToolManager::new_cron(state.session_chat.skill_paths().to_vec());
    state.set_chat_in_flight(&chat_key).await;

    let pre_tools = match state
        .session_chat
        .run_pre_model_tools_for_job(
            &state.koma_db,
            &job.ghost.id,
            &session.operator_id,
            &model.model,
            &job.pre_tools,
            Some(&cron_tm),
        )
        .await
    {
        Ok(results) => results,
        Err(err) => {
            state.clear_chat_in_flight(&chat_key).await;
            let mut log = JobLog::start(&job.ghost.id, DbJobKind::Cron, &session.id);
            log.finish(&format!("error [{}]: pre-tools failed: {err}", job.name));
            let _ = JobLogRepository::insert(state.koma_db.pool(), &log).await;
            state
                .log(LogEntry::Cron {
                    ghost_name: job.ghost.name.clone(),
                    session_id: session.id.clone(),
                    status: format!("error: pre-tools failed ({err})"),
                    job_name: job.name.clone(),
                })
                .await;
            return;
        }
    };

    let previous_output = read_previous_output(job).await;
    let prompt = build_cron_prompt(job, &previous_output, &pre_tools);
    let result = state
        .session_chat
        .chat_job(
            &state.koma_db,
            &job.ghost.id,
            model.client.as_ref(),
            &model.provider,
            &model.model,
            model.context_window,
            &session.id,
            &session.operator_id,
            &prompt,
            true,
            Some(&cron_tm),
            None,
            None,
            model.retry_on_empty,
        )
        .await;

    state.clear_chat_in_flight(&chat_key).await;

    match result {
        Ok(job_result) => {
            state.circuit_breaker.record_success(&model.alias);

            let mut log = JobLog::start(&job.ghost.id, DbJobKind::Cron, &session.id);
            log.transcript = job_result.transcript;
            log.finish(&format!("ok [{}]", job.name));
            if let Err(err) = JobLogRepository::insert(state.koma_db.pool(), &log).await {
                warn!(
                    "cron: failed to write job log for {}: {} ({err})",
                    job.ghost.name, job.name
                );
            }

            if let Err(err) = SessionRepository::add_message(
                state.koma_db.pool(),
                &job.ghost.id,
                &session.id,
                MessageRole::Ghost,
                vec![ContentBlock::Text {
                    text: job_result.response_text.clone(),
                }],
                None,
            )
            .await
            {
                warn!(
                    "cron: failed to post message to session {}:{} ({err})",
                    job.ghost.name, session.id
                );
            }

            write_previous_output(job, &job_result.response_text).await;

            state
                .log(LogEntry::Cron {
                    ghost_name: job.ghost.name.clone(),
                    session_id: session.id.clone(),
                    status: "ran".to_string(),
                    job_name: job.name.clone(),
                })
                .await;
        }
        Err(err) => {
            let mut log = JobLog::start(&job.ghost.id, DbJobKind::Cron, &session.id);
            log.finish(&format!("error [{}]: {err}", job.name));
            let _ = JobLogRepository::insert(state.koma_db.pool(), &log).await;
            state
                .log(LogEntry::Cron {
                    ghost_name: job.ghost.name.clone(),
                    session_id: session.id.clone(),
                    status: format!("error: {err}"),
                    job_name: job.name.clone(),
                })
                .await;
        }
    }
}

async fn reload_jobs(state: &Arc<AppState>, runtime: &mut CronRuntime) {
    let ghosts = match GhostRepository::list_all(state.koma_db.pool()).await {
        Ok(v) => v,
        Err(err) => {
            warn!("cron: failed to list ghosts: {err}");
            return;
        }
    };

    for ghost in &ghosts {
        let Ok(workspace) = t_koma_db::ghosts::ghost_workspace_path(&ghost.name) else {
            continue;
        };
        let cron_dir = workspace.join("cron");
        if !cron_dir.exists() {
            continue;
        }
        if runtime.watched_dirs.insert(cron_dir.clone())
            && let Err(err) = runtime.watcher.watch(&cron_dir, RecursiveMode::Recursive)
        {
            warn!("cron: failed to watch {}: {err}", cron_dir.display());
        }
    }

    let mut jobs = HashMap::new();
    for ghost in ghosts {
        let Ok(workspace) = t_koma_db::ghosts::ghost_workspace_path(&ghost.name) else {
            continue;
        };
        for job in load_jobs_from_workspace(&ghost, &workspace) {
            jobs.insert(job.key.clone(), job);
        }
    }

    // Remove scheduler entries for deleted jobs.
    let existing: HashSet<String> = runtime.jobs.keys().cloned().collect();
    let now_existing: HashSet<String> = jobs.keys().cloned().collect();
    for removed in existing.difference(&now_existing) {
        state.scheduler_set(JobKind::Cron, removed, None).await;
    }

    runtime.jobs = jobs;
}

async fn run_cron_tick(state: Arc<AppState>, runtime: &mut CronRuntime) {
    reload_jobs(&state, runtime).await;

    let now = Utc::now();
    let now_ts = now.timestamp();
    for job in runtime.jobs.values() {
        let Some(initial_due) = next_due_at_or_after(&job.schedule, now) else {
            continue;
        };
        if state.scheduler_get(JobKind::Cron, &job.key).await.is_none() {
            state
                .scheduler_set(JobKind::Cron, &job.key, Some(initial_due))
                .await;
        }

        let due = state
            .scheduler_get(JobKind::Cron, &job.key)
            .await
            .unwrap_or(initial_due);
        if now_ts >= due && now_ts < due + 60 {
            run_single_cron_job(&state, job).await;
            if let Some(next_due) = next_due_after(&job.schedule, due) {
                state
                    .scheduler_set(JobKind::Cron, &job.key, Some(next_due))
                    .await;
            } else {
                state.scheduler_set(JobKind::Cron, &job.key, None).await;
            }
        } else if now_ts > due
            && let Some(next_due) = next_due_at_or_after(&job.schedule, now)
        {
            state
                .scheduler_set(JobKind::Cron, &job.key, Some(next_due))
                .await;
        }
    }
}

pub fn start_cron_runner(state: Arc<AppState>, check_seconds: u64) -> tokio::task::JoinHandle<()> {
    let mut interval = interval_at(
        Instant::now() + Duration::from_secs(check_seconds),
        Duration::from_secs(check_seconds),
    );
    let (tx, mut rx) = mpsc::unbounded_channel::<()>();
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = tx.send(());
        }
    })
    .expect("cron watcher creation should succeed");

    let handle = tokio::spawn(async move {
        let mut runtime = CronRuntime {
            watcher,
            watched_dirs: HashSet::new(),
            jobs: HashMap::new(),
        };

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    run_cron_tick(Arc::clone(&state), &mut runtime).await;
                }
                evt = rx.recv() => {
                    if evt.is_some() {
                        // Change detected; jobs are reloaded on every tick.
                    }
                }
            }
        }
    });

    info!("cron runner started (check_seconds={check_seconds})");
    handle
}
