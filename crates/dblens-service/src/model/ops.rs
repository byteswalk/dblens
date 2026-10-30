//! 模型的服务层操作（M4a 设计文档第 4 节）：最近打开的模型、新建、反向生成、设计器保存。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dblens_core::schema::Table;
use dblens_core::{Error, Msg, Result, msg};
use dblens_dialect::design::{DesignIssue, Renames, TableChange, diff_table, validate};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use serde::{Deserialize, Serialize};

use super::file::ModelDatabase;
use super::store::ModelSummary;
use crate::service::Service;
use crate::task::{TaskKind, TaskPhase};

/// 最近打开的模型目录（JSON 数组，最近的在前）。
pub const RECENT_MODELS_SETTING: &str = "ui.models.recent";
const RECENT_MAX: usize = 10;

/// 新建空模型。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NewModel {
    pub path: String,
    pub name: String,
    pub database: ModelDatabase,
    #[serde(default)]
    pub schemas: Vec<String>,
}

/// 从库反向生成模型。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReverseRequest {
    pub profile_id: String,
    /// 所在的库（SQL Server，M6b 设计文档第 6 节）；其他库不填。
    #[serde(default)]
    pub catalog: Option<String>,

    pub schemas: Vec<String>,
    pub path: String,
    pub name: String,
}

/// 设计器保存模型中的表。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ModelDesignRequest {
    pub schema: String,
    /// 已有表的名字；新建表时不给。
    #[serde(default)]
    pub name: Option<String>,
    pub draft: Table,
    #[serde(default)]
    pub renames: Renames,
    /// 打开时文件的指纹；保存前核对。
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// 保存前的检查：校验错误、提示、变更列表。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelDesignPreview {
    pub errors: Vec<DesignIssue>,
    pub warnings: Vec<Msg>,
    pub changes: Vec<TableChange>,
    /// 文件已被修改（与请求中的指纹不同），要重新加载。
    pub conflict: bool,
}

impl Service {
    pub fn recent_models(&self) -> Result<Vec<String>> {
        let value = self.store.setting(RECENT_MODELS_SETTING)?;
        Ok(value.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
    }

    fn remember_model(&self, path: &str) -> Result<()> {
        let mut list = self.recent_models()?;
        list.retain(|p| p != path);
        list.insert(0, path.to_string());
        list.truncate(RECENT_MAX);
        let text = serde_json::to_string(&list).unwrap_or_default();
        self.store.set_setting(RECENT_MODELS_SETTING, Some(&text))
    }

    pub fn open_model(&self, path: &Path) -> Result<ModelSummary> {
        let s = self.models.open(path)?;
        self.remember_model(&s.path)?;
        Ok(s)
    }

    pub fn create_model(&self, req: NewModel) -> Result<ModelSummary> {
        let s = self.models.create(Path::new(&req.path), &req.name, req.database, req.schemas)?;
        self.remember_model(&s.path)?;
        Ok(s)
    }

    /// 反向生成：作为任务执行，返回任务 id；结束后模型已打开，任务状态的 `model` 是模型编号。
    pub async fn start_reverse(self: &Arc<Self>, req: ReverseRequest) -> Result<String> {
        let profile = self.scoped_profile(&req.profile_id, req.catalog.as_deref())?;
        if req.schemas.is_empty() {
            return Err(Error::invalid(msg!("model.no_schemas")));
        }
        let root = std::path::absolute(&req.path).unwrap_or_else(|_| PathBuf::from(&req.path));
        if root.join("model.yaml").exists() || std::fs::read_dir(&root).is_ok_and(|mut d| d.next().is_some())
        {
            return Err(Error::invalid(msg!("model.dir_not_empty", path = root.display())));
        }
        let dialect = self.metadata.dialect(&profile).await?;
        crate::later::ensure_full(&dialect)?;
        let task = self.tasks.create(TaskKind::Reverse)?;
        let mut id = String::new();
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Scanning;
            s.progress.total = req.schemas.len() as u64;
        });
        let service = self.clone();
        tokio::spawn(async move {
            let run = async {
                let mut tables = Vec::new();
                for schema in &req.schemas {
                    if task.cancelled() {
                        return Err(Error::Cancelled);
                    }
                    // 跳过元数据缓存，以库中最新的结构为准
                    let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                    for key in MetaKey::TABLE_DETAIL {
                        service.metadata.cache().forget(&profile.id, key, &args);
                    }
                    let list: Vec<Table> = service
                        .metadata
                        .tables(&profile, schema, None)
                        .await?
                        .into_iter()
                        .map(|t| Table { triggers: vec![], ..t })
                        .collect();
                    tables.push((schema.clone(), list));
                    task.update(|s| s.progress.read += 1);
                }
                task.update(|s| s.phase = TaskPhase::Writing);
                let database = ModelDatabase {
                    variant: dialect.variant(),
                    compat: dialect.compat(),
                    version: dialect.version(),
                };
                let m = service.models.create_from(&root, &req.name, database, &tables)?;
                service.remember_model(&m.path)?;
                task.update(|s| s.model = Some(m.id.clone()));
                Ok(())
            };
            let result = run.await;
            task.finish(result);
        });
        Ok(id)
    }

    async fn prepare_model_design(
        &self,
        id: &str,
        req: &ModelDesignRequest,
    ) -> Result<(ModelDesignPreview, Table)> {
        let dialect = self.models.dialect(id)?;
        let mut draft = req.draft.clone();
        if let Some(name) = &req.name {
            // 表名在模型面板中改
            draft.name = name.clone();
        }
        let warnings = dialect.prepare_draft(&mut draft);
        let mut preview =
            ModelDesignPreview { errors: validate(&draft), warnings, changes: vec![], conflict: false };
        let original = match &req.name {
            Some(n) => Some(self.models.read_table(id, &req.schema, n)?),
            None => None,
        };
        preview.conflict =
            matches!((&original, &req.fingerprint), (Some(o), Some(fp)) if &o.fingerprint != fp);
        if preview.errors.is_empty() && !preview.conflict {
            let original = original.map(|o| o.table);
            preview.changes = diff_table(original.as_ref(), &draft, &req.renames)?.changes;
        }
        Ok((preview, draft))
    }

    /// 保存前的检查（不写文件）。
    pub async fn preview_model_design(
        &self,
        id: &str,
        req: &ModelDesignRequest,
    ) -> Result<ModelDesignPreview> {
        Ok(self.prepare_model_design(id, req).await?.0)
    }

    /// 保存这张表的文件，返回新的指纹。有校验错误、文件已被修改时报错。
    pub async fn save_model_design(&self, id: &str, req: ModelDesignRequest) -> Result<String> {
        let (preview, draft) = self.prepare_model_design(id, &req).await?;
        if preview.conflict {
            let name = format!("{}.{}", req.schema, draft.name);
            return Err(Error::invalid(msg!("model.conflict", name = name)));
        }
        if !preview.errors.is_empty() {
            return Err(Error::invalid(msg!("design.has_errors")));
        }
        let columns: BTreeMap<String, String> = req.renames.columns.clone();
        self.models.save_table(
            id,
            &req.schema,
            req.name.as_deref(),
            draft,
            &columns,
            req.fingerprint.as_deref(),
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Duration;

    use dblens_core::schema::{Column, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_dialect::{Compat, Variant};

    use super::*;
    use crate::fake::{FakeDriver, Log};
    use crate::history::HistoryLog;
    use crate::policy::DefaultPolicy;
    use crate::profile::tests::input;
    use crate::registry::DriverRegistry;
    use crate::secret::{SecretBackend, Secrets};
    use crate::store::Store;
    use crate::task::{TaskState, TaskStatus};

    pub(crate) fn service() -> (Arc<Service>, String, tempfile::TempDir) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
        secrets.init_vault("master").unwrap();
        let log: Log = Arc::default();
        let registry =
            Arc::new(DriverRegistry::with_drivers(store.clone(), vec![Box::new(FakeDriver { log })]));
        let s = Service::with_history(
            store.clone(),
            secrets,
            registry,
            Arc::new(DefaultPolicy),
            HistoryLog::inline(store),
        );
        let dir = tempfile::tempdir().unwrap();
        s.tasks.set_dirs(dir.path().join("tasks"), dir.path().join("imports"));
        let p = s.create_profile(input("dev"), Some("pw")).unwrap();
        (Arc::new(s), p.id, dir)
    }

    pub(crate) async fn wait(s: &Service, id: &str) -> TaskStatus {
        for _ in 0..1000 {
            let st = s.tasks.status(id).unwrap();
            if st.state != TaskState::Running {
                return st;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("任务没有结束");
    }

    fn draft(name: &str, cols: &[&str]) -> Table {
        Table {
            name: name.into(),
            columns: cols
                .iter()
                .map(|c| Column {
                    name: (*c).into(),
                    ordinal: 0,
                    data_type: ColumnType { native: "integer".into(), logical: LogicalType::Other },
                    nullable: true,
                    default: None,
                    comment: None,
                    auto_increment: false,
                    generated: None,
                })
                .collect(),
            primary_key: Some(PrimaryKey { name: None, columns: vec![cols[0].into()] }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn create_remembers_recent_and_design_saves_files() {
        let (s, _, dir) = service();
        let path = dir.path().join("model");
        let db = ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None };
        let req =
            NewModel { path: path.to_string_lossy().into(), name: "m".into(), database: db, schemas: vec![] };
        let m = s.create_model(req).unwrap();
        assert_eq!(s.recent_models().unwrap(), std::slice::from_ref(&m.path));
        // 新建表：主键列改为 NOT NULL 的提示，与 M3 相同
        let mut req = ModelDesignRequest {
            schema: "public".into(),
            name: None,
            draft: draft("t", &["id", "a"]),
            renames: Renames::default(),
            fingerprint: None,
        };
        let p = s.preview_model_design(&m.id, &req).await.unwrap();
        assert_eq!(p.warnings, vec![msg!("design.pk_not_null", column = "id")]);
        assert!(matches!(p.changes[..], [TableChange::CreateTable { .. }]));
        let fp = s.save_model_design(&m.id, req.clone()).await.unwrap();
        // 改列名
        req.name = Some("t".into());
        req.draft = draft("t", &["id", "b"]);
        req.renames.columns.insert("b".into(), "a".into());
        req.fingerprint = Some(fp);
        let p = s.preview_model_design(&m.id, &req).await.unwrap();
        assert!(matches!(p.changes[..], [TableChange::RenameColumn { .. }]), "{:?}", p.changes);
        s.save_model_design(&m.id, req.clone()).await.unwrap();
        assert_eq!(s.models.renames(&m.id, "public").unwrap().columns["t"]["b"], "a");
        // 用旧指纹预览标出冲突、保存被拒绝；校验错误不保存
        assert!(s.preview_model_design(&m.id, &req).await.unwrap().conflict);
        let err = s.save_model_design(&m.id, req.clone()).await.unwrap_err();
        assert!(err.to_string().contains("changed"), "{err}");
        req.fingerprint = Some(s.models.read_table(&m.id, "public", "t").unwrap().fingerprint);
        req.renames = Renames::default();
        req.draft.columns[0].data_type.native = String::new();
        let err = s.save_model_design(&m.id, req).await.unwrap_err();
        assert!(err.to_string().contains("error"), "{err}");
    }

    /// 假驱动没有元数据结果：任务失败，不留下模型目录（成功的流程在实库测试中）。
    #[tokio::test]
    async fn reverse_task_failure_leaves_nothing() {
        let (s, dev, dir) = service();
        let path = dir.path().join("rev");
        let req = ReverseRequest {
            profile_id: dev.clone(),
            catalog: None,
            schemas: vec!["public".into()],
            path: path.to_string_lossy().into(),
            name: "rev".into(),
        };
        let id = s.start_reverse(req.clone()).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!((st.state, st.model), (TaskState::Failed, None));
        assert!(!path.exists());
        // 没有选 schema、目录不为空：不启动任务
        let none = ReverseRequest { schemas: vec![], ..req.clone() };
        assert!(s.start_reverse(none).await.is_err());
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("x.txt"), "x").unwrap();
        let err = s.start_reverse(req).await.unwrap_err();
        assert!(err.to_string().contains("not empty"), "{err}");
    }
}
