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
use crate::operator::Operator;
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
    let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![Box::new(FakeDriver { log })]));
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
    assert!(matches!(r, Err(Error::TicketRequired { .. })));
}

#[tokio::test]
async fn export_reports_its_result() {
    let spy = Arc::new(SpyPolicy::default());
    let (s, id, _dir) = service(spy.clone());
    let job = s.start_export(export(&id), false).await.unwrap();
    let reported = reported_after(&spy, || {
        s.exports.status(&job).unwrap().state != ExportState::Running
            && !spy.reported.lock().unwrap().is_empty()
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

fn stranger() -> Operator {
    Operator { id: "other".into(), name: String::new() }
}

#[tokio::test]
async fn sessions_belong_to_their_operator() {
    let (s, id, _dir) = service(Arc::default());
    let info = s.open_session(&id).await.unwrap();
    let other = Operator::scope(stranger(), async { (s.sessions.info(info.id), s.sessions.list()) }).await;
    assert!(matches!(other.0, Err(Error::NotFound { .. })));
    assert!(other.1.unwrap().is_empty());
    let hijack = Operator::scope(stranger(), s.sessions.execute(info.id, "ROWS 1", false)).await;
    assert!(matches!(hijack, Err(Error::NotFound { .. })));
    let closed = Operator::scope(stranger(), s.sessions.close(info.id, false)).await;
    assert!(matches!(closed, Err(Error::NotFound { .. })));
    assert!(s.sessions.info(info.id).is_ok(), "所有者不受影响");
}

#[tokio::test]
async fn tasks_and_uploads_belong_to_their_operator() {
    let (s, _, _dir) = service(Arc::default());
    let task = s.tasks.create(TaskKind::SqlFile).unwrap();
    let id = {
        let mut id = String::new();
        task.update(|st| id = st.id.clone());
        id
    };
    let r = Operator::scope(stranger(), async { s.tasks.status(&id) }).await;
    assert!(matches!(r, Err(Error::NotFound { .. })));
    assert!(s.tasks.status(&id).is_ok());
    let (up, path) = s.tasks.begin_upload().unwrap();
    std::fs::write(&path, "x").unwrap();
    s.tasks.finish_upload(&up, "a.sql", 1).unwrap();
    let src = FileSource::Upload { upload: up };
    assert!(Operator::scope(stranger(), async { s.tasks.resolve(&src, false, false) }).await.is_err());
    assert!(s.tasks.resolve(&src, false, false).is_ok());
}

#[tokio::test]
async fn exports_belong_to_their_operator() {
    let (s, id, _dir) = service(Arc::default());
    let job = s.start_export(export(&id), false).await.unwrap();
    let r = Operator::scope(stranger(), async { (s.exports.status(&job), s.exports.take_file(&job)) }).await;
    assert!(matches!(r.0, Err(Error::NotFound { .. })));
    assert!(matches!(r.1, Err(Error::NotFound { .. })));
    assert!(s.exports.status(&job).is_ok());
}
