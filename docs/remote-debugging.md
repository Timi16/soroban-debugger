# Remote Debugging Guide

## Overview

The Soroban Debugger supports remote debugging for CI jobs, isolated hosts, and other non-local environments. Remote mode is powerful, but it should be treated as a security-sensitive control plane: whoever can talk to the debug server can load contracts, inspect state, and drive execution.

This guide focuses on two things:

1. How the feature works.
2. How to deploy it without making unsafe assumptions about tokens or transport security.

> **Note: Remote client mode is supported in both the CLI and VS Code extension.** Use the `soroban-debug remote` command in the CLI, or set `request: "attach"` in your VS Code `launch.json`. For a full breakdown of what each surface supports, see the [Feature Matrix](feature-matrix.md#remote-debugging).

## Architecture

Remote debugging has three components:

1. **Debug Server**: runs where contract execution happens.
2. **Remote Client**: connects from your workstation or automation.
3. **Wire Protocol**: line-delimited JSON requests and responses over TCP.

## Quick Start

### Start a server

On the remote system:

```bash
# Only for trusted local development on an isolated machine.
soroban-debug server --port 9229

# Token-protected server on a trusted private network.
soroban-debug server --port 9229 --token "$SOROBAN_DEBUG_TOKEN"

# Token + TLS on an untrusted network.
soroban-debug server --port 9229 \
  --token "$SOROBAN_DEBUG_TOKEN" \
  --tls-cert /path/to/cert.pem \
  --tls-key /path/to/key.pem
```

### Connect from a client


soroban-debug remote \
  --remote localhost:9229 \
  --token "$SOROBAN_DEBUG_TOKEN" \
  --tls-cert /path/to/client-cert.pem \
  --tls-key /path/to/client-key.pem \
  --tls-ca /path/to/server-ca.pem \
  --contract ./contract.wasm \
  --function increment \
  --args '["user1", 100]'
```

### Timeouts and Retries (network instability)

Remote sessions often run across CI, containers, or flaky links. The remote client supports deterministic timeouts and controlled retries for **idempotent** operations.

- Retries apply to: `Ping`, `Inspect`, `GetStorage` (and other read-only state queries).
- No-retry semantics apply to: execution/stepping commands (e.g. `Execute`, `Continue`, `StepIn/Next/StepOut`) to avoid unintended side effects.

Example (tighter ping timeout, more retries):

```bash
soroban-debug remote \
  --remote host:9229 \
  --token "$TOKEN" \
  --ping-timeout-ms 1000 \
  --retry-attempts 5 \
  --retry-base-delay-ms 100 \
  --retry-max-delay-ms 1500
```

## Features

- A token protects **authentication**, not **confidentiality**.
- If you run remote debugging without TLS, the traffic should be treated as plaintext.
- On an untrusted network, a token alone is not sufficient protection.
- Anyone who captures the token can authenticate until that token is rotated.

### Recommended deployment patterns

Use one of these patterns:

1. **Loopback only**: bind or expose the service only to `127.0.0.1`, then use SSH port forwarding.
2. **Private network boundary**: run the debug server on a non-public subnet and restrict access with firewall rules or security groups.
3. **TLS termination in front of the server**: place the server behind a reverse proxy, service mesh sidecar, or tunnel that provides authenticated encrypted transport.
4. **Native TLS in the debugger**: use `--tls-cert` and `--tls-key` when the server itself is directly reachable over an untrusted network.

### Token handling guidance

- Generate tokens with a cryptographically secure RNG.
- Prefer at least 32 random bytes encoded as hex.
- Do not hardcode tokens in committed scripts or `launch.json`.
- Avoid shell history leaks by passing tokens through environment variables or secret stores instead of typing them inline.
- Rotate tokens after incident response, staff changes, CI secret exposure, or any long-lived remote-debug session.
- Scope tokens to the smallest environment possible. Do not reuse one token across staging, CI, and personal workstations.

### Generate a strong token

```bash
openssl rand -hex 32
```

### Safer shell usage

Prefer:

```bash
export SOROBAN_DEBUG_TOKEN="$(openssl rand -hex 32)"
soroban-debug server --port 9229 --token "$SOROBAN_DEBUG_TOKEN"
```

Avoid:

```bash
soroban-debug server --port 9229 --token mySecretToken123
```

The second form is easy to leak through shell history, process listings, shared transcripts, and copied terminal logs.

## Transport Hardening

### TLS

Use TLS whenever the server is reachable beyond a tightly controlled private boundary.

```bash
openssl req -x509 -newkey rsa:4096 \
  -keyout key.pem -out cert.pem \
  -days 365 -nodes \
  -subj "/CN=localhost"

soroban-debug server --port 9229 \
  --token "$SOROBAN_DEBUG_TOKEN" \
  --tls-cert cert.pem \
  --tls-key key.pem
```

### TLS termination

If you already have ingress infrastructure, it is often simpler to terminate TLS before the debugger:

- SSH tunnel
- Nginx / Envoy / HAProxy
- Kubernetes ingress or service mesh
- Cloud load balancer with private backend networking

When doing this:

- Keep the debugger itself on a private interface.
- Restrict who can reach the TLS terminator.
- Treat the segment between terminator and debugger as sensitive internal traffic.

### Firewall and network boundaries

- Allow only explicitly trusted source IPs.
- Do not expose the remote debug port directly to the public internet.
- Prefer short-lived port openings for incident or debugging windows.
- Remove ingress rules when the debug session ends.

## Authentication Behavior

The server accepts token authentication through the `Authenticate` request. Authentication failures are intentionally generic. Tokens are redacted in debugger logging and should not appear in normal client/server error messages.

Example request shape:

```json
{
  "id": 1,
  "request": {
    "type": "Authenticate",
    "token": "your-token-here"
  }
}
```

Example response:

```json
{
  "id": 1,
  "response": {
    "type": "Authenticated",
    "success": false,
    "message": "Authentication failed"
  }
}
```

## Graceful Shutdown

The debug server handles system signals to enable clean shutdown:

### Supported signals

- **SIGINT** (Ctrl+C): Graceful termination
- **SIGTERM** (Unix/Linux): Graceful termination

### Shutdown behavior

When the server receives a shutdown signal:

1. The listener socket is closed immediately
2. New connection attempts are rejected
3. Existing client connections continue until they send a disconnect request or connection loss
4. All resources are released
5. The process exits cleanly

### Clean termination example

```bash
soroban-debug server --port 9229 &
SERVER_PID=$!

# Run your debug session
soroban-debug remote --remote localhost:9229 --contract ./contract.wasm ...

# Clean shutdown
kill $SERVER_PID

# Server logs will show:
# INFO: Shutdown message
```

### CI and automation cleanup

When running the server in CI, ensure proper cleanup:

```yaml
steps:
  - name: Start Debug Server
    run: |
      soroban-debug server --port 9229 --token "${{ secrets.DEBUG_TOKEN }}" &
      echo $! > server.pid
      sleep 1

  - name: Run Tests
    run: |
      # Your test commands here
      soroban-debug remote ...

  - name: Cleanup
    if: always()
    run: |
      [ -f server.pid ] && kill $(cat server.pid) || true
      wait
```

## Supported Operations

The remote protocol supports:

- Contract loading
- Function execution
- Breakpoints
- Step debugging
- State inspection
- Storage access
- Budget inspection
- Snapshot loading

## Operational Checklist

Before exposing a debug server remotely, confirm all of the following:

- Authentication token enabled.
- Token generated randomly and stored outside source control.
- TLS enabled or traffic constrained to a trusted private boundary.
- Firewall rules limited to known sources.
- Rotation plan documented.
- Session owner knows how to revoke the token after the session.

## CI and Automation Guidance

For CI:

- Use the platform secret manager, not plaintext workflow YAML.
- Prefer ephemeral runners and per-job tokens.
- Tear down the debug server after the job completes.
- Do not print the token, even masked, unless your CI provider guarantees redaction.

Example:

```yaml
steps:
  - name: Start Debug Server
    run: |
      soroban-debug server --port 9229 --token "${{ secrets.DEBUG_TOKEN }}" &
      sleep 2

  - name: Remote Debug
    run: |
      soroban-debug remote \
        --remote localhost:9229 \
        --token "${{ secrets.DEBUG_TOKEN }}" \
        --contract ./target/wasm32-unknown-unknown/release/contract.wasm \
        --function test_function \
        --args '[1, 2, 3]'
```

## Troubleshooting

### Authentication failed

- Confirm the server and client use the same token value.
- Check for leading or trailing whitespace.
- Rotate the token if it may have been copied incorrectly or exposed.
- If you are using a secret manager, confirm the runtime actually injected the latest value.

### Connection refused

- Confirm the server is listening on the expected host and port.
- Check firewall or security group rules.
- If using an SSH tunnel, verify the local forward is active.

### TLS handshake errors

- Confirm the certificate and key match.
- Check certificate expiry and trust configuration.
- If using TLS termination, verify the proxy is forwarding traffic to the correct backend port.

## Notes on Logging

The debugger redacts authentication tokens in normal request logging and auth-failure surfacing. This is a defense-in-depth measure, not a reason to relax secret handling elsewhere. You should still assume tokens can leak through:

- shell history
- copied terminal transcripts
- process inspection tools
- CI misconfiguration
- external reverse-proxy logs

## Related Documentation

- [Plugin API](plugin-api.md)
- [Storage Snapshots](storage-snapshot.md)
- [Instruction Stepping](instruction-stepping.md)
