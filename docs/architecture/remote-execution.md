# Remote Execution

Puffer remote execution lets an agent switch its tool calls from the local
machine to a configured remote target. The switching point is the
`RemoteExecution` internal tool.

## Targets

Remote targets are stored in the user config under `remote`.

- SSH hosts are configured with an id, label, `user@host` target, optional port,
  optional cwd, and optional secret reference.
- AgentEnv is configured as an existing account with API URL, workspace UUID,
  credential secret reference, optional runner host, and sandbox defaults.

Credentials are stored through the Puffer secret vault. Tool output reports only
whether a secret is configured.

## Runner Model

All remote tool execution goes through `puffer-tool-runner`.

- Manual/local runner: pass `runnerEndpoint` directly to `enter-remote`.
- SSH: `enter-remote` starts `puffer-tool-runner` over SSH and connects through a
  local tunnel when `runnerEndpoint` is omitted.
- AgentEnv: create a sandbox from a runner image, expose runner port `50051`,
  then `enter-remote` discovers the exposed endpoint. If AgentEnv returns only a
  host port, configure `runnerHost` or pass it in the tool call.

After `enter-remote` succeeds, runner-backed tools such as Bash execute through
the active remote runner until `exit-remote` is called or the session is reset.

## Reconnect Behavior

Remote runner calls use the gRPC client's lazy connection and one-shot retry for
brief `Unavailable` responses. Puffer also wraps runner-backed tool execution
with one remote-target reconnect attempt for transport-style failures.

- SSH reconnect starts a fresh `puffer-tool-runner` over SSH and creates a new
  local tunnel. The saved SSH host id and active remote cwd are reused.
- AgentEnv reconnect rediscovers the exposed runner port, using the configured
  `runnerHost` when AgentEnv returns only a host port. If discovery fails but an
  explicit endpoint is saved, Puffer falls back to that endpoint.
- Manual endpoint reconnect reuses the saved `runnerEndpoint`.

The failed tool call is retried once after reconnect. Puffer does not reconnect
or retry normal runner-side tool failures such as permission denials, missing
files, or commands that exit non-zero.

## Exactly-Once Tool Retry

Runner-backed tool requests carry an optional `request_id` idempotency key.
Remote Puffer sessions set this key before sending a tool call. The
`puffer-tool-runner` gRPC service keeps an in-memory replay cache keyed by
`request_id`:

- The first request with a key executes normally and records the exact stream of
  stdout, stderr, and final completion/failure events.
- A duplicate request with the same key and the same request payload waits for
  the first execution to finish, then replays the recorded events without
  invoking the underlying tool again.
- Reusing a key for a different request is rejected.

This makes reconnect retries exactly-once for the common failure mode where the
gRPC stream or SSH tunnel drops but the remote runner process continues. The
cache is process-local and bounded to completed entries; it does not survive a
remote runner process restart.

## Tool Actions

`RemoteExecution` supports:

- `status` / `list-targets`
- `enter-remote`
- `exit-remote`
- `ssh:create-config`
- `ssh:delete-config`
- `agentenv:list-sandboxes`
- `agentenv:create-sandbox`
- `agentenv:terminate-sandbox`
- `agentenv:delete-sandbox`
- `agentenv:snapshot-sandbox`
- `agentenv:expose-port`
- `agentenv:list-ports`

## AgentEnv Manual Test

1. Configure AgentEnv in Settings or onboarding:
   - API URL
   - full workspace UUID
   - API key or access token
   - runner host if exposed ports return only a host port
2. Create a runner sandbox:

   ```json
   {
     "action": "agentenv:create-sandbox",
     "name": "puffer-runner-test",
     "image": "docker.io/<namespace>/<runner-image>:<tag>",
     "gpuCount": 0,
     "runnerAuthToken": "test-token"
   }
   ```

3. Wait for the sandbox to be running.
4. Enter the sandbox without `runnerEndpoint`:

   ```json
   {
     "action": "enter-remote",
     "targetType": "agentenv",
     "sandboxId": "<sandbox-id>",
     "runnerAuthToken": "test-token"
   }
   ```

5. Run Bash:

   ```text
   pwd
   ```

   Expected output is the remote runner cwd, currently `/user` for AgentEnv.

## SSH Manual Test

1. Confirm SSH key auth works outside Puffer:

   ```sh
   ssh -o BatchMode=yes -p 22 user@host "echo ok"
   ```

2. Configure the host:

   ```json
   {
     "action": "ssh:create-config",
     "ssh": {
       "id": "devbox",
       "label": "Dev Box",
       "target": "user@host",
       "port": 22,
       "cwd": "/path/on/remote"
     }
   }
   ```

3. Enter the host:

   ```json
   {
     "action": "enter-remote",
     "targetType": "ssh",
     "hostId": "devbox"
   }
   ```

4. Run Bash:

   ```text
   hostname
   ```

   Expected output is the remote host name. On Windows hosts, commands execute
   through the Windows-compatible shell fallback.

## Verification

Useful local checks:

```sh
cargo build -p puffer-cli --quiet
cargo test -p puffer-tools --quiet
npm --prefix apps/puffer-desktop run check
```

`puffer-tools` includes localhost socket tests; under a restricted sandbox, run
it with permissions that allow localhost sockets.
