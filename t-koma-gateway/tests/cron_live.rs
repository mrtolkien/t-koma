//! Live CRON test (requires --features live-tests).
//!
//! Verifies one CRON run can:
//! - execute configured pre-model `web_fetch`
//! - complete successfully
//! - avoid model-initiated extra tool calls

#[cfg(feature = "live-tests")]
#[path = "common/mod.rs"]
mod common;

#[cfg(feature = "live-tests")]
use std::ffi::OsString;
#[cfg(feature = "live-tests")]
use std::time::Duration;

#[cfg(feature = "live-tests")]
use tempfile::TempDir;
#[cfg(feature = "live-tests")]
use tokio::time::{Instant, sleep};

#[cfg(feature = "live-tests")]
use t_koma_db::{ContentBlock, JobKind, JobLogRepository, SessionRepository, ghosts};
#[cfg(feature = "live-tests")]
use t_koma_gateway::cron::start_cron_runner;

#[cfg(feature = "live-tests")]
use common::{build_state_with_default_model, setup_test_environment};

#[cfg(feature = "live-tests")]
struct EnvVarGuard {
    key: &'static str,
    prev: Option<OsString>,
}

#[cfg(feature = "live-tests")]
impl EnvVarGuard {
    fn set(key: &'static str, value: &std::path::Path) -> Self {
        let prev = std::env::var_os(key);
        // SAFETY: integration tests are single-process and this test restores
        // the previous value on drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prev }
    }
}

#[cfg(feature = "live-tests")]
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: restoring prior process env value for this test.
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_cron_live_hn_rss_pre_fetch_only() {
    t_koma_core::load_dotenv();
    let settings = match t_koma_core::Settings::load() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("failed to load settings; skipping cron live test: {err}");
            return;
        }
    };

    if !settings.tools.web.enabled || !settings.tools.web.fetch.enabled {
        eprintln!("web_fetch is disabled in settings; skipping cron live test.");
        return;
    }

    let data_dir = TempDir::new().expect("temp data dir");
    let _data_dir_guard = EnvVarGuard::set("T_KOMA_DATA_DIR", data_dir.path());

    let env = setup_test_environment("cron_live_operator", "cron_live_ghost")
        .await
        .expect("setup test environment");
    let state = build_state_with_default_model(env.koma_db.clone()).await;

    let session = SessionRepository::create(env.koma_db.pool(), &env.ghost.id, &env.operator.id)
        .await
        .expect("create active session");

    let workspace =
        ghosts::ghost_workspace_path(&env.ghost.name).expect("resolve ghost workspace path");
    let cron_dir = workspace.join("cron");
    tokio::fs::create_dir_all(&cron_dir)
        .await
        .expect("create cron dir");

    let cron_md = r#"+++
name = "HN recap"
schedule = "* * * * *"
enabled = true
carry_last_output = true
pre_tools = [
  { name = "web_fetch", input = { url = "https://hnrss.org/frontpage", mode = "text", max_chars = 12000 } }
]
+++

Summarize the key new HN items from the pre-tool output.
Output exactly one concise sentence prefixed with: HN recap:
"#;

    tokio::fs::write(cron_dir.join("hn-live.md"), cron_md)
        .await
        .expect("write cron markdown");

    let runner = start_cron_runner(state, 1);

    let timeout = Instant::now() + Duration::from_secs(90);
    let log = loop {
        if let Some(found) = JobLogRepository::latest_ok(
            env.koma_db.pool(),
            &env.ghost.id,
            &session.id,
            JobKind::Cron,
        )
        .await
        .expect("query latest cron job")
        {
            break found;
        }

        if Instant::now() >= timeout {
            panic!("timed out waiting for cron run to complete successfully");
        }

        sleep(Duration::from_millis(500)).await;
    };

    runner.abort();

    assert!(
        log.status
            .as_deref()
            .unwrap_or_default()
            .starts_with("ok [HN recap]"),
        "unexpected cron status: {:?}",
        log.status
    );

    let prompt_text = log
        .transcript
        .iter()
        .find_map(|entry| {
            entry.content.iter().find_map(|block| match block {
                ContentBlock::Text { text } if entry.role == t_koma_db::MessageRole::Operator => {
                    Some(text.as_str())
                }
                _ => None,
            })
        })
        .unwrap_or_default();

    assert!(
        prompt_text.contains("web_fetch(") && prompt_text.contains("hnrss.org/frontpage"),
        "expected pre-model web_fetch output in cron prompt; got: {}",
        prompt_text
    );
    assert!(
        prompt_text.contains("[Result #"),
        "expected cached web_fetch result marker in cron prompt"
    );

    let model_tool_calls = log
        .transcript
        .iter()
        .flat_map(|entry| entry.content.iter())
        .filter(|block| matches!(block, ContentBlock::ToolUse { .. }))
        .count();

    let model_tool_results = log
        .transcript
        .iter()
        .flat_map(|entry| entry.content.iter())
        .filter(|block| matches!(block, ContentBlock::ToolResult { .. }))
        .count();

    assert_eq!(
        model_tool_calls, 0,
        "model initiated tool calls in CRON job transcript"
    );
    assert_eq!(
        model_tool_results, 0,
        "unexpected tool result blocks in CRON job transcript"
    );
}
