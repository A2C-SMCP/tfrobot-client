use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use smcp_computer::mcp_clients::MCPServerConfig;
use tempfile::tempdir;
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::logger::{LogFilter, LogService};

// ── LogService Benchmarks ──

fn bench_log_write(c: &mut Criterion) {
    let tmp = tempdir().unwrap();
    let svc = LogService::new(tmp.path()).unwrap();

    c.bench_function("log_write_single", |b| {
        b.iter(|| {
            svc.write("info", "bench", "benchmark message", None)
                .unwrap();
        });
    });
}

fn bench_log_query(c: &mut Criterion) {
    let tmp = tempdir().unwrap();
    let svc = LogService::new(tmp.path()).unwrap();

    // Pre-fill data
    for i in 0..10_000 {
        svc.write("info", "bench", &format!("msg {i}"), None)
            .unwrap();
    }

    let mut group = c.benchmark_group("log_query");

    for limit in [10, 100, 1000] {
        group.bench_with_input(
            BenchmarkId::new("with_limit", limit),
            &limit,
            |b, &limit| {
                let filter = LogFilter {
                    limit: Some(limit),
                    ..Default::default()
                };
                b.iter(|| svc.query(&filter).unwrap());
            },
        );
    }

    group.bench_function("with_keyword_filter", |b| {
        let filter = LogFilter {
            keyword: Some("msg 5000".into()),
            limit: Some(100),
            ..Default::default()
        };
        b.iter(|| svc.query(&filter).unwrap());
    });

    group.bench_function("with_level_filter", |b| {
        let filter = LogFilter {
            levels: Some(vec!["info".to_string()]),
            limit: Some(100),
            ..Default::default()
        };
        b.iter(|| svc.query(&filter).unwrap());
    });

    group.finish();
}

fn bench_log_cleanup(c: &mut Criterion) {
    c.bench_function("log_cleanup_10k", |b| {
        b.iter_with_setup(
            || {
                let tmp = tempdir().unwrap();
                let svc = LogService::new(tmp.path()).unwrap();
                for i in 0..10_000 {
                    svc.write("info", "bench", &format!("msg {i}"), None)
                        .unwrap();
                }
                (svc, tmp)
            },
            |(svc, _tmp)| {
                svc.cleanup(0).unwrap();
            },
        );
    });
}

// ── ConfigService Benchmarks ──

fn make_test_config(name: &str) -> MCPServerConfig {
    serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": name,
        "server_parameters": {
            "command": "node",
            "args": ["server.js"],
            "env": {}
        }
    }))
    .unwrap()
}

fn bench_config_load_save(c: &mut Criterion) {
    let tmp = tempdir().unwrap();
    let svc = ConfigService::new(tmp.path().to_path_buf()).unwrap();

    // Pre-fill 50 server configs
    let configs: Vec<MCPServerConfig> = (0..50).map(|i| make_test_config(&format!("server-{i}"))).collect();
    svc.save_configs(&configs).unwrap();

    let mut group = c.benchmark_group("config");

    group.bench_function("load_50_configs", |b| {
        b.iter(|| svc.load_configs().unwrap());
    });

    group.bench_function("save_50_configs", |b| {
        b.iter(|| svc.save_configs(&configs).unwrap());
    });

    group.bench_function("add_config", |b| {
        b.iter(|| {
            svc.add_config(make_test_config("bench-add")).unwrap();
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
