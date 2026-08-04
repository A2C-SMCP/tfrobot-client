use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Deserialize;

const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Deserialize)]
struct LegacyToolDetails {
    req_id: String,
    computer_instance_id: String,
    server: String,
    tool: String,
    parameters: serde_json::Value,
    timeout: Option<f64>,
    success: bool,
    error: Option<String>,
}

pub(crate) fn migrate(conn: &mut Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if version > SCHEMA_VERSION {
        return Err(format!(
            "observability database schema {version} is newer than supported version {SCHEMA_VERSION}"
        ));
    }

    let tx = conn.transaction().map_err(|error| error.to_string())?;
    create_schema(&tx)?;

    if version < 1 && table_exists(&tx, "logs")? {
        migrate_legacy_logs(&tx)?;
    }

    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())
}

fn create_schema(tx: &Transaction<'_>) -> Result<(), String> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS activity_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            scope_kind TEXT NOT NULL CHECK (scope_kind IN ('client', 'computer')),
            computer_id TEXT,
            level TEXT NOT NULL,
            category TEXT NOT NULL,
            event_type TEXT NOT NULL,
            operation TEXT NOT NULL,
            outcome TEXT NOT NULL,
            message TEXT NOT NULL,
            fields_json TEXT,
            correlation_id TEXT,
            CHECK (
                (scope_kind = 'client' AND computer_id IS NULL) OR
                (scope_kind = 'computer' AND computer_id IS NOT NULL AND length(computer_id) > 0)
            )
        );
        CREATE INDEX IF NOT EXISTS idx_activity_timestamp ON activity_events(timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_activity_scope_timestamp
            ON activity_events(scope_kind, computer_id, timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_activity_category ON activity_events(category);

        CREATE TABLE IF NOT EXISTS tool_call_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            req_id TEXT NOT NULL,
            computer_instance_id TEXT NOT NULL,
            server TEXT NOT NULL,
            tool TEXT NOT NULL,
            parameters_json TEXT NOT NULL,
            timeout REAL,
            success INTEGER NOT NULL CHECK (success IN (0, 1)),
            error TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_tool_history_computer_timestamp
            ON tool_call_history(computer_instance_id, timestamp DESC);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_history_request
            ON tool_call_history(computer_instance_id, req_id);",
    )
    .map_err(|error| error.to_string())
}

fn table_exists(tx: &Transaction<'_>, table: &str) -> Result<bool, String> {
    tx.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |_| Ok(true),
    )
    .optional()
    .map(|value| value.unwrap_or(false))
    .map_err(|error| error.to_string())
}

fn column_exists(tx: &Transaction<'_>, column: &str) -> Result<bool, String> {
    let mut stmt = tx
        .prepare("PRAGMA table_info(logs)")
        .map_err(|error| error.to_string())?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?;
    for name in columns {
        if name.map_err(|error| error.to_string())? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_legacy_logs(tx: &Transaction<'_>) -> Result<(), String> {
    let legacy_count: i64 = tx
        .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    let computer_column = if column_exists(tx, "computer_instance_id")? {
        "computer_instance_id"
    } else {
        "NULL AS computer_instance_id"
    };
    let mut stmt = tx
        .prepare(&format!(
            "SELECT timestamp, level, category, message, details, {computer_column}
             FROM logs ORDER BY id ASC"
        ))
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(|error| error.to_string())?;

    for row in rows {
        let (timestamp, level, category, message, details, computer_id) =
            row.map_err(|error| error.to_string())?;
        let (scope_kind, computer_id_value) = match computer_id.as_deref() {
            Some(value) if !value.trim().is_empty() => ("computer", Some(value)),
            _ => ("client", None),
        };
        let outcome = if level.eq_ignore_ascii_case("error") {
            "failed"
        } else {
            "unknown"
        };
        let fields_json = details.as_ref().map(|raw| {
            serde_json::from_str::<serde_json::Value>(raw)
                .unwrap_or_else(|_| serde_json::Value::String(raw.clone()))
                .to_string()
        });
        tx.execute(
            "INSERT INTO activity_events
             (timestamp, scope_kind, computer_id, level, category, event_type, operation,
              outcome, message, fields_json)
             VALUES (?1, ?2, ?3, ?4, ?5, 'legacy_log', ?5, ?6, ?7, ?8)",
            params![
                timestamp,
                scope_kind,
                computer_id_value,
                level.to_ascii_lowercase(),
                category,
                outcome,
                message,
                fields_json
            ],
        )
        .map_err(|error| error.to_string())?;

        if category == "tool" {
            if let Some(details) = details.as_deref() {
                if let Ok(tool) = serde_json::from_str::<LegacyToolDetails>(details) {
                    tx.execute(
                        "INSERT OR IGNORE INTO tool_call_history
                         (timestamp, req_id, computer_instance_id, server, tool, parameters_json,
                          timeout, success, error)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            timestamp,
                            tool.req_id,
                            tool.computer_instance_id,
                            tool.server,
                            tool.tool,
                            tool.parameters.to_string(),
                            tool.timeout,
                            tool.success,
                            tool.error
                        ],
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
        }
    }
    drop(stmt);

    let migrated_count: i64 = tx
        .query_row("SELECT COUNT(*) FROM activity_events", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if migrated_count != legacy_count {
        return Err(format!(
            "legacy activity migration count mismatch: expected {legacy_count}, got {migrated_count}"
        ));
    }
    tx.execute("DROP TABLE logs", [])
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn migrates_legacy_activity_and_tool_history_atomically() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                level TEXT NOT NULL,
                category TEXT NOT NULL,
                message TEXT NOT NULL,
                details TEXT,
                computer_instance_id TEXT
            );",
        )
        .unwrap();
        let details = json!({
            "req_id": "request-1", "computer_instance_id": "computer-1",
            "server": "echo", "tool": "echo", "parameters": {"text": "hello"},
            "timeout": 3.0, "success": true, "error": null
        });
        conn.execute(
            "INSERT INTO logs(timestamp, level, category, message, details, computer_instance_id)
             VALUES('2026-01-01T00:00:00Z', 'info', 'tool', 'called', ?1, 'computer-1')",
            [details.to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO logs(timestamp, level, category, message, details, computer_instance_id)
             VALUES('2026-01-01T00:01:00Z', 'error', 'tool', 'malformed', 'not json', 'computer-1')",
            [],
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM activity_events", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM tool_call_history", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(!table_exists(&conn.transaction().unwrap(), "logs").unwrap());
    }

    #[test]
    fn migrates_legacy_schema_without_computer_scope_column() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                level TEXT NOT NULL,
                category TEXT NOT NULL,
                message TEXT NOT NULL,
                details TEXT
            );
            INSERT INTO logs(timestamp, level, category, message)
            VALUES('2025-01-01T00:00:00Z', 'info', 'system', 'legacy');",
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let scope: String = conn
            .query_row("SELECT scope_kind FROM activity_events", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(scope, "client");
    }
}
