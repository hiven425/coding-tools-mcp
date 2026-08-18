# 子任务：History v3 状态投影与完整性

- [ ] 1.1 升级 MemoryState v3 和 checkpoint fidelity 契约
  - 证据块: `src-tauri/src/tools/history/mod.rs` 的 bootstrap/checkpoint/state 构建路径
  - 涉及文件: `src-tauri/src/tools/history/mod.rs`, `model.rs`, `storage.rs`
  - _需求: FR-2, FR-6_
- [ ] 1.2 增加 malformed block、派生 snapshot freshness 与 repair 回归
  - 证据块: `src-tauri/src/tools/history/markdown.rs` 解析和 validate 路径
  - 涉及文件: `src-tauri/src/tools/history/markdown.rs`, `storage.rs`, `src-tauri/tests/history_session.rs`
  - _需求: FR-3, FR-6_
