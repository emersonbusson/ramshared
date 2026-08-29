# FINDING_ONLY: Fault Injection for Windows Service Control Handler Timeout

## Evidence

The requested task requires implementing fault injection to simulate delayed service stop requests and verify proper reporting of `SERVICE_STOP_PENDING` status. The task constraints strictly confine the scope of changes to `crates/ramshared-winbroker/src/lib.rs` and its related test module.

However, the Windows Service Control Manager (SCM) integration, including the implementation of the `run_service` function and the service control handler (which manages `ServiceControl::Stop` and `ServiceControl::Shutdown` and sets the `ServiceState` status, including `ServiceState::StartPending` and `ServiceState::Stopped`), is entirely contained within `crates/ramshared-winbroker/src/service.rs`.

```rust
// From crates/ramshared-winbroker/src/service.rs
pub fn run_service(config: BrokerConfigV1) -> Result<(), Box<dyn std::error::Error>> {
    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = Arc::clone(&stop);
    let status = service_control_handler::register(SERVICE_NAME, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            handler_stop.store(true, Ordering::Release);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    // ...
}
```

The `lib.rs` file only contains the domain logic for `BrokerSessionCore`, configuration parsing (`BrokerConfigV1`), and related tests. It does not interface with the Windows `windows-service` API or have any knowledge of the service control handler.

It is architecturally impossible and unsafe to implement Windows SCM fault injection within the purely domain-focused `lib.rs`. Moving the SCM handler logic from `service.rs` to `lib.rs` to satisfy the confinement requirement would be a gross architectural violation and would incorrectly mix platform-specific Windows daemon mechanics with core protocol logic.

Therefore, safe code modification to fulfill the task within the strictly confined scope is not possible.
