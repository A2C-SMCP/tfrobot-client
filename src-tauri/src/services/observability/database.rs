use super::{
    migration, ActivityEvent, ActivityEventDraft, ActivityLevel, ActivityOutcome, ActivityPage,
    ActivityQuery, ActivityScope, ActivityScopeFilter, ToolCallHistoryDraft, ToolCallHistoryRecord,
};
use chrono::{Duration, Utc};
use rusqlite::{
    params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension, Row,
};
use std::path::Path;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

#[derive(Debug, Clone, Copy)]
pub struct ObservabilityRetention {
    pub activity_days: u32,
    pub tool_history_days: u32,
}

#[derive(Clone)]
pub struct ObservabilityService {
    conn: Arc<Mutex<Connection>>,
    retention: Arc<RetentionState>,
}

struct RetentionState {
    activity_days: AtomicU32,
    tool_history_days: AtomicU32,
    next_cleanup_at: AtomicI64,
}

impl ObservabilityService {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
        let mut conn =
            Connection::open(data_dir.join("logs.db")).map_err(|error| error.to_string())?;
        conn.busy_timeout(StdDuration::from_secs(5))
            .map_err(|error| error.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|error| error.to_string())?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|error| error.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|error| error.to_string())?;
        migration::migrate(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            retention: Arc::new(RetentionState {
                activity_days: AtomicU32::new(30),
                tool_history_days: AtomicU32::new(90),
                // Retention is disabled until persisted settings are applied. Running cleanup with
                // defaults first could irreversibly delete data for a user configured to retain it.
                next_cleanup_at: AtomicI64::new(i64::MAX),
            }),
        })
    }

    pub fn record_activity(&self, draft: &ActivityEventDraft) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let id = insert_activity(&conn, &Utc::now().to_rfc3339(), draft)?;
        drop(conn);
        self.maybe_cleanup_due();
        Ok(id)
    }

    pub async fn record_activity_async(&self, draft: ActivityEventDraft) -> Result<i64, String> {
        let service = self.clone();
        tauri::async_runtime::spawn_blocking(move || service.record_activity(&draft))
            .await
            .map_err(|error| error.to_string())?
    }

    /// Opens a durable client run and records startup in the same transaction.
    ///
    /// A surviving row identifies a previous run that never reached the graceful shutdown
    /// commit point. The marker is deliberately independent from activity retention and manual
    /// clearing so those operations cannot manufacture or hide an unclean-exit signal.
    pub fn begin_client_run(&self, run_id: &str, app_version: &str) -> Result<bool, String> {
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let previous = tx
            .query_row(
                "SELECT run_id, started_at, app_version FROM client_run_state WHERE singleton_id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let detected_unclean_exit = previous.is_some();
        let timestamp = Utc::now().to_rfc3339();

        if let Some((previous_run_id, previous_started_at, previous_app_version)) = previous {
            let mut activity = ActivityEventDraft::client(
                ActivityLevel::Warn,
                "system",
                "application_lifecycle",
                "unclean_exit",
                ActivityOutcome::Unknown,
                "Previous application run did not shut down cleanly",
            );
            activity.fields = Some(serde_json::json!({
                "app_version": app_version,
                "trigger": "system",
                "previous_run_id": previous_run_id,
                "previous_started_at": previous_started_at,
                "previous_app_version": previous_app_version,
            }));
            activity.correlation_id = Some(previous_run_id);
            insert_activity(&tx, &timestamp, &activity)?;
        }

        tx.execute(
            "INSERT INTO client_run_state (singleton_id, run_id, started_at, app_version)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(singleton_id) DO UPDATE SET
                 run_id = excluded.run_id,
                 started_at = excluded.started_at,
                 app_version = excluded.app_version",
            params![run_id, timestamp, app_version],
        )
        .map_err(|error| error.to_string())?;
        let mut activity = ActivityEventDraft::client(
            ActivityLevel::Info,
            "system",
            "application_lifecycle",
            "start",
            ActivityOutcome::Succeeded,
            "Application started",
        );
        activity.fields = Some(serde_json::json!({
            "app_version": app_version,
            "trigger": "system",
            "run_id": run_id,
        }));
        activity.correlation_id = Some(run_id.to_string());
        insert_activity(&tx, &timestamp, &activity)?;
        tx.commit().map_err(|error| error.to_string())?;
        drop(conn);
        self.maybe_cleanup_due();
        Ok(detected_unclean_exit)
    }

    /// Closes the current client run exactly once and records the matching shutdown activity.
    pub fn finish_client_run(&self, run_id: &str, app_version: &str) -> Result<bool, String> {
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let current_run_id = tx
            .query_row(
                "SELECT run_id FROM client_run_state WHERE singleton_id = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if current_run_id.as_deref() != Some(run_id) {
            return Ok(false);
        }

        let timestamp = Utc::now().to_rfc3339();
        let mut activity = ActivityEventDraft::client(
            ActivityLevel::Info,
            "system",
            "application_lifecycle",
            "shutdown",
            ActivityOutcome::Succeeded,
            "Application shutting down",
        );
        activity.fields = Some(serde_json::json!({
            "app_version": app_version,
            "trigger": "system",
            "run_id": run_id,
        }));
        activity.correlation_id = Some(run_id.to_string());
        insert_activity(&tx, &timestamp, &activity)?;
        tx.execute(
            "DELETE FROM client_run_state WHERE singleton_id = 1 AND run_id = ?1",
            [run_id],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(true)
    }

    pub async fn finish_client_run_async(
        &self,
        run_id: String,
        app_version: String,
    ) -> Result<bool, String> {
        let service = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            service.finish_client_run(&run_id, &app_version)
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub fn record_tool_call(
        &self,
        activity: &ActivityEventDraft,
        history: &ToolCallHistoryDraft,
    ) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let timestamp = Utc::now().to_rfc3339();
        insert_activity(&tx, &timestamp, activity)?;
        tx.execute(
            "INSERT INTO tool_call_history
             (timestamp, req_id, computer_instance_id, server, tool, parameters_json, timeout,
              success, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                timestamp,
                history.req_id,
                history.computer_instance_id,
                history.server,
                history.tool,
                history.parameters.to_string(),
                history.timeout,
                history.success,
                history.error
            ],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        drop(conn);
        self.maybe_cleanup_due();
        Ok(())
    }

    pub async fn record_tool_call_async(
        &self,
        activity: ActivityEventDraft,
        history: ToolCallHistoryDraft,
    ) -> Result<(), String> {
        let service = self.clone();
        tauri::async_runtime::spawn_blocking(move || service.record_tool_call(&activity, &history))
            .await
            .map_err(|error| error.to_string())?
    }

    pub fn query_activity(&self, query: &ActivityQuery) -> Result<ActivityPage, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let (where_sql, values) = activity_where_clause(query);
        let total: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM activity_events {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let limit = query.limit.unwrap_or(50).clamp(1, 500);
        let offset = query.offset.unwrap_or(0).max(0);
        let mut page_values = values;
        page_values.push(SqlValue::Integer(limit));
        page_values.push(SqlValue::Integer(offset));
        let sql = format!(
            "SELECT id, timestamp, scope_kind, computer_id, level, category, event_type,
                    operation, outcome, message, fields_json, correlation_id
             FROM activity_events {where_sql}
             ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?"
        );
        let mut stmt = conn.prepare(&sql).map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params_from_iter(page_values.iter()), activity_from_row)
            .map_err(|error| error.to_string())?;
        let items = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(ActivityPage {
            items,
            total,
            limit,
            offset,
        })
    }

    pub async fn query_activity_async(&self, query: ActivityQuery) -> Result<ActivityPage, String> {
        let service = self.clone();
        tauri::async_runtime::spawn_blocking(move || service.query_activity(&query))
            .await
            .map_err(|error| error.to_string())?
    }

    pub fn export_activity(&self, query: &ActivityQuery) -> Result<String, String> {
        self.export_activity_with_count(query).map(|(json, _)| json)
    }

    pub fn export_activity_with_count(
        &self,
        query: &ActivityQuery,
    ) -> Result<(String, usize), String> {
        let mut export_query = query.clone();
        export_query.limit = Some(500);
        export_query.offset = Some(0);
        let mut items = Vec::new();
        loop {
            let page = self.query_activity(&export_query)?;
            items.extend(page.items);
            if items.len() as i64 >= page.total {
                break;
            }
            export_query.offset = Some(items.len() as i64);
        }
        let count = items.len();
        serde_json::to_string_pretty(&items)
            .map(|json| (json, count))
            .map_err(|error| error.to_string())
    }

    pub fn clear_activity(&self, scope: &ActivityScopeFilter) -> Result<u64, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let (sql, values) = activity_clear_statement(scope);
        conn.execute(&sql, params_from_iter(values.iter()))
            .map(|count| count as u64)
            .map_err(|error| error.to_string())
    }

    pub fn clear_activity_with_audit(
        &self,
        scope: &ActivityScopeFilter,
        mut audit: ActivityEventDraft,
    ) -> Result<u64, String> {
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let (sql, values) = activity_clear_statement(scope);
        let count = tx
            .execute(&sql, params_from_iter(values.iter()))
            .map_err(|error| error.to_string())? as u64;
        let fields = audit.fields.get_or_insert_with(|| serde_json::json!({}));
        if let Some(object) = fields.as_object_mut() {
            object.insert("deleted_count".to_string(), serde_json::json!(count));
        }
        insert_activity(&tx, &Utc::now().to_rfc3339(), &audit)?;
        tx.commit().map_err(|error| error.to_string())?;
        drop(conn);
        self.maybe_cleanup_due();
        Ok(count)
    }

    pub fn tool_history(
        &self,
        computer_instance_id: &str,
        limit: i64,
    ) -> Result<Vec<ToolCallHistoryRecord>, String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT timestamp, req_id, computer_instance_id, server, tool, parameters_json,
                        timeout, success, error
                 FROM tool_call_history WHERE computer_instance_id = ?1
                 ORDER BY timestamp DESC, id DESC LIMIT ?2",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![computer_instance_id, limit.clamp(1, 500)], |row| {
                let parameters: String = row.get(5)?;
                Ok(ToolCallHistoryRecord {
                    timestamp: row.get(0)?,
                    req_id: row.get(1)?,
                    computer_instance_id: row.get(2)?,
                    server: row.get(3)?,
                    tool: row.get(4)?,
                    parameters: serde_json::from_str(&parameters)
                        .unwrap_or(serde_json::Value::Null),
                    timeout: row.get(6)?,
                    success: row.get(7)?,
                    error: row.get(8)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn cleanup(&self, retention: ObservabilityRetention) -> Result<(u64, u64), String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let activity_cutoff =
            (Utc::now() - Duration::days(retention.activity_days as i64)).to_rfc3339();
        let tool_cutoff =
            (Utc::now() - Duration::days(retention.tool_history_days as i64)).to_rfc3339();
        let activity = conn
            .execute(
                "DELETE FROM activity_events WHERE timestamp < ?1",
                [activity_cutoff],
            )
            .map_err(|error| error.to_string())? as u64;
        let tools = conn
            .execute(
                "DELETE FROM tool_call_history WHERE timestamp < ?1",
                [tool_cutoff],
            )
            .map_err(|error| error.to_string())? as u64;
        Ok((activity, tools))
    }

    /// Applies retention immediately and arms event-triggered maintenance. There is deliberately
    /// no polling task: subsequent writes perform at most one cleanup per hour.
    pub fn apply_retention(&self, retention: ObservabilityRetention) -> Result<(u64, u64), String> {
        let retention = ObservabilityRetention {
            activity_days: retention.activity_days.max(1),
            tool_history_days: retention.tool_history_days.max(1),
        };
        self.retention
            .activity_days
            .store(retention.activity_days, Ordering::Relaxed);
        self.retention
            .tool_history_days
            .store(retention.tool_history_days, Ordering::Relaxed);
        let result = self.cleanup(retention)?;
        self.retention
            .next_cleanup_at
            .store(Utc::now().timestamp() + 60 * 60, Ordering::Relaxed);
        Ok(result)
    }

    fn maybe_cleanup_due(&self) {
        let now = Utc::now().timestamp();
        let due = self.retention.next_cleanup_at.load(Ordering::Relaxed);
        if due > now
            || self
                .retention
                .next_cleanup_at
                .compare_exchange(due, now + 60 * 60, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        let retention = ObservabilityRetention {
            activity_days: self.retention.activity_days.load(Ordering::Relaxed),
            tool_history_days: self.retention.tool_history_days.load(Ordering::Relaxed),
        };
        if let Err(error) = self.cleanup(retention) {
            log::error!("event-triggered observability cleanup failed: {error}");
        }
    }
}

fn activity_clear_statement(scope: &ActivityScopeFilter) -> (String, Vec<SqlValue>) {
    match scope {
        ActivityScopeFilter::All => ("DELETE FROM activity_events".to_string(), vec![]),
        ActivityScopeFilter::ClientOnly => (
            "DELETE FROM activity_events WHERE scope_kind = 'client'".to_string(),
            vec![],
        ),
        ActivityScopeFilter::Computer { computer_id } => (
            "DELETE FROM activity_events WHERE scope_kind = 'computer' AND computer_id = ?1"
                .to_string(),
            vec![SqlValue::Text(computer_id.clone())],
        ),
    }
}

fn insert_activity(
    conn: &Connection,
    timestamp: &str,
    draft: &ActivityEventDraft,
) -> Result<i64, String> {
    let (scope_kind, computer_id) = match &draft.scope {
        ActivityScope::Client => ("client", None),
        ActivityScope::Computer { computer_id } => ("computer", Some(computer_id.as_str())),
    };
    let fields_json = draft.fields.as_ref().map(serde_json::Value::to_string);
    conn.execute(
        "INSERT INTO activity_events
         (timestamp, scope_kind, computer_id, level, category, event_type, operation, outcome,
          message, fields_json, correlation_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            timestamp,
            scope_kind,
            computer_id,
            draft.level.as_str(),
            draft.category,
            draft.event_type,
            draft.operation,
            draft.outcome.as_str(),
            draft.message,
            fields_json,
            draft.correlation_id
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(conn.last_insert_rowid())
}

fn activity_where_clause(query: &ActivityQuery) -> (String, Vec<SqlValue>) {
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    if let Some(start) = query.start_time.as_ref() {
        clauses.push("timestamp >= ?");
        values.push(SqlValue::Text(start.clone()));
    }
    if let Some(end) = query.end_time.as_ref() {
        clauses.push("timestamp <= ?");
        values.push(SqlValue::Text(end.clone()));
    }
    if let Some(levels) = query.levels.as_ref().filter(|values| !values.is_empty()) {
        clauses.push("level IN (SELECT value FROM json_each(?))");
        values.push(SqlValue::Text(
            serde_json::to_string(&levels.iter().map(ActivityLevel::as_str).collect::<Vec<_>>())
                .unwrap_or_else(|_| "[]".to_string()),
        ));
    }
    if let Some(categories) = query
        .categories
        .as_ref()
        .filter(|values| !values.is_empty())
    {
        clauses.push("category IN (SELECT value FROM json_each(?))");
        values.push(SqlValue::Text(
            serde_json::to_string(categories).unwrap_or_else(|_| "[]".to_string()),
        ));
    }
    if let Some(keyword) = query.keyword.as_ref().filter(|value| !value.is_empty()) {
        clauses.push("(message LIKE ? ESCAPE '\\' OR operation LIKE ? ESCAPE '\\')");
        let escaped = keyword
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        values.push(SqlValue::Text(format!("%{escaped}%")));
        values.push(SqlValue::Text(format!("%{escaped}%")));
    }
    match &query.scope {
        ActivityScopeFilter::All => {}
        ActivityScopeFilter::ClientOnly => clauses.push("scope_kind = 'client'"),
        ActivityScopeFilter::Computer { computer_id } => {
            clauses.push("scope_kind = 'computer' AND computer_id = ?");
            values.push(SqlValue::Text(computer_id.clone()));
        }
    }
    if clauses.is_empty() {
        (String::new(), values)
    } else {
        (format!("WHERE {}", clauses.join(" AND ")), values)
    }
}

fn activity_from_row(row: &Row<'_>) -> rusqlite::Result<ActivityEvent> {
    let scope_kind: String = row.get(2)?;
    let computer_id: Option<String> = row.get(3)?;
    let fields_json: Option<String> = row.get(10)?;
    Ok(ActivityEvent {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        scope: if scope_kind == "computer" {
            ActivityScope::Computer {
                computer_id: computer_id.unwrap_or_default(),
            }
        } else {
            ActivityScope::Client
        },
        level: ActivityLevel::from_db(row.get(4)?),
        category: row.get(5)?,
        event_type: row.get(6)?,
        operation: row.get(7)?,
        outcome: ActivityOutcome::from_db(row.get(8)?),
        message: row.get(9)?,
        fields: fields_json.and_then(|value| serde_json::from_str(&value).ok()),
        correlation_id: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn activity_query_has_real_total_and_explicit_scope() {
        let dir = tempdir().unwrap();
        let service = ObservabilityService::new(dir.path()).unwrap();
        service
            .record_activity(&ActivityEventDraft::client(
                ActivityLevel::Info,
                "system",
                "lifecycle",
                "start",
                ActivityOutcome::Succeeded,
                "started",
            ))
            .unwrap();
        service
            .record_activity(&ActivityEventDraft::computer(
                "computer-1",
                ActivityLevel::Error,
                "mcp",
                "server",
                "start",
                ActivityOutcome::Failed,
                "failed",
            ))
            .unwrap();

        let page = service
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::ClientOnly,
                limit: Some(1),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].scope, ActivityScope::Client);
    }

    #[test]
    fn clearing_activity_does_not_clear_tool_history() {
        let dir = tempdir().unwrap();
        let service = ObservabilityService::new(dir.path()).unwrap();
        let activity = ActivityEventDraft::computer(
            "computer-1",
            ActivityLevel::Info,
            "tool",
            "tool_call",
            "echo",
            ActivityOutcome::Succeeded,
            "called",
        );
        service
            .record_tool_call(
                &activity,
                &ToolCallHistoryDraft {
                    req_id: "request-1".into(),
                    computer_instance_id: "computer-1".into(),
                    server: "echo".into(),
                    tool: "echo".into(),
                    parameters: serde_json::json!({"text": "hello"}),
                    timeout: None,
                    success: true,
                    error: None,
                },
            )
            .unwrap();
        service.clear_activity(&ActivityScopeFilter::All).unwrap();
        assert_eq!(service.tool_history("computer-1", 100).unwrap().len(), 1);
    }

    #[test]
    fn client_run_marker_detects_unclean_exit_independently_from_activity_history() {
        let dir = tempdir().unwrap();
        let service = ObservabilityService::new(dir.path()).unwrap();

        assert!(!service.begin_client_run("run-1", "1.0.0").unwrap());
        service.clear_activity(&ActivityScopeFilter::All).unwrap();
        assert!(service.begin_client_run("run-2", "1.0.1").unwrap());

        let page = service.query_activity(&ActivityQuery::default()).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.items[0].operation, "start");
        assert_eq!(page.items[0].correlation_id.as_deref(), Some("run-2"));
        assert_eq!(page.items[1].operation, "unclean_exit");
        assert_eq!(page.items[1].outcome, ActivityOutcome::Unknown);
        assert_eq!(
            page.items[1]
                .fields
                .as_ref()
                .and_then(|fields| fields["previous_run_id"].as_str()),
            Some("run-1")
        );
    }

    #[test]
    fn graceful_client_run_finish_is_idempotent_and_prevents_false_recovery() {
        let dir = tempdir().unwrap();
        let service = ObservabilityService::new(dir.path()).unwrap();

        assert!(!service.begin_client_run("run-1", "1.0.0").unwrap());
        assert!(service.finish_client_run("run-1", "1.0.0").unwrap());
        assert!(!service.finish_client_run("run-1", "1.0.0").unwrap());
        assert!(!service.begin_client_run("run-2", "1.0.1").unwrap());

        let page = service.query_activity(&ActivityQuery::default()).unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(
            page.items
                .iter()
                .filter(|event| event.operation == "shutdown")
                .count(),
            1
        );
        assert!(page
            .items
            .iter()
            .all(|event| event.operation != "unclean_exit"));
    }

    #[test]
    fn schema_v1_is_upgraded_with_durable_client_run_state() {
        let dir = tempdir().unwrap();
        let database_path = dir.path().join("logs.db");
        drop(ObservabilityService::new(dir.path()).unwrap());
        let conn = Connection::open(&database_path).unwrap();
        conn.execute("DROP TABLE client_run_state", []).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        drop(conn);

        let service = ObservabilityService::new(dir.path()).unwrap();
        assert!(!service
            .begin_client_run("run-after-upgrade", "1.0.0")
            .unwrap());
        assert!(service
            .query_activity(&ActivityQuery::default())
            .unwrap()
            .items
            .iter()
            .any(|event| event.operation == "start"));
    }

    #[test]
    fn audited_clear_commits_deletion_and_audit_together() {
        let dir = tempdir().unwrap();
        let service = ObservabilityService::new(dir.path()).unwrap();
        service
            .record_activity(&ActivityEventDraft::client(
                ActivityLevel::Info,
                "system",
                "lifecycle",
                "start",
                ActivityOutcome::Succeeded,
                "started",
            ))
            .unwrap();
        let mut audit = ActivityEventDraft::client(
            ActivityLevel::Info,
            "observability",
            "activity_journal",
            "clear",
            ActivityOutcome::Succeeded,
            "Activity cleared",
        );
        audit.fields = Some(serde_json::json!({"scope": {"kind": "all"}}));

        assert_eq!(
            service
                .clear_activity_with_audit(&ActivityScopeFilter::All, audit)
                .unwrap(),
            1
        );
        let page = service.query_activity(&ActivityQuery::default()).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].operation, "clear");
        assert_eq!(
            page.items[0]
                .fields
                .as_ref()
                .and_then(|fields| fields["deleted_count"].as_u64()),
            Some(1)
        );
    }

    #[test]
    fn tool_history_and_activity_roll_back_together() {
        let dir = tempdir().unwrap();
        let service = ObservabilityService::new(dir.path()).unwrap();
        let activity = ActivityEventDraft::computer(
            "computer-1",
            ActivityLevel::Info,
            "tool",
            "tool_call",
            "echo",
            ActivityOutcome::Succeeded,
            "called",
        );
        let history = ToolCallHistoryDraft {
            req_id: "duplicate-request".into(),
            computer_instance_id: "computer-1".into(),
            server: "echo".into(),
            tool: "echo".into(),
            parameters: serde_json::json!({}),
            timeout: None,
            success: true,
            error: None,
        };
        service.record_tool_call(&activity, &history).unwrap();
        assert!(service.record_tool_call(&activity, &history).is_err());

        let page = service.query_activity(&ActivityQuery::default()).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(service.tool_history("computer-1", 100).unwrap().len(), 1);
    }

    #[test]
    fn zero_day_retention_is_clamped_before_cleanup() {
        let dir = tempdir().unwrap();
        let service = ObservabilityService::new(dir.path()).unwrap();
        service
            .record_activity(&ActivityEventDraft::client(
                ActivityLevel::Info,
                "system",
                "lifecycle",
                "start",
                ActivityOutcome::Succeeded,
                "started",
            ))
            .unwrap();

        service
            .apply_retention(ObservabilityRetention {
                activity_days: 0,
                tool_history_days: 0,
            })
            .unwrap();

        assert_eq!(
            service
                .query_activity(&ActivityQuery::default())
                .unwrap()
                .total,
            1
        );
    }
}
