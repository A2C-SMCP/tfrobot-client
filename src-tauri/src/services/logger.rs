use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub timestamp: String,
    pub level: String,
    pub category: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogFilter {
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub levels: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
    pub keyword: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub struct LogService {
    conn: Mutex<Connection>,
}

impl LogService {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        let db_path = data_dir.join("logs.db");
        let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                level TEXT NOT NULL DEFAULT 'info',
                category TEXT NOT NULL DEFAULT 'system',
                message TEXT NOT NULL,
                details TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp);
            CREATE INDEX IF NOT EXISTS idx_logs_level ON logs(level);
            CREATE INDEX IF NOT EXISTS idx_logs_category ON logs(category);",
        )
        .map_err(|e| e.to_string())?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn write(
        &self,
        level: &str,
        category: &str,
        message: &str,
        details: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO logs (timestamp, level, category, message, details) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now, level, category, message, details],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn query(&self, filter: &LogFilter) -> Result<Vec<LogEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut sql = String::from(
            "SELECT id, timestamp, level, category, message, details FROM logs WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref start) = filter.start_time {
            sql.push_str(" AND timestamp >= ?");
            param_values.push(Box::new(start.clone()));
        }
        if let Some(ref end) = filter.end_time {
            sql.push_str(" AND timestamp <= ?");
            param_values.push(Box::new(end.clone()));
        }
        if let Some(ref levels) = filter.levels {
            if !levels.is_empty() {
                let placeholders: Vec<String> =
                    levels.iter().enumerate().map(|_| "?".to_string()).collect();
                sql.push_str(&format!(" AND level IN ({})", placeholders.join(",")));
                for l in levels {
                    param_values.push(Box::new(l.clone()));
                }
            }
        }
        if let Some(ref categories) = filter.categories {
            if !categories.is_empty() {
                let placeholders: Vec<String> = categories
                    .iter()
                    .enumerate()
                    .map(|_| "?".to_string())
                    .collect();
                sql.push_str(&format!(" AND category IN ({})", placeholders.join(",")));
                for c in categories {
                    param_values.push(Box::new(c.clone()));
                }
            }
        }
        if let Some(ref keyword) = filter.keyword {
            if !keyword.is_empty() {
                sql.push_str(" AND message LIKE ?");
                param_values.push(Box::new(format!("%{}%", keyword)));
            }
        }

        sql.push_str(" ORDER BY timestamp DESC");

        let limit = filter.limit.unwrap_or(100);
        let offset = filter.offset.unwrap_or(0);
        sql.push_str(" LIMIT ? OFFSET ?");
        param_values.push(Box::new(limit));
        param_values.push(Box::new(offset));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(LogEntry {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    level: row.get(2)?,
                    category: row.get(3)?,
                    message: row.get(4)?,
                    details: row.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|e| e.to_string())?);
        }
        Ok(entries)
    }

    pub fn cleanup(&self, days: i64) -> Result<u64, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let cutoff = (Utc::now() - chrono::Duration::days(days)).to_rfc3339();
        let count = conn
            .execute("DELETE FROM logs WHERE timestamp < ?1", params![cutoff])
            .map_err(|e| e.to_string())?;
        Ok(count as u64)
    }

    pub fn export(&self, filter: &LogFilter) -> Result<String, String> {
        let entries = self.query(filter)?;
        serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())
    }

    pub fn clear_all(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM logs", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (LogService, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let svc = LogService::new(tmp.path()).unwrap();
        (svc, tmp)
    }

    #[test]
    fn test_write_and_query_log() {
        let (svc, _tmp) = setup();
        svc.write("info", "test", "hello world", None).unwrap();

        let logs = svc.query(&LogFilter::default()).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "hello world");
        assert_eq!(logs[0].level, "info");
        assert_eq!(logs[0].category, "test");
    }

    #[test]
    fn test_query_with_level_filter() {
        let (svc, _tmp) = setup();
        svc.write("info", "cat", "msg1", None).unwrap();
        svc.write("error", "cat", "msg2", None).unwrap();

        let filter = LogFilter {
            levels: Some(vec!["error".to_string()]),
            ..Default::default()
        };
        let logs = svc.query(&filter).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "msg2");
    }

    #[test]
    fn test_query_with_category_filter() {
        let (svc, _tmp) = setup();
        svc.write("info", "mcp", "msg1", None).unwrap();
        svc.write("info", "smcp", "msg2", None).unwrap();

        let filter = LogFilter {
            categories: Some(vec!["mcp".to_string()]),
            ..Default::default()
        };
        let logs = svc.query(&filter).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "msg1");
    }

    #[test]
    fn test_query_with_keyword_filter() {
        let (svc, _tmp) = setup();
        svc.write("info", "cat", "hello world", None).unwrap();
        svc.write("info", "cat", "goodbye", None).unwrap();

        let filter = LogFilter {
            keyword: Some("hello".to_string()),
            ..Default::default()
        };
        let logs = svc.query(&filter).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "hello world");
    }

    #[test]
    fn test_query_with_limit_offset() {
        let (svc, _tmp) = setup();
        for i in 0..20 {
            svc.write("info", "cat", &format!("msg{i}"), None).unwrap();
        }

        let filter = LogFilter {
            limit: Some(5),
            offset: Some(10),
            ..Default::default()
        };
        let logs = svc.query(&filter).unwrap();
        assert_eq!(logs.len(), 5);
    }

    #[test]
    fn test_clear_all() {
        let (svc, _tmp) = setup();
        svc.write("info", "cat", "msg", None).unwrap();
        svc.clear_all().unwrap();
        let logs = svc.query(&LogFilter::default()).unwrap();
        assert!(logs.is_empty());
    }

    #[test]
    fn test_export_returns_json() {
        let (svc, _tmp) = setup();
        svc.write("info", "cat", "msg", None).unwrap();
        let json = svc.export(&LogFilter::default()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_write_with_details() {
        let (svc, _tmp) = setup();
        svc.write("error", "cat", "msg", Some("stack trace here"))
            .unwrap();
        let logs = svc.query(&LogFilter::default()).unwrap();
        assert_eq!(logs[0].details.as_deref(), Some("stack trace here"));
    }

    #[test]
    fn test_cleanup_old_logs() {
        let (svc, _tmp) = setup();
        svc.write("info", "cat", "recent msg", None).unwrap();
        // cleanup with 0 days should delete everything (cutoff = now)
        let deleted = svc.cleanup(0).unwrap();
        assert!(deleted >= 1);
        let logs = svc.query(&LogFilter::default()).unwrap();
        assert!(logs.is_empty());
    }

    #[test]
    fn test_query_empty_db() {
        let (svc, _tmp) = setup();
        let logs = svc.query(&LogFilter::default()).unwrap();
        assert!(logs.is_empty());
    }
}
