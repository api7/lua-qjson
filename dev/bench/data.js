window.BENCHMARK_DATA = {
  "lastUpdate": 1780376080484,
  "repoUrl": "https://github.com/api7/lua-qjson",
  "entries": {
    "Benchmark": [
      {
        "commit": {
          "author": {
            "email": "membphis@gmail.com",
            "name": "YuanSheng Wang",
            "username": "membphis"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "715b83e97eef4adb6f33bae06af419e61a3bdd82",
          "message": "ci: add performance regression gate with criterion benchmarks (#157)",
          "timestamp": "2026-06-02T11:32:22+08:00",
          "tree_id": "705ef24e8a272bd1902f61a2ea61f52f1107834f",
          "url": "https://github.com/api7/lua-qjson/commit/715b83e97eef4adb6f33bae06af419e61a3bdd82"
        },
        "date": 1780371299577,
        "tool": "cargo",
        "benches": [
          {
            "name": "parse_eager/parse/small_api",
            "value": 2958,
            "range": "± 189",
            "unit": "ns/iter"
          },
          {
            "name": "parse_eager/parse/wide_object",
            "value": 9529,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "parse_eager/parse/deep_nesting",
            "value": 6579,
            "range": "± 32",
            "unit": "ns/iter"
          },
          {
            "name": "parse_lazy/parse/small_api",
            "value": 1030,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "parse_lazy/parse/wide_object",
            "value": 3213,
            "range": "± 131",
            "unit": "ns/iter"
          },
          {
            "name": "parse_lazy/parse/deep_nesting",
            "value": 2768,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "field_access/get_str/model",
            "value": 30,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "field_access/get_f64/max_tokens",
            "value": 46,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "field_access/get_str/nested",
            "value": 73,
            "range": "± 0",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "membphis@gmail.com",
            "name": "YuanSheng Wang",
            "username": "membphis"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "83c26ade1267c41adf54307ba650cf0468a9dfb7",
          "message": "test: expand real-world JSON corpus with production API samples (#158)",
          "timestamp": "2026-06-02T12:35:31+08:00",
          "tree_id": "b6a3fe78c455b3f8d8770a7913c60d7a8b629c98",
          "url": "https://github.com/api7/lua-qjson/commit/83c26ade1267c41adf54307ba650cf0468a9dfb7"
        },
        "date": 1780375066706,
        "tool": "cargo",
        "benches": [
          {
            "name": "parse_eager/parse/small_api",
            "value": 2969,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "parse_eager/parse/wide_object",
            "value": 9455,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "parse_eager/parse/deep_nesting",
            "value": 6489,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "parse_lazy/parse/small_api",
            "value": 1030,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "parse_lazy/parse/wide_object",
            "value": 3267,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "parse_lazy/parse/deep_nesting",
            "value": 2835,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "field_access/get_str/model",
            "value": 30,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "field_access/get_f64/max_tokens",
            "value": 45,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "field_access/get_str/nested",
            "value": 73,
            "range": "± 0",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "membphis@gmail.com",
            "name": "YuanSheng Wang",
            "username": "membphis"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "80dbe2d09f1b01df4976d865f86d90b1f4e050b0",
          "message": "test: add NDJSON edge case coverage (#159)",
          "timestamp": "2026-06-02T12:52:20+08:00",
          "tree_id": "ec40f17adee9c32da40b40445dcc2af50c0f00a4",
          "url": "https://github.com/api7/lua-qjson/commit/80dbe2d09f1b01df4976d865f86d90b1f4e050b0"
        },
        "date": 1780376080146,
        "tool": "cargo",
        "benches": [
          {
            "name": "parse_eager/parse/small_api",
            "value": 3010,
            "range": "± 65",
            "unit": "ns/iter"
          },
          {
            "name": "parse_eager/parse/wide_object",
            "value": 10168,
            "range": "± 59",
            "unit": "ns/iter"
          },
          {
            "name": "parse_eager/parse/deep_nesting",
            "value": 6499,
            "range": "± 39",
            "unit": "ns/iter"
          },
          {
            "name": "parse_lazy/parse/small_api",
            "value": 1045,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "parse_lazy/parse/wide_object",
            "value": 3595,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "parse_lazy/parse/deep_nesting",
            "value": 2699,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "field_access/get_str/model",
            "value": 30,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "field_access/get_f64/max_tokens",
            "value": 46,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "field_access/get_str/nested",
            "value": 74,
            "range": "± 0",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}