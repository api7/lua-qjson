window.BENCHMARK_DATA = {
  "lastUpdate": 1780371300469,
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
      }
    ]
  }
}