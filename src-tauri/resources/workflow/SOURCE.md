# WISP workflow worker source

`rice-workflow-worker.exe` is built from the separately maintained AGPL-3.0-only
fork at <https://github.com/yanmengli123/rice-endosperm-workflow>.

The exact source commit, binary SHA-256, engine version and protocol version are
recorded in the adjacent `worker-build.json` file generated during the release
build. The corresponding AGPL license text is included as
`LICENSE-AGPL-3.0.txt`.

The worker communicates with the MIT-licensed desktop shell over the versioned
JSONL stdio protocol `wisp.agent-rpc.v1`. It is not linked into the desktop
binary.
