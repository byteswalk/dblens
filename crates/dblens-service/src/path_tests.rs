//! 各执行路径和策略的衔接：能拦住、会回报。

use std::sync::Arc;
use std::time::Duration;

use dblens_core::Error;
use dblens_dialect::{Dialect, Variant};

use crate::design::Run;
use crate::export::{ExportFormat, ExportOptions, ExportRequest, ExportSource, ExportState};
use crate::fake::{FakeDriver, Log, SpyPolicy};
use crate::history::HistoryLog;
use crate::import::TextEncoding;
use crate::policy::{ExecContext, Operation};
use crate::profile::tests::input;
use crate::registry::DriverRegistry;
use crate::secret::{SecretBackend, Secrets};
use crate::service::Service;
use crate::sqlfile::SqlFileRequest;
use crate::store::Store;
use crate::task::{FileSource, TaskKind, TaskState};

fn service(spy: Arc<SpyPolicy>) -> (Arc<Service>, String, tempfile::TempDir) {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("master").unwrap();
    let log: Log = Arc::default();
    let registry =
        Arc::new(DriverRegistry::with_drivers(store.clone(), vec![Box::new(FakeDriver { log })]));
    let s = Service::with_history(store, secrets, registry, spy, HistoryLog::off());
    let dir = tempfile::tempdir().unwrap();
    s.exports.set_dir(dir.path().join("exports"));
    s.tasks.set_dirs(dir.path().join("tasks"), dir.path().join("imports"));
    let p = s.create_profile(input("pg"), Some("pw")).unwrap();
    (Arc::new(s), p.id, dir)
}

fn export(profile_id: &str) -> ExportRequest {
    ExportRequest {
        profile_id: profile_id.into(),
        source: ExportSource::Sql("SELECT 1".into()),
        format: ExportFormat::Csv,
        options: ExportOptions::default(),
        path: None,
        file_name: "out".into(),
    }
}

async fn reported_after(spy: &SpyPolicy, done: impl Fn() -> bool) -> Vec<(String, Operation)> {
    for _ in 0..1000 {
        if done() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(done(), "任务没有结束");
    spy.reported.lock().unwrap().clone()
}

#[tokio::test]
async fn policy_can_block_export() {
    let spy = Arc::new(SpyPolicy { block: Some(Operation::Export), ..Default::default() });
    let (s, id, _dir) = service(spy);
    let r = s.start_export(export(&id), false).await;
    assert!(matches!(r, Err(Error::Denied { .. })));
}

#[tokio::test]
async fn export_reports_its_result() {
    let spy = Arc::new(SpyPolicy::default());
    let (s, id, _dir) = service(spy.clone());
    let job = s.start_export(export(&id), false).await.unwrap();
    let reported = reported_after(&spy, || {
        s.exports.status(&job).unwrap().state != ExportState::Running && !spy.reported.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(reported, [("SELECT 1".to_string(), Operation::Export)]);
}

#[tokio::test]
async fn sql_file_reports_each_statement() {
    let spy = Arc::new(SpyPolicy::default());
    let (s, id, _dir) = service(spy.clone());
    let (up, path) = s.tasks.begin_upload().unwrap();
    let sql = "INSERT INTO t VALUES (1);\nINSERT INTO t VALUES (2);\n";
    std::fs::write(&path, sql).unwrap();
    s.tasks.finish_upload(&up, "a.sql", sql.len() as u64).unwrap();
    let req = SqlFileRequest {
        profile_id: id,
        source: FileSource::Upload { upload: up },
        encoding: TextEncoding::Auto,
        schema: None,
        continue_on_error: false,
        transaction: false,
    };
    let task = s.start_sql_file(req, false).await.unwrap();
    // SQL 文件总要确认一次
    reported_after(&spy, || s.tasks.status(&task).unwrap().state == TaskState::Confirm).await;
    s.tasks.confirm(&task, true).unwrap();
    let reported = reported_after(&spy, || s.tasks.status(&task).unwrap().state == TaskState::Done).await;
    assert_eq!(reported.len(), 2);
    assert!(reported.iter().all(|(_, op)| *op == Operation::SqlFile));
}

#[tokio::test]
async fn design_run_reports_each_statement() {
    let spy = Arc::new(SpyPolicy::default());
    let (s, id, _dir) = service(spy.clone());
    let profile = s.profile(&id).unwrap();
    let dialect = Dialect::new(Variant::Postgres);
    let task = s.tasks.create(TaskKind::Design).unwrap();
    let run = Run {
        service: &s,
        task: &task,
        profile: &profile,
        dialect,
        schema: "public",
        ctx: ExecContext::new(&profile, dialect, Operation::Design),
    };
    let (mut conn, _) = s.registry.connect(&profile, "pw".into()).await.unwrap();
    run.sql(&mut conn, "ALTER TABLE t ADD c int").await.unwrap();
    assert!(run.sql(&mut conn, "FAIL").await.is_err());
    let reported = spy.reported.lock().unwrap().clone();
    assert_eq!(reported.len(), 2);
    assert_eq!(reported[0], ("ALTER TABLE t ADD c int".to_string(), Operation::Design));
}
