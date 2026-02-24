# Remote Debugging Guide

## Overview

The Soroban Debugger supports remote debugging, allowing you to debug smart contracts running in CI environments, remote servers, or isolated systems from your local development machine. This enables powerful debugging workflows for production-like scenarios.

## Architecture

The remote debugging feature consists of three main components:

1. **Debug Server** - Runs on the remote system, hosts the contract execution environment
2. **Remote Client** - Connects from your local machine to issue debug commands  
3. **Wire Protocol** - JSON-over-TCP communication protocol for debug operations

## Quick Start

### Starting the Debug Server

On the remote system (or CI environment):

```bash
# Basic server (no authentication)
soroban-debug server --port 9229

# With token authentication
soroban-debug server --port 9229 --token mySecretToken123

# With TLS encryption
soroban-debug server --port 9229 \
  --token mySecretToken123 \
  --tls-cert /path/to/cert.pem \
  --tls-key /path/to/key.pem
```

### Connecting from Local Machine

```bash
# Connect and execute a function
soroban-debug remote \
  --remote localhost:9229 \
  --token mySecretToken123 \
  --contract ./contract.wasm \
  --function increment \
  --args '["user1", 100]'

# Just ping the server
soroban-debug remote \
  --remote localhost:9229 \
  --token mySecretToken123
```

## Features

### Supported Debug Operations

The debugger supports all core debugging operations over TCP:

- **Contract Loading** - Load WASM contracts onto the server
- **Function Execution** - Execute contract functions with arguments
- **Breakpoints** - Set, clear, and list function breakpoints
- **Step Debugging** - Step through execution
- **State Inspection** - Inspect current execution state, call stack
- **Storage Access** - Get and set contract storage
- **Budget Information** - Monitor CPU and memory consumption
- **Snapshot Loading** - Load network snapshots

### Authentication

Token-based authentication prevents unauthorized access:

```bash
# Server with token
soroban-debug server --port 9229 --token "your-secret-token-here"

# Client provides matching token  
soroban-debug remote --remote host:9229 --token "your-secret-token-here"
```

### TLS Encryption

Secure your debug sessions with TLS:

```bash
# Generate self-signed certificate (for testing)
openssl req -x509 -newkey rsa:4096 \
  -keyout key.pem -out cert.pem \
  -days 365 -nodes \
  -subj "/CN=localhost"

# Start server with TLS
soroban-debug server --port 9229 \
  --tls-cert cert.pem \
  --tls-key key.pem \
  --token myToken
```

## Wire Protocol

The debug protocol uses JSON messages over TCP with line-delimited encoding.

### Message Format

```json
{
  "id": 1,
  "request": { ... } 
}
```

```json
{
  "id": 1,
  "response": { ... }
}
```

### Request Types

#### Authenticate
```json
{
  "type": "Authenticate",
  "token": "your-token-here"
}
```

#### LoadContract
```json
{
  "type": "LoadContract",
  "contract_path": "/path/to/contract.wasm"
}
```

#### Execute
```json
{
  "type": "Execute",
  "function": "increment",
  "args": "[\"user1\", 100]"
}
```

#### Step
```json
{
  "type": "Step"
}
```

#### SetBreakpoint
```json
{
  "type": "SetBreakpoint",
  "function": "transfer"
}
```

#### Inspect
```json
{
  "type": "Inspect"
}
```

#### GetStorage
```json
{
  "type": "GetStorage"
}
```

#### GetStack
```json
{
  "type": "GetStack"
}
```

#### GetBudget
```json
{
  "type": "GetBudget"
}
```

### Response Types

#### Authenticated
```json
{
  "type": "Authenticated",
  "success": true,
  "message": "Authentication successful"
}
```

#### ContractLoaded
```json
{
  "type": "ContractLoaded",
  "size": 123456
}
```

#### ExecutionResult
```json
{
  "type": "ExecutionResult",
  "success": true,
  "output": "Ok(100)",
  "error": null
}
```

#### StepResult
```json
{
  "type": "StepResult",
  "paused": true,
  "current_function": "transfer",
  "step_count": 42
}
```

#### InspectionResult
```json
{
  "type": "InspectionResult",
  "function": "transfer",
  "step_count": 42,
  "paused": true,
  "call_stack": ["main", "transfer", "validate"]
}
```

## Use Cases

### CI/CD Debugging

Debug contracts in your CI pipeline:

```yaml
# .github/workflows/debug.yml
steps:
  - name: Start Debug Server
    run: |
      soroban-debug server --port 9229 --token ${{ secrets.DEBUG_TOKEN }} &
      sleep 2

  - name: Debug Contract
    run: |
      soroban-debug remote \
        --remote localhost:9229 \
        --token ${{ secrets.DEBUG_TOKEN }} \
        --contract ./target/wasm32-unknown-unknown/release/contract.wasm \
        --function test_function \
        --args '[1, 2, 3]'
```

### Remote Server Debugging

Debug contracts on staging/production environments:

```bash
# On remote server
ssh user@staging-server
soroban-debug server --port 9229 --token $TOKEN --tls-cert cert.pem --tls-key key.pem

# From local machine (with SSH tunnel)
ssh -L 9229:localhost:9229 user@staging-server
soroban-debug remote --remote localhost:9229 --token $TOKEN --contract local.wasm
```

### Team Debugging Sessions

Multiple developers can connect to the same debug server:

```bash
# Team member starts server
soroban-debug server --port 9229 --token team-debug-session

# Other team members connect  
soroban-debug remote --remote team-lead-ip:9229 --token team-debug-session
```

## Security Best Practices

1. **Always use authentication** in production environments
2. **Enable TLS** for remote connections over the internet
3. **Use strong tokens** - Generate cryptographically random tokens
4. **Firewall rules** - Restrict server port access to known IPs
5. **Rotate tokens** regularly for long-running servers
6. **Monitor connections** - Log all connection attempts

### Generating Secure Tokens

```bash
# Generate a secure random token
openssl rand -hex 32
```

## Troubleshooting

### Connection Refused

```bash
# Check server is running
netstat -an | grep 9229

# Check firewall allows connections
sudo ufw allow 9229/tcp

# Test basic connectivity
telnet host 9229
```

### Authentication Failed

- Verify token matches on both server and client
- Check for whitespace in token strings
- Ensure token was properly set when starting server

### TLS Handshake Errors  
- Verify certificate and key paths are correct
- Check certificate hasn't expired
- Ensure client trusts the certificate (or use self-signed for testing)

## Advanced Usage

### Custom Protocol Extensions

The debug protocol can be extended with custom request/response types:

```rust
// Add to src/server/protocol.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DebugRequest {
    // ... existing variants ...
    CustomCommand { data: String },
}
```

### Programmatic Client Usage

Use the RemoteClient API directly in Rust:

```rust
use soroban_debugger::client::RemoteClient;

let mut client = RemoteClient::connect("localhost:9229", Some("token".to_string()))?;

client.load_contract("contract.wasm")?;
let result = client.execute("increment", Some("[100]"))?;
println!("Result: {}", result);

client.set_breakpoint("transfer")?;
client.step()?;

let (function, step_count, paused, stack) = client.inspect()?;
println!("At function: {:?}, steps: {}", function, step_count);
```

## Future Enhancements

Planned features for remote debugging:

- [ ] WebSocket support for browser-based debugging
- [ ] Multi-session support (concurrent debug sessions)
- [ ] Session recording and replay
- [ ] Visual debugger UI (web interface)
- [ ] Performance profiling over network
- [ ] Distributed debugging (multiple contracts across nodes)

## Related Documentation

- [Plugin API](plugin-api.md) - Extend debugger with custom plugins
- [Storage Snapshots](storage-snapshot.md) - Load network state for debugging
- [Instruction Stepping](instruction-stepping.md) - Low-level instruction debugging

## Contributing

To contribute to remote debugging features:

1. Review the [CONTRIBUTION.md](../CONTRIBUTION.md) guide
2. Check existing issues tagged `remote-debugging`
3. Propose enhancements in GitHub Discussions
4. Submit PRs with tests and documentation

## License

This feature is part of the Soroban Debugger project, licensed under MIT OR Apache-2.0.
