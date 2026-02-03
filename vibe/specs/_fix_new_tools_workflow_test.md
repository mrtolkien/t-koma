# Spec: Make new_tools_workflow test robust to extra tool reads

## Context

The live-tools conversation workflow test in
`t-koma-gateway/tests/conversation/new_tools_workflow.rs` expects the last tool
used in Step 6 (edit file) to be `replace`. In practice, the model may use
`read_file` after `replace` to verify changes, causing the assertion to fail
even when the file edit succeeded.

## Goals

- Make the Step 6 verification tolerant of a `read_file` after `replace`.
- Preserve validation that the `replace` tool is used during the edit step.
- Keep the test’s overall intent intact and still verify file contents.

## Non-goals

- Changing tool behavior or prompts.
- Modifying production code in `t-koma-gateway` or `t-koma-db`.

## Approach

1. Add helper logic in the test to capture the tool-use list before Step 6 and
   after Step 6.
2. Assert that at least one newly added tool use in Step 6 is `replace`.
3. Keep the existing file-content verification as the primary signal that the
   edit was applied.

## Tests

- Rely on existing live-test execution:
  - `cargo test --features live-tests conversation::new_tools_workflow`
