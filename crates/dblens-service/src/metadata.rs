//! 元数据浏览（设计文档 5.3、6.4 节）：每个连接配置一个小连接池（默认 2 个连接），不会被编辑器里正在跑的
//! 长查询堵住；查询结果按（连接配置、查询种类、参数）缓存，手动刷新或执行 DDL 后失效。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use dblens_core::schema::{Routine, Sequence, Synonym, Table, View};
use dblens_core::{Error, Result, msg};
use dblens_dialect::Dialect;
use dblens_dialect::meta::{self, MetaArgs, MetaKey, MetaRows, ObjectInfo, SchemaInfo, TableRows};
use dblens_driver::{Connection, collect_all};
use tokio::sync::Semaphore;

use crate::policy::ObjectRef;
use crate::profile::Profile;
use crate::registry::DriverRegistry;
use crate::secret::Secrets;

/// 每个连接配置用于浏览元数据的最大连接数。
pub const METADATA_POOL_SIZE: usize = 2;

type CacheKey = (String, MetaKey, MetaArgs);

/// 元数据查询结果的缓存。
#[derive(Default)]
pub struct MetadataCache {
    entries: Mutex<HashMap<CacheKey, Arc<MetaRows>>>,
}

impl MetadataCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, key: &CacheKey) -> Option<Arc<MetaRows>> {
        self.entries.lock().ok()?.get(key).cloned()
    }

    pub(crate) fn put(&self, key: CacheKey, rows: Arc<MetaRows>) {
        if let Ok(mut e) = self.entries.lock() {
            e.insert(key, rows);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|e| e.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 让某个连接配置的缓存失效：给出 schema 时只清这个 schema 下的查询（以及 schema 列表），否则全部清掉。
    /// schema 名不区分大小写比较（未加引号的名字在各库中的大小写规则不同）。
    pub fn invalidate(&self, profile_id: &str, schema: Option<&str>) {
        let Ok(mut e) = self.entries.lock() else { return };
        e.retain(|(p, key, args), _| {
            if p != profile_id {
                return true;
            }
            match schema {
                None => false,
                Some(s) => {
                    let same = args.schema.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(s));
                    !(same || *key == MetaKey::Schemas)
                }
            }
        });
    }

    /// DDL 执行后调用：对象都带 schema 时只清这些 schema，否则（如 `DROP TABLE t`）清掉整个连接配置的缓存。
    pub fn invalidate_objects(&self, profile_id: &str, objects: &[ObjectRef]) {
        if objects.is_empty() || objects.iter().any(|o| o.schema.is_none()) {
            self.invalidate(profile_id, None);
        } else {
            for o in objects {
                self.invalidate(profile_id, o.schema.as_deref());
            }
        }
    }
}

struct Pool {
    permits: Semaphore,
    idle: Mutex<Vec<(Box<dyn Connection>, Dialect)>>,
}

pub struct MetadataService {
    registry: Arc<DriverRegistry>,
    secrets: Arc<Secrets>,
    cache: Arc<MetadataCache>,
    pools: Mutex<HashMap<String, Arc<Pool>>>,
}

impl MetadataService {
    pub fn new(registry: Arc<DriverRegistry>, secrets: Arc<Secrets>, cache: Arc<MetadataCache>) -> Self {
        Self { registry, secrets, cache, pools: Mutex::new(HashMap::new()) }
    }

    pub fn cache(&self) -> &Arc<MetadataCache> {
        &self.cache
    }

    fn pool(&self, profile_id: &str) -> Result<Arc<Pool>> {
        let mut pools =
            self.pools.lock().map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pools")))?;
        Ok(pools
            .entry(profile_id.to_string())
            .or_insert_with(|| {
                Arc::new(Pool { permits: Semaphore::new(METADATA_POOL_SIZE), idle: Mutex::new(Vec::new()) })
            })
            .clone())
    }

    /// 关闭某个连接配置的元数据连接并清掉缓存（连接配置被修改或删除时调用）。
    pub async fn forget(&self, profile_id: &str) -> Result<()> {
        let pool = self
            .pools
            .lock()
            .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pools")))?
            .remove(profile_id);
        if let Some(pool) = pool {
            let idle: Vec<_> = pool.idle.lock().map(|mut v| v.drain(..).collect()).unwrap_or_default();
            for (conn, _) in idle {
                let _ = conn.close().await;
            }
        }
        self.cache.invalidate(profile_id, None);
        Ok(())
    }

    /// 执行一条元数据查询（先查缓存）。该方言没有这种查询时返回 None。
    pub async fn rows(
        &self,
        profile: &Profile,
        key: MetaKey,
        args: &MetaArgs,
    ) -> Result<Option<Arc<MetaRows>>> {
        let cache_key = (profile.id.clone(), key, args.clone());
        if let Some(rows) = self.cache.get(&cache_key) {
            return Ok(Some(rows));
        }
        let pool = self.pool(&profile.id)?;
        let _permit =
            pool.permits.acquire().await.map_err(|_| Error::internal(msg!("metadata.pool_closed")))?;
        let idle = pool
            .idle
            .lock()
            .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pool")))?
            .pop();
        let (mut conn, dialect) = match idle {
            Some(c) => c,
            None => {
                let password = self.secrets.get(&profile.id)?.unwrap_or_default();
                self.registry.connect(profile, password).await?
            }
        };
        let Some(q) = dialect.meta_query(key, args)? else {
            pool.idle
                .lock()
                .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pool")))?
                .push((conn, dialect));
            return Ok(None);
        };
        let result = match conn.execute(&q.sql, &q.params).await {
            Ok(mut stream) => match collect_all(stream.as_mut(), 1000).await {
                Ok(events) => MetaRows::from_events(events),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        match result {
            Ok(rows) => {
                pool.idle
                    .lock()
                    .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pool")))?
                    .push((conn, dialect));
                let rows = Arc::new(rows);
                self.cache.put(cache_key, rows.clone());
                Ok(Some(rows))
            }
            Err(e) => {
                // 连接断开时丢弃这个连接，下次重新建立；其他错误（如权限不足）连接仍可复用
                if matches!(e, Error::Connect { .. }) {
                    let _ = conn.close().await;
                } else {
                    pool.idle
                        .lock()
                        .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pool")))?
                        .push((conn, dialect));
                }
                Err(e)
            }
        }
    }

    async fn rows_or_empty(&self, profile: &Profile, key: MetaKey, args: &MetaArgs) -> Result<Arc<MetaRows>> {
        Ok(self.rows(profile, key, args).await?.unwrap_or_default())
    }

    /// 连接的方言（取一次连接上的识别结果）。
    pub async fn dialect(&self, profile: &Profile) -> Result<Dialect> {
        let pool = self.pool(&profile.id)?;
        if let Some((_, d)) = pool
            .idle
            .lock()
            .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pool")))?
            .last()
        {
            return Ok(*d);
        }
        let _permit =
            pool.permits.acquire().await.map_err(|_| Error::internal(msg!("metadata.pool_closed")))?;
        let password = self.secrets.get(&profile.id)?.unwrap_or_default();
        let (conn, dialect) = self.registry.connect(profile, password).await?;
        pool.idle
            .lock()
            .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pool")))?
            .push((conn, dialect));
        Ok(dialect)
    }

    pub async fn schemas(&self, profile: &Profile, filter: Option<&str>) -> Result<Vec<SchemaInfo>> {
        let args = MetaArgs { filter: filter.map(Into::into), ..Default::default() };
        meta::schemas(&*self.rows_or_empty(profile, MetaKey::Schemas, &args).await?)
    }

    /// 对象树中某个 schema 下的表、视图或物化视图列表。
    pub async fn objects(
        &self,
        profile: &Profile,
        schema: &str,
        key: MetaKey,
        filter: Option<&str>,
    ) -> Result<Vec<ObjectInfo>> {
        if !matches!(key, MetaKey::Tables | MetaKey::Views | MetaKey::MaterializedViews) {
            return Err(Error::invalid(msg!("metadata.not_object_list", key = format!("{key:?}"))));
        }
        let args =
            MetaArgs { schema: Some(schema.into()), filter: filter.map(Into::into), ..Default::default() };
        meta::objects(key, &*self.rows_or_empty(profile, key, &args).await?)
    }

    /// 表明细：给出 `table` 时只取这一张，否则取整个 schema（六条查询）。
    pub async fn tables(&self, profile: &Profile, schema: &str, table: Option<&str>) -> Result<Vec<Table>> {
        let args =
            MetaArgs { schema: Some(schema.into()), table: table.map(Into::into), ..Default::default() };
        let mut rows = Vec::with_capacity(6);
        for key in MetaKey::TABLE_DETAIL {
            rows.push(self.rows_or_empty(profile, key, &args).await?);
        }
        let [tables, columns, keys, checks, indexes, triggers]: [Arc<MetaRows>; 6] =
            rows.try_into().map_err(|_| Error::internal(msg!("metadata.table_detail_count")))?;
        let rows = TableRows {
            tables: (*tables).clone(),
            columns: (*columns).clone(),
            keys: (*keys).clone(),
            checks: (*checks).clone(),
            indexes: (*indexes).clone(),
            triggers: (*triggers).clone(),
        };
        meta::tables(&self.dialect(profile).await?, &rows)
    }

    /// 视图或物化视图（含列）。
    pub async fn views(&self, profile: &Profile, schema: &str, key: MetaKey) -> Result<Vec<View>> {
        let args = MetaArgs { schema: Some(schema.into()), ..Default::default() };
        let views = self.rows_or_empty(profile, key, &args).await?;
        let columns = self.rows_or_empty(profile, MetaKey::Columns, &args).await?;
        meta::views(&self.dialect(profile).await?, key, &views, &columns)
    }

    pub async fn sequences(&self, profile: &Profile, schema: &str) -> Result<Vec<Sequence>> {
        let args = MetaArgs { schema: Some(schema.into()), ..Default::default() };
        let rows = self.rows(profile, MetaKey::Sequences, &args).await?;
        rows.map_or(Ok(vec![]), |r| meta::sequences(&r))
    }

    pub async fn routines(&self, profile: &Profile, schema: &str) -> Result<Vec<Routine>> {
        let args = MetaArgs { schema: Some(schema.into()), ..Default::default() };
        meta::routines(&*self.rows_or_empty(profile, MetaKey::Routines, &args).await?)
    }

    pub async fn synonyms(&self, profile: &Profile, schema: &str) -> Result<Vec<Synonym>> {
        let args = MetaArgs { schema: Some(schema.into()), ..Default::default() };
        let rows = self.rows(profile, MetaKey::Synonyms, &args).await?;
        rows.map_or(Ok(vec![]), |r| meta::synonyms(&r))
    }

    /// 视图定义（`ViewDefinition`）或程序源码（`RoutineSource`，需要 `kind`）。
    pub async fn source(
        &self,
        profile: &Profile,
        schema: &str,
        key: MetaKey,
        name: &str,
        kind: Option<&str>,
    ) -> Result<Option<String>> {
        if !matches!(key, MetaKey::ViewDefinition | MetaKey::RoutineSource) {
            return Err(Error::invalid(msg!("metadata.not_source", key = format!("{key:?}"))));
        }
        let args = MetaArgs {
            schema: Some(schema.into()),
            name: Some(name.into()),
            kind: kind.map(Into::into),
            ..Default::default()
        };
        match self.rows(profile, key, &args).await? {
            Some(rows) => meta::source_text(key, &rows),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(schema: &str) -> MetaArgs {
        MetaArgs { schema: Some(schema.into()), ..Default::default() }
    }

    fn filled() -> MetadataCache {
        let c = MetadataCache::new();
        let rows = Arc::new(MetaRows::default());
        c.put(("p".into(), MetaKey::Schemas, MetaArgs::default()), rows.clone());
        c.put(("p".into(), MetaKey::Tables, args("HR")), rows.clone());
        c.put(("p".into(), MetaKey::Columns, args("hr")), rows.clone());
        c.put(("p".into(), MetaKey::Tables, args("SALES")), rows.clone());
        c.put(("q".into(), MetaKey::Tables, args("HR")), rows);
        c
    }

    #[test]
    fn invalidate_by_schema_is_case_insensitive_and_drops_schema_list() {
        let c = filled();
        c.invalidate("p", Some("hr"));
        assert_eq!(c.len(), 2, "只剩 p/SALES 和 q/HR");
        assert!(c.get(&("p".into(), MetaKey::Tables, args("SALES"))).is_some());
        assert!(c.get(&("q".into(), MetaKey::Tables, args("HR"))).is_some());
        c.invalidate("p", None);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn ddl_without_schema_clears_the_whole_profile() {
        let c = filled();
        c.invalidate_objects("p", &[ObjectRef { schema: Some("SALES".into()), name: "t".into() }]);
        assert_eq!(c.len(), 3);
        c.invalidate_objects("p", &[ObjectRef { schema: None, name: "t".into() }]);
        assert_eq!(c.len(), 1);
        c.invalidate_objects("q", &[]);
        assert!(c.is_empty());
    }
}
