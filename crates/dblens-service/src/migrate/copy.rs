//! 一张表的数据搬运（M5b 设计文档第 4 节）：源库一条按主键排序的流式查询（续传时从上次的主键之后读），
//! 按目标列的类型转换，参数化的多行 INSERT 按批写入、每批一个事务，提交后更新迁移记录。
//! 出错时按方案停止（这张表失败）或跳过（出错的批逐行重试，失败的行记进报告）。

use dblens_core::schema::Table;
use dblens_core::{Error, Result, Row, Value, msg};
use dblens_dialect::WriteMode;
use dblens_dialect::migrate::data::{TableRead, identity_insert_sql, keyset_params};
use dblens_driver::{Connection, ExecEvent};

use super::record::{MAX_SKIPPED_ROWS, SkippedRow, TableCopy, TableState};
use super::run::{Ctx, as_u64, scalar};
use super::values::{key_text, to_target};
use crate::export::EXPORT_LOB_LIMIT;
use crate::import::{OnError, exec};

/// 搬运一张表。返回错误时这张表停在最后提交的批之后（取消时为 `Cancelled`）。
pub(crate) async fn copy_table(ctx: &Ctx, c: &TableCopy) -> Result<()> {
    let mut src = ctx.connect_source().await?;
    src.set_lob_limit(EXPORT_LOB_LIMIT);
    let mut tgt = match ctx.connect_target().await {
        Ok(t) => t,
        Err(e) => {
            let _ = src.close().await;
            return Err(e);
        }
    };
    let result = copy(ctx, c, &mut src, &mut tgt).await;
    if result.is_err() {
        let _ = tgt.rollback().await;
    }
    let _ = src.close().await;
    let _ = tgt.close().await;
    result
}

async fn copy(
    ctx: &Ctx,
    c: &TableCopy,
    src: &mut Box<dyn Connection>,
    tgt: &mut Box<dyn Connection>,
) -> Result<()> {
    let (sd, td) = (ctx.source_dialect, ctx.target_dialect);
    let (ss, ts) = (ctx.source_schema()?, ctx.target_schema()?);
    let (state, last, batch_size, on_error) = ctx.with(|r| {
        let p = &r.tables[&c.source];
        (p.state, p.last_key.clone(), r.plan.data.batch_size.clamp(100, 100_000), r.plan.data.on_error)
    })?;
    let key_names: Vec<String> = c.key.iter().map(|&i| c.source_columns[i].clone()).collect();
    let resumable = !c.key.is_empty();
    // 第一次搬运，或没有主键的表重迁：从头开始
    let fresh = state == TableState::Pending || !resumable || last.is_none();
    if fresh {
        if state != TableState::Pending && c.append && !resumable {
            return Err(Error::invalid(msg!("migrate.no_key_append", table = c.source)));
        }
        let target_read = TableRead { schema: &ts, table: &c.target, columns: &[], key: &[], filter: None };
        if c.replace || (state != TableState::Pending && !c.append) {
            exec(tgt, &format!("DELETE FROM {}", td.qualified(Some(&ts), &c.target)), &[]).await?;
        }
        let base =
            if c.append { Some(as_u64(scalar(tgt, &target_read.count(&td), &[]).await?)) } else { None };
        ctx.with(|r| {
            if let Some(p) = r.tables.get_mut(&c.source) {
                if !(c.append && state != TableState::Pending) {
                    p.base_rows = base;
                }
                p.rows = 0;
                p.skipped = 0;
                p.last_key = None;
            }
            r.skipped_rows.retain(|s| s.table != c.source);
        })?;
    }
    ctx.with(|r| {
        if let Some(p) = r.tables.get_mut(&c.source) {
            p.state = TableState::Copying;
            p.error = None;
        }
    })?;
    ctx.save()?;
    // 达梦写入自增列要先打开开关（只对有自增列的表）
    let identity_insert = c.identity.as_ref().and_then(|_| identity_insert_sql(&td, &ts, &c.target, true));
    if let Some(sql) = &identity_insert {
        exec(tgt, sql, &[]).await?;
    }
    tgt.set_autocommit(false).await?;

    let read = TableRead {
        schema: &ss,
        table: &c.source,
        columns: &c.source_columns,
        key: &key_names,
        filter: c.filter.as_deref(),
    };
    let last = if fresh { None } else { last };
    let params = last.as_deref().map(keyset_params).unwrap_or_default();
    let table = Table { name: c.target.clone(), columns: c.target_columns.clone(), ..Default::default() };
    let names: Vec<String> = c.target_columns.iter().map(|x| x.name.clone()).collect();
    let writer = Writer { ctx, c, table: &table, names: &names, schema: &ts, on_error };
    let sql = read.select(&sd, last.as_deref());
    let mut stream = src.execute(&sql, &params).await?;
    let mut batch: Vec<Row> = Vec::with_capacity(batch_size);
    loop {
        if ctx.task.cancelled() {
            return Err(Error::Cancelled);
        }
        match stream.next(batch_size).await? {
            Some(ExecEvent::Rows(rows)) => {
                batch.extend(rows);
                if batch.len() >= batch_size {
                    writer.write(tgt, &batch).await?;
                    batch.clear();
                }
            }
            Some(_) => {}
            None => break,
        }
    }
    drop(stream);
    if !batch.is_empty() {
        writer.write(tgt, &batch).await?;
    }
    tgt.set_autocommit(true).await?;
    if identity_insert.is_some()
        && let Some(sql) = identity_insert_sql(&td, &ts, &c.target, false)
    {
        exec(tgt, &sql, &[]).await?;
    }
    Ok(())
}

struct Writer<'a> {
    ctx: &'a Ctx,
    c: &'a TableCopy,
    table: &'a Table,
    names: &'a [String],
    schema: &'a str,
    on_error: OnError,
}

impl Writer<'_> {
    fn convert(&self, row: &Row) -> Result<Vec<Value>> {
        row.iter().zip(&self.c.target_columns).map(|(v, col)| to_target(v, col)).collect()
    }

    fn key(&self, row: &Row) -> Vec<Value> {
        self.c.key.iter().map(|&i| row[i].clone()).collect()
    }

    /// 写一组行（一个事务）。
    async fn insert(&self, tgt: &mut Box<dyn Connection>, rows: &[Vec<Value>]) -> Result<()> {
        let td = self.ctx.target_dialect;
        let result = async {
            for s in td.import_statements(self.schema, self.table, self.names, rows, WriteMode::Append)? {
                exec(tgt, &s.sql, &s.params).await?;
            }
            tgt.commit().await
        }
        .await;
        if result.is_err() {
            let _ = tgt.rollback().await;
        }
        result
    }

    /// 写一批：成功后记下行数和最后的主键。跳过模式下出错的批逐行重试。
    async fn write(&self, tgt: &mut Box<dyn Connection>, batch: &[Row]) -> Result<()> {
        let converted: Result<Vec<Vec<Value>>> = batch.iter().map(|r| self.convert(r)).collect();
        let whole = match converted {
            Ok(rows) => self.insert(tgt, &rows).await,
            Err(e) => Err(e),
        };
        let (written, skipped) = match whole {
            Ok(()) => (batch.len() as u64, vec![]),
            Err(e) if self.on_error == OnError::Stop => return Err(e),
            Err(_) => {
                let mut written = 0;
                let mut skipped = Vec::new();
                for row in batch {
                    let r = match self.convert(row) {
                        Ok(v) => self.insert(tgt, &[v]).await,
                        Err(e) => Err(e),
                    };
                    match r {
                        Ok(()) => written += 1,
                        Err(e) => skipped.push(SkippedRow {
                            table: self.c.source.clone(),
                            key: key_text(&self.key(row)),
                            error: e.to_string(),
                        }),
                    }
                }
                (written, skipped)
            }
        };
        let last = batch.last().map(|r| self.key(r));
        self.ctx.with(|r| {
            if let Some(p) = r.tables.get_mut(&self.c.source) {
                p.rows += written;
                p.skipped += skipped.len() as u64;
                if !self.c.key.is_empty() {
                    p.last_key = last;
                }
            }
            let room = MAX_SKIPPED_ROWS.saturating_sub(r.skipped_rows.len());
            r.skipped_rows.extend(skipped.into_iter().take(room));
        })?;
        self.ctx.save()
    }
}
