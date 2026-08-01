window.BENCHMARK_DATA = {
  "lastUpdate": 1785574031541,
  "repoUrl": "https://github.com/A2C-SMCP/tfrobot-client",
  "entries": {
    "Benchmark": [
      {
        "commit": {
          "author": {
            "email": "394943230@qq.com",
            "name": "hrz",
            "username": "hrz394943230"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "3ce64c9b3cc5d1f5ada837f0510e6195a41ec235",
          "message": "Merge pull request #38 from A2C-SMCP/develop\n\nrelease: merge develop into main",
          "timestamp": "2026-07-30T17:51:40+08:00",
          "tree_id": "a27952ed7fc3d2aa1f24f24174570b61f841d6ee",
          "url": "https://github.com/A2C-SMCP/tfrobot-client/commit/3ce64c9b3cc5d1f5ada837f0510e6195a41ec235"
        },
        "date": 1785406947557,
        "tool": "cargo",
        "benches": [
          {
            "name": "log_write_single",
            "value": 1074527,
            "range": "± 86411",
            "unit": "ns/iter"
          },
          {
            "name": "log_query/with_limit/10",
            "value": 21507,
            "range": "± 1054",
            "unit": "ns/iter"
          },
          {
            "name": "log_query/with_limit/100",
            "value": 78274,
            "range": "± 382",
            "unit": "ns/iter"
          },
          {
            "name": "log_query/with_limit/1000",
            "value": 626870,
            "range": "± 2262",
            "unit": "ns/iter"
          },
          {
            "name": "log_query/with_keyword_filter",
            "value": 1977184,
            "range": "± 12693",
            "unit": "ns/iter"
          },
          {
            "name": "log_query/with_level_filter",
            "value": 5403240,
            "range": "± 197263",
            "unit": "ns/iter"
          },
          {
            "name": "log_cleanup_10k",
            "value": 16757509,
            "range": "± 487560",
            "unit": "ns/iter"
          },
          {
            "name": "sdk_config/load_50_configs",
            "value": 414464,
            "range": "± 6742",
            "unit": "ns/iter"
          },
          {
            "name": "sdk_config/save_50_configs",
            "value": 524408,
            "range": "± 139533",
            "unit": "ns/iter"
          },
          {
            "name": "sdk_config/add_config",
            "value": 1138403,
            "range": "± 11932",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "394943230@qq.com",
            "name": "hrz",
            "username": "hrz394943230"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "0e4e4f3463f8aec9d68bec33f6fc207c2c5264d7",
          "message": "Merge pull request #42 from A2C-SMCP/develop\n\nrelease: TFRobot Client v0.2.0",
          "timestamp": "2026-08-01T16:24:52+08:00",
          "tree_id": "87ae5e478977d2e7f34af30b2a9f276f5226187c",
          "url": "https://github.com/A2C-SMCP/tfrobot-client/commit/0e4e4f3463f8aec9d68bec33f6fc207c2c5264d7"
        },
        "date": 1785574030808,
        "tool": "cargo",
        "benches": [
          {
            "name": "log_write_single",
            "value": 1039399,
            "range": "± 55039",
            "unit": "ns/iter"
          },
          {
            "name": "log_query/with_limit/10",
            "value": 21686,
            "range": "± 1376",
            "unit": "ns/iter"
          },
          {
            "name": "log_query/with_limit/100",
            "value": 80266,
            "range": "± 256",
            "unit": "ns/iter"
          },
          {
            "name": "log_query/with_limit/1000",
            "value": 644880,
            "range": "± 5527",
            "unit": "ns/iter"
          },
          {
            "name": "log_query/with_keyword_filter",
            "value": 1959044,
            "range": "± 29091",
            "unit": "ns/iter"
          },
          {
            "name": "log_query/with_level_filter",
            "value": 5516355,
            "range": "± 24765",
            "unit": "ns/iter"
          },
          {
            "name": "log_cleanup_10k",
            "value": 16658954,
            "range": "± 257044",
            "unit": "ns/iter"
          },
          {
            "name": "sdk_config/load_50_configs",
            "value": 425172,
            "range": "± 3730",
            "unit": "ns/iter"
          },
          {
            "name": "sdk_config/save_50_configs",
            "value": 504851,
            "range": "± 24378",
            "unit": "ns/iter"
          },
          {
            "name": "sdk_config/add_config",
            "value": 1150634,
            "range": "± 26187",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}