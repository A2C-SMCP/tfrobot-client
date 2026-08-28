use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry};

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl DiagnosticLevel {
    fn value(self) -> u8 {
        match self {
            Self::Error => 1,
            Self::Warn => 2,
            Self::Info => 3,
            Self::Debug => 4,
            Self::Trace => 5,
        }
    }
}

pub struct Diagnostics {
    level: AtomicU8,
}

impl Diagnostics {
    pub fn new(level: DiagnosticLevel) -> Self {
        Self {
            level: AtomicU8::new(level.value()),
        }
    }

    pub fn level(&self) -> DiagnosticLevel {
        match self.level.load(Ordering::Relaxed) {
            1 => DiagnosticLevel::Error,
            2 => DiagnosticLevel::Warn,
            4 => DiagnosticLevel::Debug,
            5 => DiagnosticLevel::Trace,
            _ => DiagnosticLevel::Info,
        }
    }

    pub fn set_level(&self, level: DiagnosticLevel) {
        self.level.store(level.value(), Ordering::Relaxed);
        log::set_max_level(match level {
            DiagnosticLevel::Error => log::LevelFilter::Error,
            DiagnosticLevel::Warn => log::LevelFilter::Warn,
            DiagnosticLevel::Info => log::LevelFilter::Info,
            DiagnosticLevel::Debug => log::LevelFilter::Debug,
            DiagnosticLevel::Trace => log::LevelFilter::Trace,
        });
    }
}

/// Installs the structured Rust diagnostics pipeline. The Tauri log plugin remains the sink for
/// stdout, rotating files, and Webview delivery; this layer bridges structured tracing events to
/// that sink without mixing them into the activity database.
pub fn initialize_tracing() -> Result<(), String> {
    tracing::subscriber::set_global_default(Registry::default().with(TauriLogLayer))
        .map_err(|error| error.to_string())
}

struct TauriLogLayer;

impl<S> Layer<S> for TauriLogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = DiagnosticVisitor::default();
        event.record(&mut visitor);
        let message = visitor.finish_redacted();
        match *metadata.level() {
            tracing::Level::ERROR => log::error!(target: metadata.target(), "{message}"),
            tracing::Level::WARN => log::warn!(target: metadata.target(), "{message}"),
            tracing::Level::INFO => log::info!(target: metadata.target(), "{message}"),
            tracing::Level::DEBUG => log::debug!(target: metadata.target(), "{message}"),
            tracing::Level::TRACE => log::trace!(target: metadata.target(), "{message}"),
        }
    }
}

#[derive(Default)]
struct DiagnosticVisitor {
    message: Option<String>,
    fields: Vec<String>,
}

impl DiagnosticVisitor {
    fn finish_redacted(self) -> String {
        let rendered = match (self.message, self.fields.is_empty()) {
            (Some(message), true) => message,
            (Some(message), false) => format!("{message} {}", self.fields.join(" ")),
            (None, _) => self.fields.join(" "),
        };
        super::redact_text(&rendered)
    }
}

impl Visit for DiagnosticVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}").trim_matches('"').to_string());
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_level_round_trips() {
        let diagnostics = Diagnostics::new(DiagnosticLevel::Info);
        diagnostics.set_level(DiagnosticLevel::Debug);
        assert_eq!(diagnostics.level(), DiagnosticLevel::Debug);
    }

    #[test]
    fn tracing_bridge_redacts_sensitive_structured_fields() {
        let visitor = DiagnosticVisitor {
            message: Some("request failed".to_string()),
            fields: vec!["token=\"secret-value\"".to_string()],
        };

        assert_eq!(
            visitor.finish_redacted(),
            "request failed token= [REDACTED]"
        );
    }
}
