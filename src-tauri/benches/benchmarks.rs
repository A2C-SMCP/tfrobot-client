use a2c_smcp::smcp_computer::settings::config::{
    ConfigEdit, ConfigEntity, EditIntent, ProjectConfigDoc,
};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::Arc;
use tempfile::tempdir;
use tfrobot_client_lib::services::computer::ComputerInstance;
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::observability::{
    ActivityEventDraft, ActivityLevel, ActivityOutcome, ActivityQuery, ObservabilityRetention,
    ObservabilityService,
};
use tfrobot_client_lib::services::sdk_config::SdkConfigService;

fn benchmark_activity(message: impl Into<String>) -> ActivityEventDraft {
    ActivityEventDraft::client(
        ActivityLevel::Info,
        "bench",
        "benchmark",
        "write",
        ActivityOutcome::Succeeded,
        message,
    )
}

// ── Activity journal benchmarks ──

fn bench_log_write(c: &mut Criterion) {
    let tmp = tempdir().unwrap();
    let svc = ObservabilityService::new(tmp.path()).unwrap();

    c.bench_function("log_write_single", |b| {
        b.iter(|| {
            svc.record_activity(&benchmark_activity("benchmark message"))
                .unwrap();
        });
    });
}

fn bench_log_query(c: &mut Criterion) {
    let tmp = tempdir().unwrap();
    let svc = ObservabilityService::new(tmp.path()).unwrap();

    // Pre-fill data
    for i in 0..10_000 {
        svc.record_activity(&benchmark_activity(format!("msg {i}")))
            .unwrap();
    }

    let mut group = c.benchmark_group("log_query");

    for limit in [10, 100, 1000] {
        group.bench_with_input(
            BenchmarkId::new("with_limit", limit),
            &limit,
            |b, &limit| {
                let filter = ActivityQuery {
                    limit: Some(limit),
                    ..Default::default()
                };
                b.iter(|| svc.query_activity(&filter).unwrap());
            },
        );
    }

    group.bench_function("with_keyword_filter", |b| {
        let filter = ActivityQuery {
            keyword: Some("msg 5000".into()),
            limit: Some(100),
            ..Default::default()
        };
        b.iter(|| svc.query_activity(&filter).unwrap());
    });

    group.bench_function("with_level_filter", |b| {
        let filter = ActivityQuery {
            levels: Some(vec![ActivityLevel::Info]),
            limit: Some(100),
            ..Default::default()
        };
        b.iter(|| svc.query_activity(&filter).unwrap());
    });

    group.finish();
}

fn bench_log_cleanup(c: &mut Criterion) {
    c.bench_function("log_cleanup_10k", |b| {
        b.iter_with_setup(
            || {
                let tmp = tempdir().unwrap();
                let svc = ObservabilityService::new(tmp.path()).unwrap();
                for i in 0..10_000 {
                    svc.record_activity(&benchmark_activity(format!("msg {i}")))
                        .unwrap();
                }
                (svc, tmp)
            },
            |(svc, _tmp)| {
                svc.cleanup(ObservabilityRetention {
                    activity_days: 0,
                    tool_history_days: 0,
                })
                .unwrap();
            },
        );
    });
}

// ── SDK Config Adapter Benchmarks ──

fn make_test_config() -> serde_json::Value {
    serde_json::json!({
        "type": "stdio",
        "server_parameters": {
            "command": "node",
            "args": ["server.js"],
            "env": {}
        }
    })
}

fn bench_config_load_save(c: &mut Criterion) {
    let tmp = tempdir().unwrap();
    let config = Arc::new(ConfigService::new(tmp.path().to_path_buf()).unwrap());
    let instance_id = "bench-computer";
    config
        .add_computer_instance(ComputerInstance::new(instance_id, "Bench Computer"))
        .unwrap();
    let sdk_config = SdkConfigService::new(config);

    // Pre-fill 50 server configs
    let servers: serde_json::Map<String, serde_json::Value> = (0..50)
        .map(|i| (format!("server-{i}"), make_test_config()))
        .collect();
    let document = ProjectConfigDoc {
        mcp: Some(
            serde_json::json!({ "servers": servers })
                .as_object()
                .unwrap()
                .clone(),
        ),
        ..Default::default()
    };
    sdk_config.save(instance_id, &document).unwrap();

    let mut group = c.benchmark_group("sdk_config");

    group.bench_function("load_50_configs", |b| {
        b.iter(|| sdk_config.load(instance_id));
    });

    group.bench_function("save_50_configs", |b| {
        b.iter(|| sdk_config.save(instance_id, &document).unwrap());
    });

    group.bench_function("add_config", |b| {
        b.iter(|| {
            sdk_config
                .update(
                    instance_id,
                    &[ConfigEdit::new(
                        ConfigEntity::McpServer("bench-add".to_string()),
                        EditIntent::Upsert(make_test_config()),
                    )],
                )
                .unwrap();
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_log_write,
    bench_log_query,
    bench_log_cleanup,
    bench_config_load_save,
);
criterion_main!(benches);
